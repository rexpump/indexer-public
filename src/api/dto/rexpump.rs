//! DTOs for RexPump memecoin launchpad endpoints

use serde::Serialize;

use crate::db::queries::rexpump::{
    OhlcvCandle, RexPumpPoolRow, RexPumpStateRow, RexPumpSwapRow, TokenStats, TokenWithStats,
};

/// RexPump token (memecoin) response
#[derive(Debug, Serialize)]
pub struct RexPumpTokenDto {
    pub pool_id: String,
    pub memecoin_address: String,
    pub memecoin_treasury: String,
    pub token_id: String,
    pub currency_flipped: bool,
    pub creator_address: String,
    /// Creator fee allocation in basis points
    pub creator_fee_allocation: u32,
    pub block_number: u64,
    pub created_at: String,
    pub transaction_hash: String,
}

impl From<RexPumpPoolRow> for RexPumpTokenDto {
    fn from(row: RexPumpPoolRow) -> Self {
        Self {
            pool_id: row.pool_id,
            memecoin_address: row.memecoin_address,
            memecoin_treasury: row.memecoin_treasury,
            token_id: row.token_id,
            currency_flipped: row.currency_flipped == 1,
            creator_address: row.creator_address,
            creator_fee_allocation: row.creator_fee_allocation,
            block_number: row.block_number,
            created_at: row.block_time,
            transaction_hash: row.transaction_hash,
        }
    }
}

/// Token with statistics for trending/listing
#[derive(Debug, Serialize)]
pub struct TrendingTokenDto {
    pub pool_id: String,
    pub memecoin_address: String,
    pub creator_address: String,
    pub created_at: String,
    /// Number of swaps in last 24h
    pub swap_count_24h: u64,
    /// Unique traders in last 24h
    pub unique_traders_24h: u64,
}

impl From<TokenWithStats> for TrendingTokenDto {
    fn from(row: TokenWithStats) -> Self {
        Self {
            pool_id: row.pool_id,
            memecoin_address: row.memecoin_address,
            creator_address: row.creator_address,
            created_at: row.created_at,
            swap_count_24h: row.swap_count,
            unique_traders_24h: row.unique_traders,
        }
    }
}

/// RexPump swap response
#[derive(Debug, Serialize)]
pub struct RexPumpSwapDto {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub pool_id: String,
    pub sender: String,
    /// Token0 amount change (can be negative)
    pub amount0: String,
    /// Token1 amount change (can be negative)
    pub amount1: String,
    pub fee0: String,
    pub fee1: String,
    pub block_number: u64,
    pub block_time: String,
}

impl From<RexPumpSwapRow> for RexPumpSwapDto {
    fn from(row: RexPumpSwapRow) -> Self {
        Self {
            id: row.id,
            transaction_hash: row.transaction_hash,
            log_index: row.log_index,
            pool_id: row.pool_id,
            sender: row.sender,
            amount0: row.amount0,
            amount1: row.amount1,
            fee0: row.fee0,
            fee1: row.fee1,
            block_number: row.block_number,
            block_time: row.block_time,
        }
    }
}

/// Pool state for price chart
#[derive(Debug, Serialize)]
pub struct PricePointDto {
    pub sqrt_price_x96: String,
    pub tick: i32,
    pub liquidity: String,
    pub block_number: u64,
    pub timestamp: String,
}

impl From<RexPumpStateRow> for PricePointDto {
    fn from(row: RexPumpStateRow) -> Self {
        Self {
            sqrt_price_x96: row.sqrt_price_x96,
            tick: row.tick,
            liquidity: row.liquidity,
            block_number: row.block_number,
            timestamp: row.block_time,
        }
    }
}

/// OHLCV candle for charts
#[derive(Debug, Serialize)]
pub struct CandleDto {
    pub timestamp: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: String,
    pub trades: u64,
}

impl From<OhlcvCandle> for CandleDto {
    fn from(row: OhlcvCandle) -> Self {
        Self {
            timestamp: row.timestamp,
            open: row.open,
            high: row.high,
            low: row.low,
            close: row.close,
            volume: row.volume,
            trades: row.trades,
        }
    }
}

/// Token statistics response
#[derive(Debug, Serialize)]
pub struct TokenStatsDto {
    pub total_swaps: u64,
    pub unique_traders: u64,
    pub first_swap: String,
    pub last_swap: String,
}

impl From<TokenStats> for TokenStatsDto {
    fn from(row: TokenStats) -> Self {
        Self {
            total_swaps: row.total_swaps,
            unique_traders: row.unique_traders,
            first_swap: row.first_swap,
            last_swap: row.last_swap,
        }
    }
}

/// Token detail response (token + stats)
#[derive(Debug, Serialize)]
pub struct TokenDetailDto {
    #[serde(flatten)]
    pub token: RexPumpTokenDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<TokenStatsDto>,
}
