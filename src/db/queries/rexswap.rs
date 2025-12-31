//! RexSwap DEX queries (pools, swaps, liquidity)

use anyhow::Result;
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use crate::db::ClickHouseClient;

// ============================================================================
// Row types from database
// ============================================================================

/// Pool record from database
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct PoolRow {
    pub id: String,
    pub base: String,
    pub quote: String,
    pub pool_idx: String,
    pub template_id: String,
    pub hooks_address: String,
    pub block_create: u64,
    pub time_create: String,
    pub network: String,
}

/// Swap record from database
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct SwapRow {
    pub id: String,
    pub transaction_hash: String,
    pub call_index: u32,
    pub user_address: String,
    pub pool_id: String,
    pub is_buy: u8,
    pub in_base_qty: u8,
    pub qty: String,
    pub base_flow: String,
    pub quote_flow: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    pub call_source: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// Liquidity change record from database
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct LiquidityChangeRow {
    pub id: String,
    pub transaction_hash: String,
    pub call_index: u32,
    pub pool_id: String,
    pub user_address: String,
    pub position_type: String,
    pub change_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bid_tick: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_tick: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liq: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_flow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote_flow: Option<String>,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// Pool with statistics
#[derive(Debug, Clone, Serialize)]
pub struct PoolWithStats {
    #[serde(flatten)]
    pub pool: PoolRow,
    pub swap_count: u64,
    pub volume_24h: Option<f64>,
}

// ============================================================================
// Query struct
// ============================================================================

/// RexSwap query methods
pub struct RexSwapQueries;

impl RexSwapQueries {
    // ========================================================================
    // Pool queries
    // ========================================================================

    /// Get all pools
    pub async fn get_pools(
        db: &ClickHouseClient,
        network: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<PoolRow>, u64)> {
        // Get count
        #[derive(Row, Deserialize)]
        struct CountRow {
            count: u64,
        }

        let total = db
            .raw()
            .query(&format!(
                "SELECT count() as count FROM pools FINAL WHERE network = '{}'",
                network
            ))
            .fetch_one::<CountRow>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        // Get pools
        let sql = format!(
            r#"SELECT 
                id, base, quote, toString(pool_idx) as pool_idx, 
                template_id, hooks_address, block_create,
                toString(time_create) as time_create, network
            FROM pools FINAL
            WHERE network = '{}'
            ORDER BY block_create DESC
            LIMIT {} OFFSET {}"#,
            network, limit, offset
        );

        let pools = db.raw().query(&sql).fetch_all::<PoolRow>().await?;
        Ok((pools, total))
    }

    /// Get pool by ID
    pub async fn get_pool(
        db: &ClickHouseClient,
        network: &str,
        pool_id: &str,
    ) -> Result<Option<PoolRow>> {
        let sql = format!(
            r#"SELECT 
                id, base, quote, toString(pool_idx) as pool_idx,
                template_id, hooks_address, block_create,
                toString(time_create) as time_create, network
            FROM pools FINAL
            WHERE network = '{}' AND id = '{}'"#,
            network, pool_id
        );

        let pool = db.raw().query(&sql).fetch_optional::<PoolRow>().await?;
        Ok(pool)
    }

    /// Search pools by token address
    pub async fn search_pools(
        db: &ClickHouseClient,
        network: &str,
        token: &str,
    ) -> Result<Vec<PoolRow>> {
        let token_lower = token.to_lowercase();
        let sql = format!(
            r#"SELECT 
                id, base, quote, toString(pool_idx) as pool_idx,
                template_id, hooks_address, block_create,
                toString(time_create) as time_create, network
            FROM pools FINAL
            WHERE network = '{}' 
              AND (base = '{}' OR quote = '{}')
            ORDER BY block_create DESC
            LIMIT 100"#,
            network, token_lower, token_lower
        );

        let pools = db.raw().query(&sql).fetch_all::<PoolRow>().await?;
        Ok(pools)
    }

    // ========================================================================
    // Swap queries
    // ========================================================================

    /// Get recent swaps
    pub async fn get_swaps(
        db: &ClickHouseClient,
        network: &str,
        pool_id: Option<&str>,
        user: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<SwapRow>, u64)> {
        let mut conditions = vec![format!("network = '{}'", network)];

        if let Some(pid) = pool_id {
            conditions.push(format!("pool_id = '{}'", pid));
        }
        if let Some(u) = user {
            conditions.push(format!("user_address = '{}'", u.to_lowercase()));
        }

        let where_clause = conditions.join(" AND ");

        // Count
        #[derive(Row, Deserialize)]
        struct CountRow {
            count: u64,
        }

        let total = db
            .raw()
            .query(&format!(
                "SELECT count() as count FROM swaps WHERE {}",
                where_clause
            ))
            .fetch_one::<CountRow>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        // Get swaps
        let sql = format!(
            r#"SELECT 
                id, transaction_hash, call_index, user_address, pool_id,
                is_buy, in_base_qty, toString(qty) as qty,
                toString(base_flow) as base_flow, toString(quote_flow) as quote_flow,
                price, call_source, block_number,
                toString(block_time) as block_time, network
            FROM swaps
            WHERE {}
            ORDER BY block_number DESC, call_index DESC
            LIMIT {} OFFSET {}"#,
            where_clause, limit, offset
        );

        let swaps = db.raw().query(&sql).fetch_all::<SwapRow>().await?;
        Ok((swaps, total))
    }

    // ========================================================================
    // Liquidity queries
    // ========================================================================

    /// Get liquidity changes for a pool or user
    pub async fn get_liquidity_changes(
        db: &ClickHouseClient,
        network: &str,
        pool_id: Option<&str>,
        user: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<LiquidityChangeRow>, u64)> {
        let mut conditions = vec![format!("network = '{}'", network)];

        if let Some(pid) = pool_id {
            conditions.push(format!("pool_id = '{}'", pid));
        }
        if let Some(u) = user {
            conditions.push(format!("user_address = '{}'", u.to_lowercase()));
        }

        let where_clause = conditions.join(" AND ");

        // Count
        #[derive(Row, Deserialize)]
        struct CountRow {
            count: u64,
        }

        let total = db
            .raw()
            .query(&format!(
                "SELECT count() as count FROM liquidity_changes WHERE {}",
                where_clause
            ))
            .fetch_one::<CountRow>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        // Get changes
        let sql = format!(
            r#"SELECT 
                id, transaction_hash, call_index, pool_id, user_address,
                position_type, change_type, bid_tick, ask_tick,
                toString(liq) as liq, 
                toString(base_flow) as base_flow, 
                toString(quote_flow) as quote_flow,
                block_number, toString(block_time) as block_time, network
            FROM liquidity_changes
            WHERE {}
            ORDER BY block_number DESC, call_index DESC
            LIMIT {} OFFSET {}"#,
            where_clause, limit, offset
        );

        let changes = db
            .raw()
            .query(&sql)
            .fetch_all::<LiquidityChangeRow>()
            .await?;
        Ok((changes, total))
    }

    /// Get user's active LP positions (aggregated by pool)
    pub async fn get_user_positions(
        db: &ClickHouseClient,
        network: &str,
        user: &str,
    ) -> Result<Vec<UserPosition>> {
        let user_lower = user.to_lowercase();
        let sql = format!(
            r#"SELECT 
                pool_id,
                position_type,
                countIf(change_type = 'mint') as mint_count,
                countIf(change_type = 'burn') as burn_count,
                max(block_number) as last_activity_block
            FROM liquidity_changes
            WHERE network = '{}' AND user_address = '{}'
            GROUP BY pool_id, position_type
            ORDER BY last_activity_block DESC"#,
            network, user_lower
        );

        let positions = db.raw().query(&sql).fetch_all::<UserPosition>().await?;
        Ok(positions)
    }
}

/// User LP position summary
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct UserPosition {
    pub pool_id: String,
    pub position_type: String,
    pub mint_count: u64,
    pub burn_count: u64,
    pub last_activity_block: u64,
}
