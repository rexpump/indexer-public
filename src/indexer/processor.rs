//! Transaction processing logic for RexSwap indexer

use alloy::primitives::{Address, B256, U256};
use anyhow::Result;
use chrono::{TimeZone, Utc};
use sha3::{Digest, Keccak256};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tracing::debug;

use crate::db::{ClickHouseClient, LiquidityChangeRecord, PoolRecord, SwapRecord};
use crate::events::{DecodedEvent, LiquidityChangeEvent, PoolInitEvent, PoolTemplateEvent, SwapEvent};

/// Processes decoded events and stores them in the database
pub struct EventProcessor {
    db: Arc<ClickHouseClient>,
    network: String,
    call_index: AtomicU32,
}

impl EventProcessor {
    pub fn new(db: Arc<ClickHouseClient>, network: String) -> Self {
        Self {
            db,
            network,
            call_index: AtomicU32::new(0),
        }
    }

    /// Process a decoded event from transaction calldata
    pub async fn process_decoded_event(
        &self,
        event: &DecodedEvent,
        block_number: u64,
        block_time: u64,
        tx_hash: &str,
        tx_index: u32,
    ) -> Result<()> {
        match event {
            DecodedEvent::Swap(swap) => {
                let record = self.create_swap_record(swap, block_number, block_time, tx_hash, tx_index);
                self.db.insert_swap(&record).await?;
            }
            DecodedEvent::LiquidityChange(liq) => {
                let record = self.create_liquidity_record(liq, block_number, block_time, tx_hash);
                self.db.insert_liquidity_change(&record).await?;
            }
            DecodedEvent::PoolInit(pool) => {
                let record = self.create_pool_record(pool, block_number, block_time);
                self.db.insert_pool(&record).await?;
            }
            DecodedEvent::PoolTemplate(template) => {
                debug!(
                    "Pool template {} with fee_rate {} and hooks {:?}",
                    template.pool_idx, template.fee_rate, template.hooks
                );
            }
            DecodedEvent::KnockoutCross(cross) => {
                debug!("Knockout cross at tick {}", cross.tick);
            }
            DecodedEvent::Unknown => {}
        }
        Ok(())
    }

    /// Create a swap record from decoded swap event
    fn create_swap_record(
        &self,
        swap: &SwapEvent,
        block_number: u64,
        block_time: u64,
        tx_hash: &str,
        tx_index: u32,
    ) -> SwapRecord {
        let call_index = self.call_index.fetch_add(1, Ordering::SeqCst);
        let pool_id = self.compute_pool_hash(&swap.base, &swap.quote, &swap.pool_idx);

        // Calculate price if we have flow data
        let price = if swap.quote_flow != 0 {
            Some((swap.base_flow.abs() as f64) / (swap.quote_flow.abs() as f64))
        } else {
            None
        };

        SwapRecord {
            id: format!("{}-{}", tx_hash, call_index),
            transaction_hash: tx_hash.to_string(),
            call_index,
            user_address: "0x0".to_string(), // Not available from calldata
            pool_id: format!("{:?}", pool_id),
            is_buy: swap.is_buy,
            is_vault: false,
            in_base_qty: swap.in_base_qty,
            qty: swap.qty.to_string(),
            limit_price: Some(fixed_to_float(swap.limit_price)),
            min_out: Some(swap.min_out.to_string()),
            base_flow: swap.base_flow.to_string(),
            quote_flow: swap.quote_flow.to_string(),
            price,
            call_source: swap.call_source.clone(),
            dex: "rexswap".to_string(),
            hook_delta_base: swap.hook_delta_base,
            hook_delta_quote: swap.hook_delta_quote,
            hook_fee_override: swap.hook_fee_override,
            block_number,
            block_time: format_timestamp(block_time),
            transaction_index: tx_index,
            network: self.network.clone(),
        }
    }

    /// Create a liquidity change record from decoded event
    fn create_liquidity_record(
        &self,
        liq: &LiquidityChangeEvent,
        block_number: u64,
        block_time: u64,
        tx_hash: &str,
    ) -> LiquidityChangeRecord {
        let call_index = self.call_index.fetch_add(1, Ordering::SeqCst);
        let pool_id = self.compute_pool_hash(&liq.base, &liq.quote, &liq.pool_idx);

        LiquidityChangeRecord {
            id: format!("{}-{}", tx_hash, call_index),
            transaction_hash: tx_hash.to_string(),
            call_index,
            pool_id: format!("{:?}", pool_id),
            user_address: "0x0".to_string(),
            is_vault: false,
            position_type: liq.position_type.to_string(),
            change_type: liq.change_type.to_string(),
            bid_tick: Some(liq.bid_tick),
            ask_tick: Some(liq.ask_tick),
            is_bid: liq.is_bid,
            liq: liq.liq.map(|l| l.to_string()),
            base_flow: Some(liq.base_flow.to_string()),
            quote_flow: Some(liq.quote_flow.to_string()),
            call_source: liq.call_source.clone(),
            pivot_time: liq.pivot_time,
            hook_delta: liq.hook_delta,
            block_number,
            block_time: format_timestamp(block_time),
            network: self.network.clone(),
        }
    }

    /// Create a pool record from decoded pool init event
    fn create_pool_record(
        &self,
        pool: &PoolInitEvent,
        block_number: u64,
        block_time: u64,
    ) -> PoolRecord {
        let pool_id = self.compute_pool_hash(&pool.base, &pool.quote, &pool.pool_idx);

        PoolRecord {
            id: format!("{:?}", pool_id),
            base: format!("{:?}", pool.base),
            quote: format!("{:?}", pool.quote),
            pool_idx: pool.pool_idx.to_string(),
            template_id: format!("{}", pool.pool_idx),
            hooks_address: "0x0".to_string(),
            block_create: block_number,
            time_create: format_timestamp(block_time),
            network: self.network.clone(),
        }
    }

    /// Compute pool hash from base, quote, and pool index
    fn compute_pool_hash(&self, base: &Address, quote: &Address, pool_idx: &U256) -> B256 {
        let mut hasher = Keccak256::new();

        // ABI encode: address, address, uint256
        let mut encoded = Vec::with_capacity(96);

        // Pad addresses to 32 bytes
        encoded.extend_from_slice(&[0u8; 12]);
        encoded.extend_from_slice(base.as_slice());

        encoded.extend_from_slice(&[0u8; 12]);
        encoded.extend_from_slice(quote.as_slice());

        // Pool idx as 32 bytes
        let pool_idx_bytes: [u8; 32] = pool_idx.to_be_bytes();
        encoded.extend_from_slice(&pool_idx_bytes);

        hasher.update(&encoded);
        B256::from_slice(&hasher.finalize())
    }
}

/// Convert Q64.64 fixed point to floating point
fn fixed_to_float(x: u128) -> f64 {
    // sqrt(price) in Q64.64, so price = x^2 / 2^128
    let x_f64 = x as f64;
    (x_f64 * x_f64) / (2f64.powi(128))
}

/// Format unix timestamp to ClickHouse datetime string
fn format_timestamp(ts: u64) -> String {
    let dt = Utc.timestamp_opt(ts as i64, 0).unwrap();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

