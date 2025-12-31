//! RexPump schema - memecoin launchpad events
//!
//! Tracks events from RexPump contracts:
//! - PositionManager: PoolCreated, PoolSwap, HookSwap, PoolStateUpdated, fees
//! - BidWall: BidWallInitialized, BidWallRepositioned, etc.
//! - FairLaunch: FairLaunchCreated, FairLaunchEnded
//! - FeeEscrow: Deposit, Withdrawal

use super::TableSchema;

pub struct RexPumpSchema;

impl TableSchema for RexPumpSchema {
    fn module_name() -> &'static str {
        "rexpump"
    }

    fn create_tables_sql() -> &'static str {
        r#"
-- RexPump Pools (memecoins created via RexPump)
CREATE TABLE IF NOT EXISTS rexpump_pools (
    id String,
    pool_id String,
    memecoin_address String,
    memecoin_treasury String,
    token_id UInt256,
    currency_flipped UInt8,
    creator_address String,
    creator_fee_allocation UInt32,
    block_number UInt64,
    block_time DateTime,
    transaction_hash String,
    network String
) ENGINE = ReplacingMergeTree(block_time)
ORDER BY (network, pool_id);

-- RexPump Swaps (from PoolSwap and HookSwap events)
CREATE TABLE IF NOT EXISTS rexpump_swaps (
    id String,
    transaction_hash String,
    log_index UInt32,
    pool_id String,
    sender String,
    -- Amounts (can be positive or negative)
    amount0 String,
    amount1 String,
    fee0 String,
    fee1 String,
    -- Hook LP fees
    hook_lp_fee0 Nullable(String),
    hook_lp_fee1 Nullable(String),
    block_number UInt64,
    block_time DateTime,
    network String
) ENGINE = MergeTree()
ORDER BY (network, pool_id, block_number, log_index)
PARTITION BY toYYYYMM(block_time);

-- Pool State Updates (price, tick, liquidity after each swap)
CREATE TABLE IF NOT EXISTS rexpump_pool_states (
    id String,
    transaction_hash String,
    log_index UInt32,
    pool_id String,
    sqrt_price_x96 String,
    tick Int32,
    protocol_fee UInt32,
    swap_fee UInt32,
    liquidity String,
    block_number UInt64,
    block_time DateTime,
    network String
) ENGINE = MergeTree()
ORDER BY (network, pool_id, block_number, log_index)
PARTITION BY toYYYYMM(block_time);

-- Fee Distribution Events
CREATE TABLE IF NOT EXISTS rexpump_fee_distributions (
    id String,
    transaction_hash String,
    log_index UInt32,
    pool_id String,
    donate_amount String,
    creator_amount String,
    bidwall_amount String,
    governance_amount String,
    protocol_amount String,
    block_number UInt64,
    block_time DateTime,
    network String
) ENGINE = MergeTree()
ORDER BY (network, pool_id, block_number, log_index)
PARTITION BY toYYYYMM(block_time);

-- BidWall Events
CREATE TABLE IF NOT EXISTS rexpump_bidwall_events (
    id String,
    transaction_hash String,
    log_index UInt32,
    pool_id String,
    event_type String,
    eth_amount Nullable(String),
    tick_lower Nullable(Int32),
    tick_upper Nullable(Int32),
    recipient Nullable(String),
    tokens Nullable(String),
    disabled Nullable(UInt8),
    block_number UInt64,
    block_time DateTime,
    network String
) ENGINE = MergeTree()
ORDER BY (network, pool_id, block_number, log_index)
PARTITION BY toYYYYMM(block_time);

-- Fair Launch Events
CREATE TABLE IF NOT EXISTS rexpump_fairlaunch_events (
    id String,
    transaction_hash String,
    log_index UInt32,
    pool_id String,
    event_type String,
    tokens Nullable(String),
    starts_at Nullable(UInt64),
    ends_at Nullable(UInt64),
    revenue Nullable(String),
    supply Nullable(String),
    ended_at Nullable(UInt64),
    block_number UInt64,
    block_time DateTime,
    network String
) ENGINE = MergeTree()
ORDER BY (network, pool_id, block_number, log_index)
PARTITION BY toYYYYMM(block_time);

-- Fee Escrow Events (Deposit/Withdrawal)
CREATE TABLE IF NOT EXISTS rexpump_fee_escrow_events (
    id String,
    transaction_hash String,
    log_index UInt32,
    pool_id Nullable(String),
    event_type String,
    payee Nullable(String),
    sender Nullable(String),
    recipient Nullable(String),
    token_address String,
    amount String,
    block_number UInt64,
    block_time DateTime,
    network String
) ENGINE = MergeTree()
ORDER BY (network, block_number, log_index)
PARTITION BY toYYYYMM(block_time);

-- Referrer Fee Payments
CREATE TABLE IF NOT EXISTS rexpump_referrer_fees (
    id String,
    transaction_hash String,
    log_index UInt32,
    pool_id String,
    recipient String,
    token_address String,
    amount String,
    block_number UInt64,
    block_time DateTime,
    network String
) ENGINE = MergeTree()
ORDER BY (network, pool_id, block_number, log_index)
PARTITION BY toYYYYMM(block_time)
"#
    }

    fn drop_tables_sql() -> &'static str {
        r#"
DROP TABLE IF EXISTS rexpump_pools;
DROP TABLE IF EXISTS rexpump_swaps;
DROP TABLE IF EXISTS rexpump_pool_states;
DROP TABLE IF EXISTS rexpump_fee_distributions;
DROP TABLE IF EXISTS rexpump_bidwall_events;
DROP TABLE IF EXISTS rexpump_fairlaunch_events;
DROP TABLE IF EXISTS rexpump_fee_escrow_events;
DROP TABLE IF EXISTS rexpump_referrer_fees
"#
    }

    fn table_names() -> &'static [&'static str] {
        &[
            "rexpump_pools",
            "rexpump_swaps",
            "rexpump_pool_states",
            "rexpump_fee_distributions",
            "rexpump_bidwall_events",
            "rexpump_fairlaunch_events",
            "rexpump_fee_escrow_events",
            "rexpump_referrer_fees",
        ]
    }
}

// ============================================================================
// Record types for database insertion
// ============================================================================

/// RexPump Pool Created record
#[derive(Debug, Clone)]
pub struct RexPumpPoolRecord {
    pub id: String,
    pub pool_id: String,
    pub memecoin_address: String,
    pub memecoin_treasury: String,
    pub token_id: String,
    pub currency_flipped: bool,
    pub creator_address: String,
    pub creator_fee_allocation: u32,
    pub block_number: u64,
    pub block_time: String,
    pub transaction_hash: String,
    pub network: String,
}

/// RexPump Swap record (from PoolSwap or HookSwap)
#[derive(Debug, Clone)]
pub struct RexPumpSwapRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub pool_id: String,
    pub sender: String,
    pub amount0: String,
    pub amount1: String,
    pub fee0: String,
    pub fee1: String,
    pub hook_lp_fee0: Option<String>,
    pub hook_lp_fee1: Option<String>,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// RexPump Pool State record
#[derive(Debug, Clone)]
pub struct RexPumpPoolStateRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub pool_id: String,
    pub sqrt_price_x96: String,
    pub tick: i32,
    pub protocol_fee: u32,
    pub swap_fee: u32,
    pub liquidity: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// RexPump Fee Distribution record
#[derive(Debug, Clone)]
pub struct RexPumpFeeDistributionRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub pool_id: String,
    pub donate_amount: String,
    pub creator_amount: String,
    pub bidwall_amount: String,
    pub governance_amount: String,
    pub protocol_amount: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// BidWall Event record
#[derive(Debug, Clone)]
pub struct BidWallEventRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub pool_id: String,
    pub event_type: String, // "initialized", "repositioned", "closed", "deposit", "rewards_transferred", "disabled_updated"
    pub eth_amount: Option<String>,
    pub tick_lower: Option<i32>,
    pub tick_upper: Option<i32>,
    pub recipient: Option<String>,
    pub tokens: Option<String>,
    pub disabled: Option<bool>,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// FairLaunch Event record
#[derive(Debug, Clone)]
pub struct FairLaunchEventRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub pool_id: String,
    pub event_type: String, // "created", "ended"
    pub tokens: Option<String>,
    pub starts_at: Option<u64>,
    pub ends_at: Option<u64>,
    pub revenue: Option<String>,
    pub supply: Option<String>,
    pub ended_at: Option<u64>,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// Fee Escrow Event record
#[derive(Debug, Clone)]
pub struct FeeEscrowEventRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub pool_id: Option<String>,
    pub event_type: String, // "deposit", "withdrawal"
    pub payee: Option<String>,
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub token_address: String,
    pub amount: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}

/// Referrer Fee record
#[derive(Debug, Clone)]
pub struct ReferrerFeeRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub pool_id: String,
    pub recipient: String,
    pub token_address: String,
    pub amount: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
}
