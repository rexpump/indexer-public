//! DTOs for RexSwap DEX endpoints

use serde::Serialize;

use crate::db::queries::rexswap::{LiquidityChangeRow, PoolRow, SwapRow, UserPosition};

/// Pool response
#[derive(Debug, Serialize)]
pub struct PoolDto {
    pub id: String,
    pub base: String,
    pub quote: String,
    pub pool_idx: String,
    pub template_id: String,
    pub hooks_address: String,
    pub block_create: u64,
    pub time_create: String,
}

impl From<PoolRow> for PoolDto {
    fn from(row: PoolRow) -> Self {
        Self {
            id: row.id,
            base: row.base,
            quote: row.quote,
            pool_idx: row.pool_idx,
            template_id: row.template_id,
            hooks_address: row.hooks_address,
            block_create: row.block_create,
            time_create: row.time_create,
        }
    }
}

/// Swap response
#[derive(Debug, Serialize)]
pub struct SwapDto {
    pub id: String,
    pub transaction_hash: String,
    pub call_index: u32,
    pub user_address: String,
    pub pool_id: String,
    /// true = buy base token, false = sell base token
    pub is_buy: bool,
    /// true = quantity is in base token
    pub in_base_qty: bool,
    /// Input quantity (decimal string)
    pub qty: String,
    /// Change in base token balance (can be negative)
    pub base_flow: String,
    /// Change in quote token balance (can be negative)
    pub quote_flow: String,
    /// Execution price (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    pub call_source: String,
    pub block_number: u64,
    pub block_time: String,
}

impl From<SwapRow> for SwapDto {
    fn from(row: SwapRow) -> Self {
        Self {
            id: row.id,
            transaction_hash: row.transaction_hash,
            call_index: row.call_index,
            user_address: row.user_address,
            pool_id: row.pool_id,
            is_buy: row.is_buy == 1,
            in_base_qty: row.in_base_qty == 1,
            qty: row.qty,
            base_flow: row.base_flow,
            quote_flow: row.quote_flow,
            price: row.price,
            call_source: row.call_source,
            block_number: row.block_number,
            block_time: row.block_time,
        }
    }
}

/// Liquidity change response
#[derive(Debug, Serialize)]
pub struct LiquidityChangeDto {
    pub id: String,
    pub transaction_hash: String,
    pub call_index: u32,
    pub pool_id: String,
    pub user_address: String,
    /// Position type: "ambient", "range", "knockout"
    pub position_type: String,
    /// Change type: "mint", "burn", "harvest"
    pub change_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bid_tick: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_tick: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_flow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_flow: Option<String>,
    pub block_number: u64,
    pub block_time: String,
}

impl From<LiquidityChangeRow> for LiquidityChangeDto {
    fn from(row: LiquidityChangeRow) -> Self {
        Self {
            id: row.id,
            transaction_hash: row.transaction_hash,
            call_index: row.call_index,
            pool_id: row.pool_id,
            user_address: row.user_address,
            position_type: row.position_type,
            change_type: row.change_type,
            bid_tick: row.bid_tick,
            ask_tick: row.ask_tick,
            liquidity: row.liq,
            base_flow: row.base_flow,
            quote_flow: row.quote_flow,
            block_number: row.block_number,
            block_time: row.block_time,
        }
    }
}

/// User position summary
#[derive(Debug, Serialize)]
pub struct UserPositionDto {
    pub pool_id: String,
    pub position_type: String,
    pub mint_count: u64,
    pub burn_count: u64,
    pub last_activity_block: u64,
}

impl From<UserPosition> for UserPositionDto {
    fn from(row: UserPosition) -> Self {
        Self {
            pool_id: row.pool_id,
            position_type: row.position_type,
            mint_count: row.mint_count,
            burn_count: row.burn_count,
            last_activity_block: row.last_activity_block,
        }
    }
}
