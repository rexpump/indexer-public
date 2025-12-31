//! ClickHouse database client
//!
//! Handles connection, schema initialization, and data insertion.

use anyhow::{Context, Result};
use clickhouse::{Client, Row};
use serde::Deserialize;
use tracing::{debug, info};

use crate::config::ClickHouseConfig;

/// Escape string for safe insertion into ClickHouse SQL
/// Prevents SQL injection by escaping backslashes, single quotes, and null bytes
pub fn escape_clickhouse_string(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('\'', "\\'")
     .replace('\0', "")  // Remove null bytes (can break parsing)
}

/// Escape optional string and wrap in quotes, or return NULL
fn escape_optional(opt: Option<&String>) -> String {
    match opt {
        Some(s) => format!("'{}'", escape_clickhouse_string(s)),
        None => "NULL".to_string(),
    }
}

use super::schema::{
    collect_create_schemas, collect_drop_schemas, IndexerStatus,
    // RexSwap records
    LiquidityChangeRecord, PoolRecord, SwapRecord,
    // ERC-20 records
    TokenTransferRecord,
    // ERC-721 (NFT) records
    NftTransferRecord,
    // RexPump records
    BidWallEventRecord, FairLaunchEventRecord, FeeEscrowEventRecord, ReferrerFeeRecord,
    RexPumpFeeDistributionRecord, RexPumpPoolRecord, RexPumpPoolStateRecord, RexPumpSwapRecord,
};

pub struct ClickHouseClient {
    client: Client,
    #[allow(dead_code)]
    database: String,
}

impl ClickHouseClient {
    pub async fn new(config: &ClickHouseConfig) -> Result<Self> {
        let client = Client::default()
            .with_url(&config.url())
            .with_user(&config.user)
            .with_password(&config.password)
            .with_database(&config.database);

        // Test connection
        client
            .query("SELECT 1")
            .execute()
            .await
            .context("Failed to connect to ClickHouse")?;

        info!("Connected to ClickHouse at {}", config.url());

        Ok(Self {
            client,
            database: config.database.clone(),
        })
    }

    /// Initialize database schema from all modules
    pub async fn init_schema(&self) -> Result<()> {
        info!("Creating database schema...");

        // Create database first (if not exists)
        info!("Ensuring database '{}' exists...", self.database);
        self.client
            .query(&format!("CREATE DATABASE IF NOT EXISTS {}", self.database))
            .execute()
            .await
            .with_context(|| format!("Failed to create database '{}'", self.database))?;

        for (module_name, sql) in collect_create_schemas() {
            info!("Initializing schema for module: {}", module_name);

            for statement in sql.split(';') {
                // Remove SQL comments and trim
                let statement: String = statement
                    .lines()
                    .filter(|line| !line.trim().starts_with("--"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let statement = statement.trim();
                
                if statement.is_empty() {
                    continue;
                }

                debug!("Executing: {}...", &statement[..statement.len().min(50)]);
                self.client
                    .query(statement)
                    .execute()
                    .await
                    .with_context(|| format!("Failed to execute: {}", statement))?;
            }
        }

        info!("Schema created successfully");
        Ok(())
    }

    /// Update TTL for all tables based on config
    /// ttl_days = 0 means remove TTL (store forever)
    pub async fn update_ttl(&self, ttl_config: &crate::config::TtlConfig) -> Result<()> {
        info!("Updating TTL settings...");

        let ttl_updates = vec![
            // ERC tokens
            ("token_transfers", "block_time", ttl_config.erc20_transfers),
            ("nft_transfers", "block_time", ttl_config.erc721_transfers),
            // RexSwap DEX
            ("swaps", "block_time", ttl_config.rexswap_swaps),
            ("pools", "time_create", ttl_config.rexswap_pools),
            ("liquidity_changes", "block_time", ttl_config.rexswap_liquidity),
            // RexPump Launchpad
            ("rexpump_swaps", "block_time", ttl_config.rexpump_swaps),
            ("rexpump_pools", "block_time", ttl_config.rexpump_pools),
        ];

        for (table, time_column, ttl_days) in ttl_updates {
            // Check if table exists first
            let exists = self.table_exists(table).await?;
            if !exists {
                debug!("Table {} does not exist, skipping TTL update", table);
                continue;
            }

            let sql = if ttl_days > 0 {
                format!(
                    "ALTER TABLE {} MODIFY TTL {} + INTERVAL {} DAY",
                    table, time_column, ttl_days
                )
            } else {
                format!("ALTER TABLE {} REMOVE TTL", table)
            };

            info!(
                "Setting TTL for {}: {} days{}",
                table,
                ttl_days,
                if ttl_days == 0 { " (disabled)" } else { "" }
            );

            match self.client.query(&sql).execute().await {
                Ok(_) => debug!("TTL updated for {}", table),
                Err(e) => {
                    let err_msg = e.to_string().to_lowercase();
                    // REMOVE TTL fails if there's no TTL - that's ok
                    // ClickHouse returns: "Table doesn't have any table TTL expression, cannot remove"
                    if ttl_days == 0 && (err_msg.contains("ttl") && err_msg.contains("cannot remove")) {
                        debug!("Table {} has no TTL to remove, skipping", table);
                    } else {
                        return Err(e).with_context(|| format!("Failed to update TTL for {}", table));
                    }
                }
            }
        }

        info!("TTL settings updated successfully");
        Ok(())
    }

    /// Check if a table exists
    async fn table_exists(&self, table: &str) -> Result<bool> {
        #[derive(Row, Deserialize)]
        struct CountResult {
            count: u64,
        }

        let sql = format!(
            "SELECT count() as count FROM system.tables WHERE database = '{}' AND name = '{}'",
            self.database, table
        );

        let result = self
            .client
            .query(&sql)
            .fetch_one::<CountResult>()
            .await
            .context("Failed to check table existence")?;

        Ok(result.count > 0)
    }

    /// Drop all tables from all modules
    pub async fn drop_all_tables(&self) -> Result<()> {
        info!("Dropping all tables...");

        for (module_name, sql) in collect_drop_schemas() {
            info!("Dropping tables for module: {}", module_name);

            for statement in sql.split(';') {
                let statement = statement.trim();
                if statement.is_empty() {
                    continue;
                }

                self.client
                    .query(statement)
                    .execute()
                    .await
                    .with_context(|| format!("Failed to execute: {}", statement))?;
            }
        }

        info!("All tables dropped");
        Ok(())
    }

    // ========================================================================
    // Indexer state methods
    // ========================================================================

    /// Get indexer status
    pub async fn get_indexer_status(&self, network: &str) -> Result<IndexerStatus> {
        #[derive(Row, Deserialize)]
        struct LastBlock {
            last_synced_block: u64,
        }

        #[derive(Row, Deserialize)]
        struct CountResult {
            count: u64,
        }

        let last_block = self
            .client
            .query(&format!(
                "SELECT last_synced_block FROM indexer_state FINAL WHERE network = '{}'",
                network
            ))
            .fetch_optional::<LastBlock>()
            .await
            .context("Failed to fetch last synced block")?
            .map(|r| r.last_synced_block)
            .unwrap_or(0);

        let swaps_count = self
            .client
            .query(&format!(
                "SELECT count() as count FROM swaps WHERE network = '{}'",
                network
            ))
            .fetch_one::<CountResult>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        let pools_count = self
            .client
            .query(&format!(
                "SELECT count() as count FROM pools WHERE network = '{}'",
                network
            ))
            .fetch_one::<CountResult>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        let liquidity_changes_count = self
            .client
            .query(&format!(
                "SELECT count() as count FROM liquidity_changes WHERE network = '{}'",
                network
            ))
            .fetch_one::<CountResult>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        let token_transfers_count = self
            .client
            .query(&format!(
                "SELECT count() as count FROM token_transfers WHERE network = '{}'",
                network
            ))
            .fetch_one::<CountResult>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        let nft_transfers_count = self
            .client
            .query(&format!(
                "SELECT count() as count FROM nft_transfers WHERE network = '{}'",
                network
            ))
            .fetch_one::<CountResult>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        let rexpump_swaps_count = self
            .client
            .query(&format!(
                "SELECT count() as count FROM rexpump_swaps WHERE network = '{}'",
                network
            ))
            .fetch_one::<CountResult>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        Ok(IndexerStatus {
            last_synced_block: last_block,
            swaps_count,
            pools_count,
            liquidity_changes_count,
            token_transfers_count,
            nft_transfers_count,
            rexpump_swaps_count,
        })
    }

    /// Get last synced block for a network
    pub async fn get_last_synced_block(&self, network: &str) -> Result<Option<u64>> {
        #[derive(Row, Deserialize)]
        struct LastBlock {
            last_synced_block: u64,
        }

        let result = self
            .client
            .query(&format!(
                "SELECT last_synced_block FROM indexer_state FINAL WHERE network = '{}'",
                network
            ))
            .fetch_optional::<LastBlock>()
            .await
            .context("Failed to fetch last synced block")?;

        Ok(result.map(|r| r.last_synced_block))
    }

    /// Update last synced block
    pub async fn update_last_synced_block(&self, network: &str, block: u64) -> Result<()> {
        self.client
            .query(&format!(
                "INSERT INTO indexer_state (network, last_synced_block) VALUES ('{}', {})",
                network, block
            ))
            .execute()
            .await
            .context("Failed to update last synced block")?;

        Ok(())
    }

    // ========================================================================
    // RexSwap insert methods
    // ========================================================================

    /// Insert a swap event
    pub async fn insert_swap(&self, swap: &SwapRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO swaps (
                id, transaction_hash, call_index, user_address, pool_id,
                is_buy, is_vault, in_base_qty, qty, limit_price, min_out,
                base_flow, quote_flow, price, call_source, dex,
                hook_delta_base, hook_delta_quote, hook_fee_override,
                block_number, block_time, transaction_index, network
            ) VALUES (
                '{}', '{}', {}, '{}', '{}',
                {}, {}, {}, {}, {}, {},
                {}, {}, {}, '{}', '{}',
                {}, {}, {},
                {}, '{}', {}, '{}'
            )"#,
            swap.id,
            swap.transaction_hash,
            swap.call_index,
            swap.user_address,
            swap.pool_id,
            swap.is_buy as u8,
            swap.is_vault as u8,
            swap.in_base_qty as u8,
            swap.qty,
            swap.limit_price
                .map(|p| p.to_string())
                .unwrap_or("NULL".to_string()),
            swap.min_out
                .as_ref()
                .map(|m| m.to_string())
                .unwrap_or("NULL".to_string()),
            swap.base_flow,
            swap.quote_flow,
            swap.price
                .map(|p| p.to_string())
                .unwrap_or("NULL".to_string()),
            swap.call_source,
            swap.dex,
            swap.hook_delta_base
                .map(|d| d.to_string())
                .unwrap_or("NULL".to_string()),
            swap.hook_delta_quote
                .map(|d| d.to_string())
                .unwrap_or("NULL".to_string()),
            swap.hook_fee_override
                .map(|f| f.to_string())
                .unwrap_or("NULL".to_string()),
            swap.block_number,
            swap.block_time,
            swap.transaction_index,
            swap.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert swap")?;

        Ok(())
    }

    /// Insert a pool
    pub async fn insert_pool(&self, pool: &PoolRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO pools (
                id, base, quote, pool_idx, template_id, hooks_address,
                block_create, time_create, network
            ) VALUES (
                '{}', '{}', '{}', {}, '{}', '{}',
                {}, '{}', '{}'
            )"#,
            pool.id,
            pool.base,
            pool.quote,
            pool.pool_idx,
            pool.template_id,
            pool.hooks_address,
            pool.block_create,
            pool.time_create,
            pool.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert pool")?;

        Ok(())
    }

    /// Insert a liquidity change
    pub async fn insert_liquidity_change(&self, liq: &LiquidityChangeRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO liquidity_changes (
                id, transaction_hash, call_index, pool_id, user_address, is_vault,
                position_type, change_type, bid_tick, ask_tick, is_bid,
                liq, base_flow, quote_flow, call_source, pivot_time, hook_delta,
                block_number, block_time, network
            ) VALUES (
                '{}', '{}', {}, '{}', '{}', {},
                '{}', '{}', {}, {}, {},
                {}, {}, {}, '{}', {}, {},
                {}, '{}', '{}'
            )"#,
            liq.id,
            liq.transaction_hash,
            liq.call_index,
            liq.pool_id,
            liq.user_address,
            liq.is_vault as u8,
            liq.position_type,
            liq.change_type,
            liq.bid_tick
                .map(|t| t.to_string())
                .unwrap_or("NULL".to_string()),
            liq.ask_tick
                .map(|t| t.to_string())
                .unwrap_or("NULL".to_string()),
            liq.is_bid as u8,
            liq.liq
                .as_ref()
                .map(|l| l.to_string())
                .unwrap_or("NULL".to_string()),
            liq.base_flow
                .as_ref()
                .map(|f| f.to_string())
                .unwrap_or("NULL".to_string()),
            liq.quote_flow
                .as_ref()
                .map(|f| f.to_string())
                .unwrap_or("NULL".to_string()),
            liq.call_source,
            liq.pivot_time
                .map(|t| t.to_string())
                .unwrap_or("NULL".to_string()),
            liq.hook_delta
                .map(|d| d.to_string())
                .unwrap_or("NULL".to_string()),
            liq.block_number,
            liq.block_time,
            liq.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert liquidity change")?;

        Ok(())
    }

    // ========================================================================
    // ERC-20 insert methods
    // ========================================================================

    /// Insert a token transfer
    pub async fn insert_token_transfer(&self, transfer: &TokenTransferRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO token_transfers (
                id, transaction_hash, log_index, token_address, from_address, to_address,
                amount, block_number, block_time, network, token_symbol, token_decimals
            ) VALUES (
                '{}', '{}', {}, '{}', '{}', '{}',
                '{}', {}, '{}', '{}', {}, {}
            )"#,
            transfer.id,
            transfer.transaction_hash,
            transfer.log_index,
            transfer.token_address,
            transfer.from_address,
            transfer.to_address,
            transfer.amount,
            transfer.block_number,
            transfer.block_time,
            transfer.network,
            escape_optional(transfer.token_symbol.as_ref()),
            transfer
                .token_decimals
                .map(|d| d.to_string())
                .unwrap_or("NULL".to_string())
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert token transfer")?;

        Ok(())
    }

    /// Batch insert ERC-20 token transfers
    pub async fn insert_token_transfers_batch(
        &self,
        transfers: &[TokenTransferRecord],
    ) -> Result<()> {
        if transfers.is_empty() {
            return Ok(());
        }

        let values: Vec<String> = transfers
            .iter()
            .map(|t| {
                format!(
                    "('{}', '{}', {}, '{}', '{}', '{}', '{}', {}, '{}', '{}', {}, {})",
                    escape_clickhouse_string(&t.id),
                    escape_clickhouse_string(&t.transaction_hash),
                    t.log_index,
                    escape_clickhouse_string(&t.token_address),
                    escape_clickhouse_string(&t.from_address),
                    escape_clickhouse_string(&t.to_address),
                    escape_clickhouse_string(&t.amount),
                    t.block_number,
                    escape_clickhouse_string(&t.block_time),
                    escape_clickhouse_string(&t.network),
                    escape_optional(t.token_symbol.as_ref()),
                    t.token_decimals
                        .map(|d| d.to_string())
                        .unwrap_or("NULL".to_string())
                )
            })
            .collect();

        let sql = format!(
            r#"INSERT INTO token_transfers (
                id, transaction_hash, log_index, token_address, from_address, to_address,
                amount, block_number, block_time, network, token_symbol, token_decimals
            ) VALUES {}"#,
            values.join(", ")
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert token transfers batch")?;

        Ok(())
    }

    /// Batch insert/update wallet-token interactions
    /// Uses ReplacingMergeTree so duplicates will be merged keeping latest
    pub async fn insert_wallet_tokens_batch(
        &self,
        records: &[crate::db::schema::erc20::WalletTokenRecord],
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let values: Vec<String> = records
            .iter()
            .map(|r| {
                format!(
                    "('{}', '{}', '{}', '{}')",
                    r.wallet_address,
                    r.token_address,
                    r.last_interaction,
                    r.network
                )
            })
            .collect();

        let sql = format!(
            r#"INSERT INTO wallet_tokens (wallet_address, token_address, last_interaction, network) VALUES {}"#,
            values.join(", ")
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert wallet_tokens batch")?;

        Ok(())
    }

    // ========================================================================
    // ERC-721 (NFT) insert methods
    // ========================================================================

    /// Insert single NFT transfer
    pub async fn insert_nft_transfer(&self, transfer: &NftTransferRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO nft_transfers (
                id, transaction_hash, log_index, contract_address, from_address, to_address,
                token_id, block_number, block_time, network, collection_name, token_uri
            ) VALUES (
                '{}', '{}', {}, '{}', '{}', '{}',
                '{}', {}, '{}', '{}', {}, {}
            )"#,
            transfer.id,
            transfer.transaction_hash,
            transfer.log_index,
            transfer.contract_address,
            transfer.from_address,
            transfer.to_address,
            transfer.token_id,
            transfer.block_number,
            transfer.block_time,
            transfer.network,
            escape_optional(transfer.collection_name.as_ref()),
            escape_optional(transfer.token_uri.as_ref())
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert NFT transfer")?;

        Ok(())
    }

    /// Batch insert NFT transfers
    pub async fn insert_nft_transfers_batch(&self, transfers: &[NftTransferRecord]) -> Result<()> {
        if transfers.is_empty() {
            return Ok(());
        }

        let values: Vec<String> = transfers
            .iter()
            .map(|t| {
                format!(
                    "('{}', '{}', {}, '{}', '{}', '{}', '{}', {}, '{}', '{}', {}, {})",
                    escape_clickhouse_string(&t.id),
                    escape_clickhouse_string(&t.transaction_hash),
                    t.log_index,
                    escape_clickhouse_string(&t.contract_address),
                    escape_clickhouse_string(&t.from_address),
                    escape_clickhouse_string(&t.to_address),
                    escape_clickhouse_string(&t.token_id),
                    t.block_number,
                    escape_clickhouse_string(&t.block_time),
                    escape_clickhouse_string(&t.network),
                    escape_optional(t.collection_name.as_ref()),
                    escape_optional(t.token_uri.as_ref())
                )
            })
            .collect();

        let sql = format!(
            r#"INSERT INTO nft_transfers (
                id, transaction_hash, log_index, contract_address, from_address, to_address,
                token_id, block_number, block_time, network, collection_name, token_uri
            ) VALUES {}"#,
            values.join(", ")
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert NFT transfers batch")?;

        Ok(())
    }

    // ========================================================================
    // RexPump insert methods
    // ========================================================================

    /// Insert RexPump pool
    pub async fn insert_rexpump_pool(&self, pool: &RexPumpPoolRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO rexpump_pools (
                id, pool_id, memecoin_address, memecoin_treasury, token_id,
                currency_flipped, creator_address, creator_fee_allocation,
                block_number, block_time, transaction_hash, network
            ) VALUES (
                '{}', '{}', '{}', '{}', {},
                {}, '{}', {},
                {}, '{}', '{}', '{}'
            )"#,
            pool.id,
            pool.pool_id,
            pool.memecoin_address,
            pool.memecoin_treasury,
            pool.token_id,
            pool.currency_flipped as u8,
            pool.creator_address,
            pool.creator_fee_allocation,
            pool.block_number,
            pool.block_time,
            pool.transaction_hash,
            pool.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert rexpump pool")?;

        Ok(())
    }

    /// Insert RexPump swap
    pub async fn insert_rexpump_swap(&self, swap: &RexPumpSwapRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO rexpump_swaps (
                id, transaction_hash, log_index, pool_id, sender,
                amount0, amount1, fee0, fee1, hook_lp_fee0, hook_lp_fee1,
                block_number, block_time, network
            ) VALUES (
                '{}', '{}', {}, '{}', '{}',
                '{}', '{}', '{}', '{}', {}, {},
                {}, '{}', '{}'
            )"#,
            swap.id,
            swap.transaction_hash,
            swap.log_index,
            swap.pool_id,
            swap.sender,
            swap.amount0,
            swap.amount1,
            swap.fee0,
            swap.fee1,
            swap.hook_lp_fee0
                .as_ref()
                .map(|f| format!("'{}'", f))
                .unwrap_or("NULL".to_string()),
            swap.hook_lp_fee1
                .as_ref()
                .map(|f| format!("'{}'", f))
                .unwrap_or("NULL".to_string()),
            swap.block_number,
            swap.block_time,
            swap.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert rexpump swap")?;

        Ok(())
    }

    /// Insert RexPump pool state update
    pub async fn insert_rexpump_pool_state(&self, state: &RexPumpPoolStateRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO rexpump_pool_states (
                id, transaction_hash, log_index, pool_id, sqrt_price_x96, tick,
                protocol_fee, swap_fee, liquidity, block_number, block_time, network
            ) VALUES (
                '{}', '{}', {}, '{}', '{}', {},
                {}, {}, '{}', {}, '{}', '{}'
            )"#,
            state.id,
            state.transaction_hash,
            state.log_index,
            state.pool_id,
            state.sqrt_price_x96,
            state.tick,
            state.protocol_fee,
            state.swap_fee,
            state.liquidity,
            state.block_number,
            state.block_time,
            state.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert rexpump pool state")?;

        Ok(())
    }

    /// Insert RexPump fee distribution
    pub async fn insert_rexpump_fee_distribution(
        &self,
        dist: &RexPumpFeeDistributionRecord,
    ) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO rexpump_fee_distributions (
                id, transaction_hash, log_index, pool_id, donate_amount, creator_amount,
                bidwall_amount, governance_amount, protocol_amount, block_number, block_time, network
            ) VALUES (
                '{}', '{}', {}, '{}', '{}', '{}',
                '{}', '{}', '{}', {}, '{}', '{}'
            )"#,
            dist.id,
            dist.transaction_hash,
            dist.log_index,
            dist.pool_id,
            dist.donate_amount,
            dist.creator_amount,
            dist.bidwall_amount,
            dist.governance_amount,
            dist.protocol_amount,
            dist.block_number,
            dist.block_time,
            dist.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert rexpump fee distribution")?;

        Ok(())
    }

    /// Insert BidWall event
    pub async fn insert_bidwall_event(&self, event: &BidWallEventRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO rexpump_bidwall_events (
                id, transaction_hash, log_index, pool_id, event_type,
                eth_amount, tick_lower, tick_upper, recipient, tokens, disabled,
                block_number, block_time, network
            ) VALUES (
                '{}', '{}', {}, '{}', '{}',
                {}, {}, {}, {}, {}, {},
                {}, '{}', '{}'
            )"#,
            event.id,
            event.transaction_hash,
            event.log_index,
            event.pool_id,
            event.event_type,
            event
                .eth_amount
                .as_ref()
                .map(|e| format!("'{}'", e))
                .unwrap_or("NULL".to_string()),
            event
                .tick_lower
                .map(|t| t.to_string())
                .unwrap_or("NULL".to_string()),
            event
                .tick_upper
                .map(|t| t.to_string())
                .unwrap_or("NULL".to_string()),
            event
                .recipient
                .as_ref()
                .map(|r| format!("'{}'", r))
                .unwrap_or("NULL".to_string()),
            event
                .tokens
                .as_ref()
                .map(|t| format!("'{}'", t))
                .unwrap_or("NULL".to_string()),
            event
                .disabled
                .map(|d| (d as u8).to_string())
                .unwrap_or("NULL".to_string()),
            event.block_number,
            event.block_time,
            event.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert bidwall event")?;

        Ok(())
    }

    /// Insert FairLaunch event
    pub async fn insert_fairlaunch_event(&self, event: &FairLaunchEventRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO rexpump_fairlaunch_events (
                id, transaction_hash, log_index, pool_id, event_type,
                tokens, starts_at, ends_at, revenue, supply, ended_at,
                block_number, block_time, network
            ) VALUES (
                '{}', '{}', {}, '{}', '{}',
                {}, {}, {}, {}, {}, {},
                {}, '{}', '{}'
            )"#,
            event.id,
            event.transaction_hash,
            event.log_index,
            event.pool_id,
            event.event_type,
            event
                .tokens
                .as_ref()
                .map(|t| format!("'{}'", t))
                .unwrap_or("NULL".to_string()),
            event
                .starts_at
                .map(|t| t.to_string())
                .unwrap_or("NULL".to_string()),
            event
                .ends_at
                .map(|t| t.to_string())
                .unwrap_or("NULL".to_string()),
            event
                .revenue
                .as_ref()
                .map(|r| format!("'{}'", r))
                .unwrap_or("NULL".to_string()),
            event
                .supply
                .as_ref()
                .map(|s| format!("'{}'", s))
                .unwrap_or("NULL".to_string()),
            event
                .ended_at
                .map(|t| t.to_string())
                .unwrap_or("NULL".to_string()),
            event.block_number,
            event.block_time,
            event.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert fairlaunch event")?;

        Ok(())
    }

    /// Insert Fee Escrow event
    pub async fn insert_fee_escrow_event(&self, event: &FeeEscrowEventRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO rexpump_fee_escrow_events (
                id, transaction_hash, log_index, pool_id, event_type,
                payee, sender, recipient, token_address, amount,
                block_number, block_time, network
            ) VALUES (
                '{}', '{}', {}, {}, '{}',
                {}, {}, {}, '{}', '{}',
                {}, '{}', '{}'
            )"#,
            event.id,
            event.transaction_hash,
            event.log_index,
            event
                .pool_id
                .as_ref()
                .map(|p| format!("'{}'", p))
                .unwrap_or("NULL".to_string()),
            event.event_type,
            event
                .payee
                .as_ref()
                .map(|p| format!("'{}'", p))
                .unwrap_or("NULL".to_string()),
            event
                .sender
                .as_ref()
                .map(|s| format!("'{}'", s))
                .unwrap_or("NULL".to_string()),
            event
                .recipient
                .as_ref()
                .map(|r| format!("'{}'", r))
                .unwrap_or("NULL".to_string()),
            event.token_address,
            event.amount,
            event.block_number,
            event.block_time,
            event.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert fee escrow event")?;

        Ok(())
    }

    /// Insert Referrer Fee
    pub async fn insert_referrer_fee(&self, fee: &ReferrerFeeRecord) -> Result<()> {
        let sql = format!(
            r#"INSERT INTO rexpump_referrer_fees (
                id, transaction_hash, log_index, pool_id, recipient,
                token_address, amount, block_number, block_time, network
            ) VALUES (
                '{}', '{}', {}, '{}', '{}',
                '{}', '{}', {}, '{}', '{}'
            )"#,
            fee.id,
            fee.transaction_hash,
            fee.log_index,
            fee.pool_id,
            fee.recipient,
            fee.token_address,
            fee.amount,
            fee.block_number,
            fee.block_time,
            fee.network
        );

        self.client
            .query(&sql)
            .execute()
            .await
            .context("Failed to insert referrer fee")?;

        Ok(())
    }

    /// Get raw client for advanced queries
    pub fn raw(&self) -> &Client {
        &self.client
    }
}
