//! RexSwap schema - pools, swaps, liquidity changes
//!
//! Tables for tracking RexSwap DEX activity.
//! Note: RexSwap doesn't emit events, so we parse transaction calldata.

use super::TableSchema;

pub struct RexSwapSchema;

impl TableSchema for RexSwapSchema {
    fn module_name() -> &'static str {
        "rexswap"
    }

    fn create_tables_sql() -> &'static str {
        r#"
-- Pool templates (pool type configurations)
CREATE TABLE IF NOT EXISTS pool_templates (
    id String,
    pool_idx UInt256,
    fee_rate UInt16,
    tick_size UInt16,
    jit_thresh UInt8,
    knockout UInt8,
    hooks_address String,
    enabled UInt8,
    block_init UInt64,
    time_init DateTime,
    block_revise UInt64,
    time_revise DateTime,
    network String
) ENGINE = ReplacingMergeTree(time_revise)
ORDER BY (network, id);

-- Liquidity pools
CREATE TABLE IF NOT EXISTS pools (
    id String,
    base String,
    quote String,
    pool_idx UInt256,
    template_id String,
    hooks_address String,
    block_create UInt64,
    time_create DateTime,
    network String
) ENGINE = ReplacingMergeTree(time_create)
ORDER BY (network, id);

-- Swap events (from calldata parsing)
CREATE TABLE IF NOT EXISTS swaps (
    id String,
    transaction_hash String,
    call_index UInt32,
    user_address String,
    pool_id String,
    is_buy UInt8,
    is_vault UInt8,
    in_base_qty UInt8,
    qty UInt256,
    limit_price Nullable(Float64),
    min_out Nullable(UInt256),
    base_flow Int256,
    quote_flow Int256,
    price Nullable(Float64),
    call_source String,
    dex String,
    hook_delta_base Nullable(Int128),
    hook_delta_quote Nullable(Int128),
    hook_fee_override Nullable(UInt16),
    block_number UInt64,
    block_time DateTime,
    transaction_index UInt32,
    network String
) ENGINE = MergeTree()
ORDER BY (network, block_number, transaction_index, call_index)
PARTITION BY toYYYYMM(block_time);

-- Liquidity changes (mint, burn, harvest, etc.)
CREATE TABLE IF NOT EXISTS liquidity_changes (
    id String,
    transaction_hash String,
    call_index UInt32,
    pool_id String,
    user_address String,
    is_vault UInt8,
    position_type String,
    change_type String,
    bid_tick Nullable(Int32),
    ask_tick Nullable(Int32),
    is_bid UInt8,
    liq Nullable(UInt256),
    base_flow Nullable(Int256),
    quote_flow Nullable(Int256),
    call_source String,
    pivot_time Nullable(UInt64),
    hook_delta Nullable(Int128),
    block_number UInt64,
    block_time DateTime,
    network String
) ENGINE = MergeTree()
ORDER BY (network, block_number, call_index)
PARTITION BY toYYYYMM(block_time);

-- Knockout cross events
CREATE TABLE IF NOT EXISTS knockout_crosses (
    id String,
    transaction_hash String,
    pool_id String,
    tick Int32,
    is_bid UInt8,
    pivot_time UInt64,
    fee_mileage UInt64,
    block_number UInt64,
    block_time DateTime,
    network String
) ENGINE = MergeTree()
ORDER BY (network, block_number, transaction_hash)
PARTITION BY toYYYYMM(block_time)
"#
    }

    fn drop_tables_sql() -> &'static str {
        r#"
DROP TABLE IF EXISTS pool_templates;
DROP TABLE IF EXISTS pools;
DROP TABLE IF EXISTS swaps;
DROP TABLE IF EXISTS liquidity_changes;
DROP TABLE IF EXISTS knockout_crosses
"#
    }

    fn table_names() -> &'static [&'static str] {
        &[
            "pool_templates",
            "pools",
            "swaps",
            "liquidity_changes",
            "knockout_crosses",
        ]
    }
}

// ============================================================================
// Record types for database insertion
// ============================================================================

/// Swap record for database insertion
#[derive(Debug, Clone)]
pub struct SwapRecord {
    pub id: String,
    pub transaction_hash: String,
    pub call_index: u32,
    pub user_address: String,
    pub pool_id: String,
    pub is_buy: bool,
    pub is_vault: bool,
    pub in_base_qty: bool,
    pub qty: String,
    pub limit_price: Option<f64>,
    pub min_out: Option<String>,
    pub base_flow: String,
    pub quote_flow: String,
    pub price: Option<f64>,
    pub call_source: String,
    pub dex: String,
    pub hook_delta_base: Option<i128>,
    pub hook_delta_quote: Option<i128>,
    pub hook_fee_override: Option<u16>,
    pub block_number: u64,
    pub block_time: String,
    pub transaction_index: u32,
    pub network: String,
}

/// Pool record for database insertion
#[derive(Debug, Clone)]
pub struct PoolRecord {
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

/// Liquidity change record
#[derive(Debug, Clone)]
pub struct LiquidityChangeRecord {
    pub id: String,
    pub transaction_hash: String,
    pub call_index: u32,
    pub pool_id: String,
    pub user_address: String,
    pub is_vault: bool,
    pub position_type: String,
    pub change_type: String,
    pub bid_tick: Option<i32>,
    pub ask_tick: Option<i32>,
    pub is_bid: bool,
    pub liq: Option<String>,
    pub base_flow: Option<String>,
    pub quote_flow: Option<String>,
    pub call_source: String,
    pub pivot_time: Option<u64>,
    pub hook_delta: Option<i128>,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}
