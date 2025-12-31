//! Calldata decoder for RexSwap transactions
//!
//! Decodes transaction input data instead of events, since RexSwap
//! doesn't emit swap/liquidity events.

use alloy::primitives::{Address, Bytes, U256};
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use tracing::{debug, warn};

use super::types::*;

// Define the RexSwap contract calls we need to decode
sol! {
    /// Direct swap function on RexSwapDex
    #[derive(Debug)]
    function swap(
        address base,
        address quote,
        uint256 poolIdx,
        bool isBuy,
        bool inBaseQty,
        uint128 qty,
        uint16 tip,
        uint128 limitPrice,
        uint128 minOut,
        uint8 reserveFlags,
        bytes hookData
    ) external payable returns (int128 baseFlow, int128 quoteFlow);

    /// Alternative swap without hookData (for older versions)
    #[derive(Debug)]
    function swapNoHook(
        address base,
        address quote,
        uint256 poolIdx,
        bool isBuy,
        bool inBaseQty,
        uint128 qty,
        uint16 tip,
        uint128 limitPrice,
        uint128 minOut,
        uint8 reserveFlags
    ) external payable returns (int128 baseFlow, int128 quoteFlow);

    /// Generic user command through proxy paths
    #[derive(Debug)]
    function userCmd(uint16 callpath, bytes cmd) external payable returns (bytes);
}

/// Function selectors (first 4 bytes of keccak256 of signature)
pub mod selectors {
    /// swap(address,address,uint256,bool,bool,uint128,uint16,uint128,uint128,uint8,bytes)
    pub const SWAP: [u8; 4] = [0x3d, 0x71, 0x9c, 0xd9];
    /// swap(address,address,uint256,bool,bool,uint128,uint16,uint128,uint128,uint8) - no hookData
    pub const SWAP_NO_HOOK: [u8; 4] = [0xa1, 0x5a, 0x38, 0x7d];
    /// userCmd(uint16,bytes)
    pub const USER_CMD: [u8; 4] = [0xa1, 0x5a, 0x38, 0x7d]; // Will compute actual value
}

/// Proxy path indices
pub mod proxy_paths {
    pub const HOT_PROXY: u16 = 1;
    pub const LP_PROXY: u16 = 2;      // WarmPath
    pub const COLD_PROXY: u16 = 3;
    pub const LONG_PROXY: u16 = 4;
    pub const MICRO_PROXY: u16 = 5;
    pub const KNOCKOUT_LP_PROXY: u16 = 7;
}

/// User command codes from ProtocolCmd.sol / UserCmd library
pub mod cmd_codes {
    // Cold path commands
    pub const INIT_POOL: u8 = 71;
    pub const APPROVE_ROUTER: u8 = 72;
    pub const DEPOSIT_SURPLUS: u8 = 73;
    pub const DISBURSE_SURPLUS: u8 = 74;
    pub const TRANSFER_SURPLUS: u8 = 75;
    
    // Warm path LP commands
    pub const MINT_RANGE_LIQ: u8 = 1;
    pub const MINT_RANGE_BASE: u8 = 11;
    pub const MINT_RANGE_QUOTE: u8 = 12;
    pub const BURN_RANGE_LIQ: u8 = 2;
    pub const BURN_RANGE_BASE: u8 = 21;
    pub const BURN_RANGE_QUOTE: u8 = 22;
    pub const MINT_AMBIENT_LIQ: u8 = 3;
    pub const MINT_AMBIENT_BASE: u8 = 31;
    pub const MINT_AMBIENT_QUOTE: u8 = 32;
    pub const BURN_AMBIENT_LIQ: u8 = 4;
    pub const BURN_AMBIENT_BASE: u8 = 41;
    pub const BURN_AMBIENT_QUOTE: u8 = 42;
    pub const HARVEST: u8 = 5;
    
    // Knockout commands
    pub const MINT_KNOCKOUT: u8 = 91;
    pub const BURN_KNOCKOUT: u8 = 92;
    pub const CLAIM_KNOCKOUT: u8 = 93;
    pub const RECOVER_KNOCKOUT: u8 = 94;
}

/// Decoded transaction result
#[derive(Debug, Clone)]
pub enum DecodedTransaction {
    Swap(SwapCall),
    LiquidityChange(LiquidityCall),
    PoolInit(PoolInitCall),
    KnockoutOp(KnockoutCall),
    Unknown,
}

/// Parsed swap call data
#[derive(Debug, Clone)]
pub struct SwapCall {
    pub base: Address,
    pub quote: Address,
    pub pool_idx: U256,
    pub is_buy: bool,
    pub in_base_qty: bool,
    pub qty: u128,
    pub tip: u16,
    pub limit_price: u128,
    pub min_out: u128,
    pub reserve_flags: u8,
}

/// Parsed liquidity operation call data
#[derive(Debug, Clone)]
pub struct LiquidityCall {
    pub base: Address,
    pub quote: Address,
    pub pool_idx: U256,
    pub position_type: PositionType,
    pub change_type: ChangeType,
    pub bid_tick: i32,
    pub ask_tick: i32,
    pub liq: u128,
    pub limit_lower: u128,
    pub limit_higher: u128,
}

/// Parsed pool init call data
#[derive(Debug, Clone)]
pub struct PoolInitCall {
    pub base: Address,
    pub quote: Address,
    pub pool_idx: U256,
    pub price: u128,
}

/// Parsed knockout operation call data
#[derive(Debug, Clone)]
pub struct KnockoutCall {
    pub base: Address,
    pub quote: Address,
    pub pool_idx: U256,
    pub bid_tick: i32,
    pub ask_tick: i32,
    pub is_bid: bool,
    pub operation: ChangeType,
    pub liq: Option<u128>,
    pub pivot_time: Option<u32>,
}

/// Calldata decoder for RexSwap transactions
pub struct CalldataDecoder;

impl CalldataDecoder {
    /// Decode transaction input data
    pub fn decode(input: &[u8]) -> Result<DecodedTransaction> {
        if input.len() < 4 {
            return Ok(DecodedTransaction::Unknown);
        }

        let selector = &input[0..4];
        let _data = &input[4..];

        // Try to decode swap
        if let Ok(call) = Self::decode_swap(input) {
            return Ok(DecodedTransaction::Swap(call));
        }

        // Try to decode userCmd
        if let Ok(decoded) = Self::decode_user_cmd(input) {
            return Ok(decoded);
        }

        debug!("Unknown function selector: {:02x?}", selector);
        Ok(DecodedTransaction::Unknown)
    }

    /// Decode direct swap() call
    fn decode_swap(input: &[u8]) -> Result<SwapCall> {
        // Try with hookData first
        if let Ok(call) = swapCall::abi_decode(input) {
            return Ok(SwapCall {
                base: call.base,
                quote: call.quote,
                pool_idx: call.poolIdx,
                is_buy: call.isBuy,
                in_base_qty: call.inBaseQty,
                qty: call.qty,
                tip: call.tip,
                limit_price: call.limitPrice,
                min_out: call.minOut,
                reserve_flags: call.reserveFlags,
            });
        }

        // Try without hookData
        if let Ok(call) = swapNoHookCall::abi_decode(input) {
            return Ok(SwapCall {
                base: call.base,
                quote: call.quote,
                pool_idx: call.poolIdx,
                is_buy: call.isBuy,
                in_base_qty: call.inBaseQty,
                qty: call.qty,
                tip: call.tip,
                limit_price: call.limitPrice,
                min_out: call.minOut,
                reserve_flags: call.reserveFlags,
            });
        }

        anyhow::bail!("Not a swap call")
    }

    /// Decode userCmd() call and its inner command
    fn decode_user_cmd(input: &[u8]) -> Result<DecodedTransaction> {
        let call = userCmdCall::abi_decode(input)
            .context("Failed to decode userCmd")?;

        let callpath = call.callpath;
        let cmd = &call.cmd;

        if cmd.is_empty() {
            return Ok(DecodedTransaction::Unknown);
        }

        let cmd_code = cmd[0];

        match callpath {
            proxy_paths::LP_PROXY => Self::decode_warm_path_cmd(cmd_code, cmd),
            proxy_paths::COLD_PROXY => Self::decode_cold_path_cmd(cmd_code, cmd),
            proxy_paths::KNOCKOUT_LP_PROXY => Self::decode_knockout_cmd(cmd_code, cmd),
            _ => {
                debug!("Unknown callpath: {}", callpath);
                Ok(DecodedTransaction::Unknown)
            }
        }
    }

    /// Decode WarmPath (LP) commands
    fn decode_warm_path_cmd(code: u8, cmd: &[u8]) -> Result<DecodedTransaction> {
        // WarmPath userCmd format:
        // (uint8 code, address base, address quote, uint256 poolIdx,
        //  int24 bidTick, int24 askTick, uint128 liq, uint128 limitLower,
        //  uint128 limitHigher, uint8 reserveFlags, address lpConduit)
        // = 352 bytes total

        if cmd.len() < 352 {
            warn!("WarmPath cmd too short: {} bytes", cmd.len());
            return Ok(DecodedTransaction::Unknown);
        }

        // Decode using ABI (each field is 32 bytes padded)
        let base = Address::from_slice(&cmd[12..32]);
        let quote = Address::from_slice(&cmd[44..64]);
        let pool_idx = U256::from_be_slice(&cmd[64..96]);
        let bid_tick = i32::from_be_bytes(cmd[124..128].try_into().unwrap());
        let ask_tick = i32::from_be_bytes(cmd[156..160].try_into().unwrap());
        let liq = u128::from_be_bytes(cmd[176..192].try_into().unwrap());
        let limit_lower = u128::from_be_bytes(cmd[208..224].try_into().unwrap());
        let limit_higher = u128::from_be_bytes(cmd[240..256].try_into().unwrap());

        let (position_type, change_type) = match code {
            cmd_codes::MINT_RANGE_LIQ | cmd_codes::MINT_RANGE_BASE | cmd_codes::MINT_RANGE_QUOTE => {
                (PositionType::Concentrated, ChangeType::Mint)
            }
            cmd_codes::BURN_RANGE_LIQ | cmd_codes::BURN_RANGE_BASE | cmd_codes::BURN_RANGE_QUOTE => {
                (PositionType::Concentrated, ChangeType::Burn)
            }
            cmd_codes::MINT_AMBIENT_LIQ | cmd_codes::MINT_AMBIENT_BASE | cmd_codes::MINT_AMBIENT_QUOTE => {
                (PositionType::Ambient, ChangeType::Mint)
            }
            cmd_codes::BURN_AMBIENT_LIQ | cmd_codes::BURN_AMBIENT_BASE | cmd_codes::BURN_AMBIENT_QUOTE => {
                (PositionType::Ambient, ChangeType::Burn)
            }
            cmd_codes::HARVEST => {
                (PositionType::Concentrated, ChangeType::Harvest)
            }
            _ => {
                debug!("Unknown WarmPath command code: {}", code);
                return Ok(DecodedTransaction::Unknown);
            }
        };

        Ok(DecodedTransaction::LiquidityChange(LiquidityCall {
            base,
            quote,
            pool_idx,
            position_type,
            change_type,
            bid_tick,
            ask_tick,
            liq,
            limit_lower,
            limit_higher,
        }))
    }

    /// Decode ColdPath commands
    fn decode_cold_path_cmd(code: u8, cmd: &[u8]) -> Result<DecodedTransaction> {
        match code {
            cmd_codes::INIT_POOL => {
                // initPool format: (uint8 code, address base, address quote, uint256 poolIdx, uint128 price)
                if cmd.len() < 160 {
                    return Ok(DecodedTransaction::Unknown);
                }

                let base = Address::from_slice(&cmd[12..32]);
                let quote = Address::from_slice(&cmd[44..64]);
                let pool_idx = U256::from_be_slice(&cmd[64..96]);
                let price = u128::from_be_bytes(cmd[112..128].try_into().unwrap());

                Ok(DecodedTransaction::PoolInit(PoolInitCall {
                    base,
                    quote,
                    pool_idx,
                    price,
                }))
            }
            _ => {
                debug!("Ignoring ColdPath command code: {}", code);
                Ok(DecodedTransaction::Unknown)
            }
        }
    }

    /// Decode Knockout commands
    fn decode_knockout_cmd(code: u8, cmd: &[u8]) -> Result<DecodedTransaction> {
        // Knockout format is similar to WarmPath but with different fields
        if cmd.len() < 384 {
            warn!("Knockout cmd too short: {} bytes", cmd.len());
            return Ok(DecodedTransaction::Unknown);
        }

        let base = Address::from_slice(&cmd[12..32]);
        let quote = Address::from_slice(&cmd[44..64]);
        let pool_idx = U256::from_be_slice(&cmd[64..96]);
        let bid_tick = i32::from_be_bytes(cmd[124..128].try_into().unwrap());
        let ask_tick = i32::from_be_bytes(cmd[156..160].try_into().unwrap());
        let is_bid = cmd[191] != 0;

        let operation = match code {
            cmd_codes::MINT_KNOCKOUT => ChangeType::Mint,
            cmd_codes::BURN_KNOCKOUT => ChangeType::Burn,
            cmd_codes::CLAIM_KNOCKOUT => ChangeType::Claim,
            cmd_codes::RECOVER_KNOCKOUT => ChangeType::Recover,
            _ => {
                debug!("Unknown knockout code: {}", code);
                return Ok(DecodedTransaction::Unknown);
            }
        };

        let liq = if code == cmd_codes::MINT_KNOCKOUT || code == cmd_codes::BURN_KNOCKOUT {
            Some(u128::from_be_bytes(cmd[304..320].try_into().unwrap()))
        } else {
            None
        };

        let pivot_time = if code == cmd_codes::RECOVER_KNOCKOUT || code == cmd_codes::CLAIM_KNOCKOUT {
            Some(u32::from_be_bytes(cmd[316..320].try_into().unwrap()))
        } else {
            None
        };

        Ok(DecodedTransaction::KnockoutOp(KnockoutCall {
            base,
            quote,
            pool_idx,
            bid_tick,
            ask_tick,
            is_bid,
            operation,
            liq,
            pivot_time,
        }))
    }

    /// Convert decoded transaction to our internal event types
    pub fn to_event(decoded: &DecodedTransaction, _tx_hash: &str) -> Option<DecodedEvent> {
        match decoded {
            DecodedTransaction::Swap(swap) => Some(DecodedEvent::Swap(SwapEvent {
                base: swap.base,
                quote: swap.quote,
                pool_idx: swap.pool_idx,
                is_buy: swap.is_buy,
                in_base_qty: swap.in_base_qty,
                qty: swap.qty,
                limit_price: swap.limit_price,
                min_out: swap.min_out,
                reserve_flags: swap.reserve_flags,
                base_flow: 0, // Not available from calldata
                quote_flow: 0,
                call_source: "swap".to_string(),
                hook_delta_base: None,
                hook_delta_quote: None,
                hook_fee_override: None,
            })),

            DecodedTransaction::LiquidityChange(liq) => {
                Some(DecodedEvent::LiquidityChange(LiquidityChangeEvent {
                    base: liq.base,
                    quote: liq.quote,
                    pool_idx: liq.pool_idx,
                    position_type: liq.position_type,
                    change_type: liq.change_type,
                    bid_tick: liq.bid_tick,
                    ask_tick: liq.ask_tick,
                    is_bid: false,
                    liq: Some(liq.liq),
                    base_flow: 0, // Not available from calldata
                    quote_flow: 0,
                    call_source: "warmpath".to_string(),
                    pivot_time: None,
                    hook_delta: None,
                }))
            }

            DecodedTransaction::PoolInit(pool) => Some(DecodedEvent::PoolInit(PoolInitEvent {
                base: pool.base,
                quote: pool.quote,
                pool_idx: pool.pool_idx,
            })),

            DecodedTransaction::KnockoutOp(ko) => {
                Some(DecodedEvent::LiquidityChange(LiquidityChangeEvent {
                    base: ko.base,
                    quote: ko.quote,
                    pool_idx: ko.pool_idx,
                    position_type: PositionType::Knockout,
                    change_type: ko.operation,
                    bid_tick: ko.bid_tick,
                    ask_tick: ko.ask_tick,
                    is_bid: ko.is_bid,
                    liq: ko.liq,
                    base_flow: 0,
                    quote_flow: 0,
                    call_source: "knockout".to_string(),
                    pivot_time: ko.pivot_time.map(|t| t as u64),
                    hook_delta: None,
                }))
            }

            DecodedTransaction::Unknown => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::hex;

    #[test]
    fn test_decode_swap() {
        // Example swap calldata from broadcast
        let input = hex::decode(
            "3d719cd9\
             00000000000000000000000030088172d68ba619b363d16bf7041d8182f22bc5\
             000000000000000000000000e91f6ad6c06491494bf0d1db38d91f10f70d6541\
             0000000000000000000000000000000000000000000000000000000000008ca0\
             0000000000000000000000000000000000000000000000000000000000000001\
             0000000000000000000000000000000000000000000000000000000000000001\
             0000000000000000000000000000000000000000000000000de0b6b3a7640000\
             0000000000000000000000000000000000000000000000000000000000000000\
             00000000000000000000000000000000ffffffffffffffffffffffffffffffff\
             0000000000000000000000000000000000000000000000000000000000000000\
             0000000000000000000000000000000000000000000000000000000000000000"
        ).unwrap();

        let result = CalldataDecoder::decode(&input);
        assert!(result.is_ok());
        
        if let Ok(DecodedTransaction::Swap(swap)) = result {
            assert!(swap.is_buy);
            assert!(swap.in_base_qty);
            assert_eq!(swap.qty, 1000000000000000000u128);
        } else {
            panic!("Expected Swap transaction");
        }
    }
}
