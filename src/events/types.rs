//! Common types for parsed RexSwap operations
//!
//! Note: These are called "Events" for historical reasons, but RexSwap
//! doesn't emit events for swaps/liquidity. These structs represent
//! decoded transaction calldata, not actual blockchain events.

use alloy::primitives::{Address, B256, U256};

/// Parsed swap operation (from swap() calldata)
#[derive(Debug, Clone)]
pub struct SwapEvent {
    pub base: Address,
    pub quote: Address,
    pub pool_idx: U256,
    pub is_buy: bool,
    pub in_base_qty: bool,
    pub qty: u128,
    pub limit_price: u128,
    pub min_out: u128,
    pub reserve_flags: u8,
    pub base_flow: i128,
    pub quote_flow: i128,
    pub call_source: String,
    // Hook-related (RexSwap specific)
    pub hook_delta_base: Option<i128>,
    pub hook_delta_quote: Option<i128>,
    pub hook_fee_override: Option<u16>,
}

/// Parsed liquidity change event
#[derive(Debug, Clone)]
pub struct LiquidityChangeEvent {
    pub base: Address,
    pub quote: Address,
    pub pool_idx: U256,
    pub position_type: PositionType,
    pub change_type: ChangeType,
    pub bid_tick: i32,
    pub ask_tick: i32,
    pub is_bid: bool,
    pub liq: Option<u128>,
    pub base_flow: i128,
    pub quote_flow: i128,
    pub call_source: String,
    pub pivot_time: Option<u64>,
    // Hook-related
    pub hook_delta: Option<i128>,
}

/// Position type for liquidity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionType {
    Ambient,
    Concentrated,
    Knockout,
}

impl std::fmt::Display for PositionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PositionType::Ambient => write!(f, "ambient"),
            PositionType::Concentrated => write!(f, "concentrated"),
            PositionType::Knockout => write!(f, "knockout"),
        }
    }
}

/// Change type for liquidity operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Mint,
    Burn,
    Harvest,
    Claim,
    Recover,
    Cross,
}

impl std::fmt::Display for ChangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeType::Mint => write!(f, "mint"),
            ChangeType::Burn => write!(f, "burn"),
            ChangeType::Harvest => write!(f, "harvest"),
            ChangeType::Claim => write!(f, "claim"),
            ChangeType::Recover => write!(f, "recover"),
            ChangeType::Cross => write!(f, "cross"),
        }
    }
}

/// Parsed pool initialization event
#[derive(Debug, Clone)]
pub struct PoolInitEvent {
    pub base: Address,
    pub quote: Address,
    pub pool_idx: U256,
}

/// Parsed knockout cross event
#[derive(Debug, Clone)]
pub struct KnockoutCrossEvent {
    pub pool_hash: B256,
    pub tick: i32,
    pub is_bid: bool,
    pub pivot_time: u32,
    pub fee_mileage: u64,
}

/// Parsed pool template event
#[derive(Debug, Clone)]
pub struct PoolTemplateEvent {
    pub pool_idx: U256,
    pub fee_rate: u16,
    pub tick_size: u16,
    pub jit_thresh: u8,
    pub knockout: u8,
    pub hooks: Address,
}

/// Generic decoded event wrapper
#[derive(Debug, Clone)]
pub enum DecodedEvent {
    Swap(SwapEvent),
    LiquidityChange(LiquidityChangeEvent),
    PoolInit(PoolInitEvent),
    KnockoutCross(KnockoutCrossEvent),
    PoolTemplate(PoolTemplateEvent),
    Unknown,
}

