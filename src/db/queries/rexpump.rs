//! RexPump memecoin launchpad queries

use anyhow::Result;
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use crate::db::ClickHouseClient;

// ============================================================================
// Row types from database
// ============================================================================

/// RexPump pool (memecoin) from database
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct RexPumpPoolRow {
    pub id: String,
    pub pool_id: String,
    pub memecoin_address: String,
    pub memecoin_treasury: String,
    pub token_id: String,
    pub currency_flipped: u8,
    pub creator_address: String,
    pub creator_fee_allocation: u32,
    pub block_number: u64,
    pub block_time: String,
    pub transaction_hash: String,
    pub network: String,
}

/// RexPump swap from database
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct RexPumpSwapRow {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub pool_id: String,
    pub sender: String,
    pub amount0: String,
    pub amount1: String,
    pub fee0: String,
    pub fee1: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// RexPump pool state (for price charts)
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct RexPumpStateRow {
    pub id: String,
    pub pool_id: String,
    pub sqrt_price_x96: String,
    pub tick: i32,
    pub liquidity: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// Token with statistics (for listing/trending)
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TokenWithStats {
    pub pool_id: String,
    pub memecoin_address: String,
    pub creator_address: String,
    pub created_at: String,
    pub swap_count: u64,
    pub unique_traders: u64,
}

/// OHLCV candle for charts
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct OhlcvCandle {
    pub timestamp: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: String,
    pub trades: u64,
}

// ============================================================================
// Query struct
// ============================================================================

/// RexPump query methods
pub struct RexPumpQueries;

impl RexPumpQueries {
    // ========================================================================
    // Token (pool) queries
    // ========================================================================

    /// Get all tokens with pagination
    pub async fn get_tokens(
        db: &ClickHouseClient,
        network: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<RexPumpPoolRow>, u64)> {
        #[derive(Row, Deserialize)]
        struct CountRow {
            count: u64,
        }

        let total = db
            .raw()
            .query(&format!(
                "SELECT count() as count FROM rexpump_pools FINAL WHERE network = '{}'",
                network
            ))
            .fetch_one::<CountRow>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        let sql = format!(
            r#"SELECT 
                id, pool_id, memecoin_address, memecoin_treasury,
                toString(token_id) as token_id, currency_flipped,
                creator_address, creator_fee_allocation,
                block_number, toString(block_time) as block_time,
                transaction_hash, network
            FROM rexpump_pools FINAL
            WHERE network = '{}'
            ORDER BY block_number DESC
            LIMIT {} OFFSET {}"#,
            network, limit, offset
        );

        let tokens = db.raw().query(&sql).fetch_all::<RexPumpPoolRow>().await?;
        Ok((tokens, total))
    }

    /// Get trending tokens (by swap count in last 24h)
    pub async fn get_trending_tokens(
        db: &ClickHouseClient,
        network: &str,
        limit: u32,
    ) -> Result<Vec<TokenWithStats>> {
        let sql = format!(
            r#"SELECT 
                p.pool_id as pool_id,
                p.memecoin_address as memecoin_address,
                p.creator_address as creator_address,
                toString(p.block_time) as created_at,
                count(s.id) as swap_count,
                uniqExact(s.sender) as unique_traders
            FROM rexpump_pools p FINAL
            LEFT JOIN rexpump_swaps s ON p.pool_id = s.pool_id 
                AND s.block_time > now() - INTERVAL 24 HOUR
            WHERE p.network = '{}'
            GROUP BY p.pool_id, p.memecoin_address, p.creator_address, p.block_time
            ORDER BY swap_count DESC
            LIMIT {}"#,
            network, limit
        );

        let tokens = db.raw().query(&sql).fetch_all::<TokenWithStats>().await?;
        Ok(tokens)
    }

    /// Get token by pool_id
    pub async fn get_token(
        db: &ClickHouseClient,
        network: &str,
        pool_id: &str,
    ) -> Result<Option<RexPumpPoolRow>> {
        let sql = format!(
            r#"SELECT 
                id, pool_id, memecoin_address, memecoin_treasury,
                toString(token_id) as token_id, currency_flipped,
                creator_address, creator_fee_allocation,
                block_number, toString(block_time) as block_time,
                transaction_hash, network
            FROM rexpump_pools FINAL
            WHERE network = '{}' AND pool_id = '{}'"#,
            network, pool_id
        );

        let token = db
            .raw()
            .query(&sql)
            .fetch_optional::<RexPumpPoolRow>()
            .await?;
        Ok(token)
    }

    /// Get tokens created by a user
    pub async fn get_user_created_tokens(
        db: &ClickHouseClient,
        network: &str,
        user: &str,
    ) -> Result<Vec<RexPumpPoolRow>> {
        let user_lower = user.to_lowercase();
        let sql = format!(
            r#"SELECT 
                id, pool_id, memecoin_address, memecoin_treasury,
                toString(token_id) as token_id, currency_flipped,
                creator_address, creator_fee_allocation,
                block_number, toString(block_time) as block_time,
                transaction_hash, network
            FROM rexpump_pools FINAL
            WHERE network = '{}' AND creator_address = '{}'
            ORDER BY block_number DESC"#,
            network, user_lower
        );

        let tokens = db.raw().query(&sql).fetch_all::<RexPumpPoolRow>().await?;
        Ok(tokens)
    }

    // ========================================================================
    // Swap queries
    // ========================================================================

    /// Get swaps for a token
    pub async fn get_token_swaps(
        db: &ClickHouseClient,
        network: &str,
        pool_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<RexPumpSwapRow>, u64)> {
        #[derive(Row, Deserialize)]
        struct CountRow {
            count: u64,
        }

        let total = db
            .raw()
            .query(&format!(
                "SELECT count() as count FROM rexpump_swaps WHERE network = '{}' AND pool_id = '{}'",
                network, pool_id
            ))
            .fetch_one::<CountRow>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        let sql = format!(
            r#"SELECT 
                id, transaction_hash, log_index, pool_id, sender,
                amount0, amount1, fee0, fee1,
                block_number, toString(block_time) as block_time, network
            FROM rexpump_swaps
            WHERE network = '{}' AND pool_id = '{}'
            ORDER BY block_number DESC, log_index DESC
            LIMIT {} OFFSET {}"#,
            network, pool_id, limit, offset
        );

        let swaps = db.raw().query(&sql).fetch_all::<RexPumpSwapRow>().await?;
        Ok((swaps, total))
    }

    /// Get user's swaps across all tokens
    pub async fn get_user_swaps(
        db: &ClickHouseClient,
        network: &str,
        user: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<RexPumpSwapRow>, u64)> {
        let user_lower = user.to_lowercase();

        #[derive(Row, Deserialize)]
        struct CountRow {
            count: u64,
        }

        let total = db
            .raw()
            .query(&format!(
                "SELECT count() as count FROM rexpump_swaps WHERE network = '{}' AND sender = '{}'",
                network, user_lower
            ))
            .fetch_one::<CountRow>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        let sql = format!(
            r#"SELECT 
                id, transaction_hash, log_index, pool_id, sender,
                amount0, amount1, fee0, fee1,
                block_number, toString(block_time) as block_time, network
            FROM rexpump_swaps
            WHERE network = '{}' AND sender = '{}'
            ORDER BY block_number DESC, log_index DESC
            LIMIT {} OFFSET {}"#,
            network, user_lower, limit, offset
        );

        let swaps = db.raw().query(&sql).fetch_all::<RexPumpSwapRow>().await?;
        Ok((swaps, total))
    }

    // ========================================================================
    // Chart/Price data
    // ========================================================================

    /// Get pool state history (for price chart)
    pub async fn get_price_history(
        db: &ClickHouseClient,
        network: &str,
        pool_id: &str,
        limit: u32,
    ) -> Result<Vec<RexPumpStateRow>> {
        let sql = format!(
            r#"SELECT 
                id, pool_id, sqrt_price_x96, tick, liquidity,
                block_number, toString(block_time) as block_time, network
            FROM rexpump_pool_states
            WHERE network = '{}' AND pool_id = '{}'
            ORDER BY block_number DESC
            LIMIT {}"#,
            network, pool_id, limit
        );

        let states = db.raw().query(&sql).fetch_all::<RexPumpStateRow>().await?;
        Ok(states)
    }

    /// Get OHLCV candles for charting (1 hour intervals)
    pub async fn get_ohlcv(
        db: &ClickHouseClient,
        network: &str,
        pool_id: &str,
        interval_minutes: u32,
        limit: u32,
    ) -> Result<Vec<OhlcvCandle>> {
        // Calculate price from sqrt_price_x96: price = (sqrt_price_x96 / 2^96)^2
        let sql = format!(
            r#"SELECT 
                toStartOfInterval(block_time, INTERVAL {} MINUTE) as timestamp,
                argMin(pow(toFloat64(sqrt_price_x96) / pow(2, 96), 2), block_number) as open,
                max(pow(toFloat64(sqrt_price_x96) / pow(2, 96), 2)) as high,
                min(pow(toFloat64(sqrt_price_x96) / pow(2, 96), 2)) as low,
                argMax(pow(toFloat64(sqrt_price_x96) / pow(2, 96), 2), block_number) as close,
                toString(sum(toUInt256OrZero(liquidity))) as volume,
                count() as trades
            FROM rexpump_pool_states
            WHERE network = '{}' AND pool_id = '{}'
            GROUP BY timestamp
            ORDER BY timestamp DESC
            LIMIT {}"#,
            interval_minutes, network, pool_id, limit
        );

        let candles = db.raw().query(&sql).fetch_all::<OhlcvCandle>().await?;
        Ok(candles)
    }

    // ========================================================================
    // Statistics
    // ========================================================================

    /// Get token statistics
    pub async fn get_token_stats(
        db: &ClickHouseClient,
        network: &str,
        pool_id: &str,
    ) -> Result<Option<TokenStats>> {
        let sql = format!(
            r#"SELECT 
                count() as total_swaps,
                uniqExact(sender) as unique_traders,
                min(block_time) as first_swap,
                max(block_time) as last_swap
            FROM rexpump_swaps
            WHERE network = '{}' AND pool_id = '{}'"#,
            network, pool_id
        );

        let stats = db
            .raw()
            .query(&sql)
            .fetch_optional::<TokenStats>()
            .await?;
        Ok(stats)
    }
}

/// Token statistics
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TokenStats {
    pub total_swaps: u64,
    pub unique_traders: u64,
    pub first_swap: String,
    pub last_swap: String,
}
