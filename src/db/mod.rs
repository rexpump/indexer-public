//! Database module - ClickHouse client and schema definitions
//!
//! Schema is split into modules:
//! - core: indexer state
//! - rexswap: pools, swaps, liquidity changes
//! - erc20: ERC-20 token transfers
//! - erc721: NFT transfers
//! - rexpump: memecoin launchpad events

mod client;
pub mod queries;
pub mod schema;

pub use client::ClickHouseClient;
pub use schema::{
    // Core
    IndexerStatus,
    // RexSwap
    LiquidityChangeRecord, PoolRecord, SwapRecord,
    // ERC-20
    TokenTransferRecord,
    // ERC-721 (NFT)
    NftTransferRecord,
    // RexPump
    BidWallEventRecord, FairLaunchEventRecord, FeeEscrowEventRecord, ReferrerFeeRecord,
    RexPumpFeeDistributionRecord, RexPumpPoolRecord, RexPumpPoolStateRecord, RexPumpSwapRecord,
};
