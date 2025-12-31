//! RexPump event handler
//!
//! Tracks events from RexPump contracts:
//! - PositionManager: PoolCreated, PoolSwap, HookSwap, PoolStateUpdated, fees
//! - BidWall: BidWallInitialized, BidWallRepositioned, etc.
//! - FairLaunch: FairLaunchCreated, FairLaunchEnded
//! - FeeEscrow: Deposit, Withdrawal

use alloy::primitives::{Address, B256};
use alloy::rpc::types::Log;
use alloy::sol;
use alloy::sol_types::SolEvent;
use anyhow::Result;
use chrono::{TimeZone, Utc};
use tracing::{debug, warn};

use super::{BlockContext, TxContext};
use crate::db::{
    BidWallEventRecord, ClickHouseClient, FairLaunchEventRecord, FeeEscrowEventRecord,
    ReferrerFeeRecord, RexPumpFeeDistributionRecord, RexPumpPoolRecord, RexPumpPoolStateRecord,
    RexPumpSwapRecord,
};

// ============================================================================
// Event definitions - using separate sol! blocks for clarity
// ============================================================================

// PositionManager events
sol! {
    #[derive(Debug)]
    event PoolCreated(
        bytes32 indexed _poolId,
        address _memecoin,
        address _memecoinTreasury,
        uint256 _tokenId,
        bool _currencyFlipped,
        address _creator,
        uint24 _creatorFeeAllocation
    );

    #[derive(Debug)]
    event PoolStateUpdated(
        bytes32 indexed _poolId,
        uint160 _sqrtPriceX96,
        int24 _tick,
        uint24 _protocolFee,
        uint24 _swapFee,
        uint128 _liquidity
    );

    #[derive(Debug)]
    event HookSwap(
        bytes32 indexed id,
        address indexed sender,
        int128 amount0,
        int128 amount1,
        uint128 hookLPfeeAmount0,
        uint128 hookLPfeeAmount1
    );

    #[derive(Debug)]
    event PoolFeesDistributed(
        bytes32 indexed _poolId,
        uint256 _donateAmount,
        uint256 _creatorAmount,
        uint256 _bidWallAmount,
        uint256 _governanceAmount,
        uint256 _protocolAmount
    );

    #[derive(Debug)]
    event ReferrerFeePaid(
        bytes32 indexed _poolId,
        address _recipient,
        address _token,
        uint256 _amount
    );
}

// BidWall events
sol! {
    #[derive(Debug)]
    event BidWallInitialized(
        bytes32 indexed _poolId,
        uint256 _eth,
        int24 _tickLower,
        int24 _tickUpper
    );

    #[derive(Debug)]
    event BidWallRepositioned(
        bytes32 indexed _poolId,
        uint256 _eth,
        int24 _tickLower,
        int24 _tickUpper
    );

    #[derive(Debug)]
    event BidWallClosed(
        bytes32 indexed _poolId,
        address _recipient,
        uint256 _eth
    );

    #[derive(Debug)]
    event BidWallDeposit(
        bytes32 indexed _poolId,
        uint256 _added,
        uint256 _pending
    );

    #[derive(Debug)]
    event BidWallRewardsTransferred(
        bytes32 indexed _poolId,
        address _recipient,
        uint256 _tokens
    );

    #[derive(Debug)]
    event BidWallDisabledStateUpdated(
        bytes32 indexed _poolId,
        bool _disabled
    );
}

// FairLaunch events
sol! {
    #[derive(Debug)]
    event FairLaunchCreated(
        bytes32 indexed _poolId,
        uint256 _tokens,
        uint256 _startsAt,
        uint256 _endsAt
    );

    #[derive(Debug)]
    event FairLaunchEnded(
        bytes32 indexed _poolId,
        uint256 _revenue,
        uint256 _supply,
        uint256 _endedAt
    );
}

// FeeEscrow events
sol! {
    #[derive(Debug)]
    event Deposit(
        bytes32 indexed _poolId,
        address _payee,
        address _token,
        uint256 _amount
    );

    #[derive(Debug)]
    event Withdrawal(
        address _sender,
        address _recipient,
        address _token,
        uint256 _amount
    );
}

// ============================================================================
// Handler implementation
// ============================================================================

/// RexPump contract addresses
#[derive(Debug, Clone)]
pub struct RexPumpContracts {
    pub position_manager: Address,
    pub bidwall: Address,
    pub fairlaunch: Address,
    pub fee_escrow: Address,
}

impl RexPumpContracts {
    pub fn as_vec(&self) -> Vec<Address> {
        vec![
            self.position_manager,
            self.bidwall,
            self.fairlaunch,
            self.fee_escrow,
        ]
    }
}

/// RexPump event handler
pub struct RexPumpHandler {
    contracts: RexPumpContracts,
    topic_signatures: Vec<B256>,
}

impl RexPumpHandler {
    pub fn new(contracts: RexPumpContracts) -> Self {
        // Collect all topic signatures
        let topic_signatures = vec![
            // PositionManager
            PoolCreated::SIGNATURE_HASH,
            PoolStateUpdated::SIGNATURE_HASH,
            HookSwap::SIGNATURE_HASH,
            PoolFeesDistributed::SIGNATURE_HASH,
            ReferrerFeePaid::SIGNATURE_HASH,
            // BidWall
            BidWallInitialized::SIGNATURE_HASH,
            BidWallRepositioned::SIGNATURE_HASH,
            BidWallClosed::SIGNATURE_HASH,
            BidWallDeposit::SIGNATURE_HASH,
            BidWallRewardsTransferred::SIGNATURE_HASH,
            BidWallDisabledStateUpdated::SIGNATURE_HASH,
            // FairLaunch
            FairLaunchCreated::SIGNATURE_HASH,
            FairLaunchEnded::SIGNATURE_HASH,
            // FeeEscrow
            Deposit::SIGNATURE_HASH,
            Withdrawal::SIGNATURE_HASH,
        ];

        Self {
            contracts,
            topic_signatures,
        }
    }

    pub fn name(&self) -> &'static str {
        "rexpump"
    }

    pub fn topic_signatures(&self) -> &[B256] {
        &self.topic_signatures
    }

    pub fn matches_log(&self, log: &Log) -> bool {
        // Check if from one of our contracts
        let contracts = self.contracts.as_vec();
        if !contracts.contains(&log.address()) {
            return false;
        }

        // Check topic
        if log.topics().is_empty() {
            return false;
        }

        self.topic_signatures.contains(&log.topics()[0])
    }

    /// Process PoolCreated event
    async fn process_pool_created(
        &self,
        log: &Log,
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<()> {
        let event = PoolCreated::decode_log(log.as_ref())?;
        let log_index = log.log_index.unwrap_or(0) as u32;

        let record = RexPumpPoolRecord {
            id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
            pool_id: format!("{:?}", event._poolId),
            memecoin_address: format!("{:?}", event._memecoin),
            memecoin_treasury: format!("{:?}", event._memecoinTreasury),
            token_id: event._tokenId.to_string(),
            currency_flipped: event._currencyFlipped,
            creator_address: format!("{:?}", event._creator),
            creator_fee_allocation: event._creatorFeeAllocation.to::<u32>(),
            block_number: block_ctx.block_number,
            block_time: format_timestamp(block_ctx.block_time),
            transaction_hash: tx_ctx.transaction_hash.clone(),
            network: block_ctx.network.clone(),
        };

        db.insert_rexpump_pool(&record).await?;
        debug!("Inserted RexPump pool: {}", record.pool_id);
        Ok(())
    }

    /// Process PoolStateUpdated event
    async fn process_pool_state_updated(
        &self,
        log: &Log,
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<()> {
        let event = PoolStateUpdated::decode_log(log.as_ref())?;
        let log_index = log.log_index.unwrap_or(0) as u32;

        let record = RexPumpPoolStateRecord {
            id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
            transaction_hash: tx_ctx.transaction_hash.clone(),
            log_index,
            pool_id: format!("{:?}", event._poolId),
            sqrt_price_x96: event._sqrtPriceX96.to_string(),
            tick: event._tick.as_i32(),
            protocol_fee: event._protocolFee.to::<u32>(),
            swap_fee: event._swapFee.to::<u32>(),
            liquidity: event._liquidity.to_string(),
            block_number: block_ctx.block_number,
            block_time: format_timestamp(block_ctx.block_time),
            network: block_ctx.network.clone(),
        };

        db.insert_rexpump_pool_state(&record).await?;
        Ok(())
    }

    /// Process HookSwap event (simplified swap with sender)
    async fn process_hook_swap(
        &self,
        log: &Log,
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<()> {
        let event = HookSwap::decode_log(log.as_ref())?;
        let log_index = log.log_index.unwrap_or(0) as u32;

        let record = RexPumpSwapRecord {
            id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
            transaction_hash: tx_ctx.transaction_hash.clone(),
            log_index,
            pool_id: format!("{:?}", event.id),
            sender: format!("{:?}", event.sender),
            amount0: event.amount0.to_string(),
            amount1: event.amount1.to_string(),
            fee0: "0".to_string(),
            fee1: "0".to_string(),
            hook_lp_fee0: Some(event.hookLPfeeAmount0.to_string()),
            hook_lp_fee1: Some(event.hookLPfeeAmount1.to_string()),
            block_number: block_ctx.block_number,
            block_time: format_timestamp(block_ctx.block_time),
            network: block_ctx.network.clone(),
        };

        db.insert_rexpump_swap(&record).await?;
        debug!(
            "Inserted RexPump swap in block {}",
            block_ctx.block_number
        );
        Ok(())
    }

    /// Process PoolFeesDistributed event
    async fn process_fees_distributed(
        &self,
        log: &Log,
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<()> {
        let event = PoolFeesDistributed::decode_log(log.as_ref())?;
        let log_index = log.log_index.unwrap_or(0) as u32;

        let record = RexPumpFeeDistributionRecord {
            id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
            transaction_hash: tx_ctx.transaction_hash.clone(),
            log_index,
            pool_id: format!("{:?}", event._poolId),
            donate_amount: event._donateAmount.to_string(),
            creator_amount: event._creatorAmount.to_string(),
            bidwall_amount: event._bidWallAmount.to_string(),
            governance_amount: event._governanceAmount.to_string(),
            protocol_amount: event._protocolAmount.to_string(),
            block_number: block_ctx.block_number,
            block_time: format_timestamp(block_ctx.block_time),
            network: block_ctx.network.clone(),
        };

        db.insert_rexpump_fee_distribution(&record).await?;
        Ok(())
    }

    /// Process ReferrerFeePaid event
    async fn process_referrer_fee(
        &self,
        log: &Log,
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<()> {
        let event = ReferrerFeePaid::decode_log(log.as_ref())?;
        let log_index = log.log_index.unwrap_or(0) as u32;

        let record = ReferrerFeeRecord {
            id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
            transaction_hash: tx_ctx.transaction_hash.clone(),
            log_index,
            pool_id: format!("{:?}", event._poolId),
            recipient: format!("{:?}", event._recipient),
            token_address: format!("{:?}", event._token),
            amount: event._amount.to_string(),
            block_number: block_ctx.block_number,
            block_time: format_timestamp(block_ctx.block_time),
            network: block_ctx.network.clone(),
        };

        db.insert_referrer_fee(&record).await?;
        Ok(())
    }

    /// Process BidWall events
    async fn process_bidwall_event(
        &self,
        log: &Log,
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<()> {
        let log_index = log.log_index.unwrap_or(0) as u32;
        let topic0 = log.topics()[0];

        let record = if topic0 == BidWallInitialized::SIGNATURE_HASH {
            let event = BidWallInitialized::decode_log(log.as_ref())?;
            BidWallEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: format!("{:?}", event._poolId),
                event_type: "initialized".to_string(),
                eth_amount: Some(event._eth.to_string()),
                tick_lower: Some(event._tickLower.as_i32()),
                tick_upper: Some(event._tickUpper.as_i32()),
                recipient: None,
                tokens: None,
                disabled: None,
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else if topic0 == BidWallRepositioned::SIGNATURE_HASH {
            let event = BidWallRepositioned::decode_log(log.as_ref())?;
            BidWallEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: format!("{:?}", event._poolId),
                event_type: "repositioned".to_string(),
                eth_amount: Some(event._eth.to_string()),
                tick_lower: Some(event._tickLower.as_i32()),
                tick_upper: Some(event._tickUpper.as_i32()),
                recipient: None,
                tokens: None,
                disabled: None,
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else if topic0 == BidWallClosed::SIGNATURE_HASH {
            let event = BidWallClosed::decode_log(log.as_ref())?;
            BidWallEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: format!("{:?}", event._poolId),
                event_type: "closed".to_string(),
                eth_amount: Some(event._eth.to_string()),
                tick_lower: None,
                tick_upper: None,
                recipient: Some(format!("{:?}", event._recipient)),
                tokens: None,
                disabled: None,
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else if topic0 == BidWallDeposit::SIGNATURE_HASH {
            let event = BidWallDeposit::decode_log(log.as_ref())?;
            BidWallEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: format!("{:?}", event._poolId),
                event_type: "deposit".to_string(),
                eth_amount: Some(event._added.to_string()),
                tick_lower: None,
                tick_upper: None,
                recipient: None,
                tokens: None,
                disabled: None,
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else if topic0 == BidWallRewardsTransferred::SIGNATURE_HASH {
            let event = BidWallRewardsTransferred::decode_log(log.as_ref())?;
            BidWallEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: format!("{:?}", event._poolId),
                event_type: "rewards_transferred".to_string(),
                eth_amount: None,
                tick_lower: None,
                tick_upper: None,
                recipient: Some(format!("{:?}", event._recipient)),
                tokens: Some(event._tokens.to_string()),
                disabled: None,
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else if topic0 == BidWallDisabledStateUpdated::SIGNATURE_HASH {
            let event = BidWallDisabledStateUpdated::decode_log(log.as_ref())?;
            BidWallEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: format!("{:?}", event._poolId),
                event_type: "disabled_updated".to_string(),
                eth_amount: None,
                tick_lower: None,
                tick_upper: None,
                recipient: None,
                tokens: None,
                disabled: Some(event._disabled),
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else {
            return Ok(());
        };

        db.insert_bidwall_event(&record).await?;
        Ok(())
    }

    /// Process FairLaunch events
    async fn process_fairlaunch_event(
        &self,
        log: &Log,
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<()> {
        let log_index = log.log_index.unwrap_or(0) as u32;
        let topic0 = log.topics()[0];

        let record = if topic0 == FairLaunchCreated::SIGNATURE_HASH {
            let event = FairLaunchCreated::decode_log(log.as_ref())?;
            FairLaunchEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: format!("{:?}", event._poolId),
                event_type: "created".to_string(),
                tokens: Some(event._tokens.to_string()),
                starts_at: Some(event._startsAt.try_into().unwrap_or(0)),
                ends_at: Some(event._endsAt.try_into().unwrap_or(0)),
                revenue: None,
                supply: None,
                ended_at: None,
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else if topic0 == FairLaunchEnded::SIGNATURE_HASH {
            let event = FairLaunchEnded::decode_log(log.as_ref())?;
            FairLaunchEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: format!("{:?}", event._poolId),
                event_type: "ended".to_string(),
                tokens: None,
                starts_at: None,
                ends_at: None,
                revenue: Some(event._revenue.to_string()),
                supply: Some(event._supply.to_string()),
                ended_at: Some(event._endedAt.try_into().unwrap_or(0)),
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else {
            return Ok(());
        };

        db.insert_fairlaunch_event(&record).await?;
        Ok(())
    }

    /// Process FeeEscrow events
    async fn process_fee_escrow_event(
        &self,
        log: &Log,
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<()> {
        let log_index = log.log_index.unwrap_or(0) as u32;
        let topic0 = log.topics()[0];

        let record = if topic0 == Deposit::SIGNATURE_HASH {
            let event = Deposit::decode_log(log.as_ref())?;
            FeeEscrowEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: Some(format!("{:?}", event._poolId)),
                event_type: "deposit".to_string(),
                payee: Some(format!("{:?}", event._payee)),
                sender: None,
                recipient: None,
                token_address: format!("{:?}", event._token),
                amount: event._amount.to_string(),
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else if topic0 == Withdrawal::SIGNATURE_HASH {
            let event = Withdrawal::decode_log(log.as_ref())?;
            FeeEscrowEventRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                pool_id: None,
                event_type: "withdrawal".to_string(),
                payee: None,
                sender: Some(format!("{:?}", event._sender)),
                recipient: Some(format!("{:?}", event._recipient)),
                token_address: format!("{:?}", event._token),
                amount: event._amount.to_string(),
                block_number: block_ctx.block_number,
                block_time: format_timestamp(block_ctx.block_time),
                network: block_ctx.network.clone(),
            }
        } else {
            return Ok(());
        };

        db.insert_fee_escrow_event(&record).await?;
        Ok(())
    }

    pub async fn process_logs(
        &self,
        logs: &[Log],
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<usize> {
        let mut count = 0;

        for log in logs {
            if log.topics().is_empty() {
                continue;
            }

            let topic0 = log.topics()[0];
            let result = if topic0 == PoolCreated::SIGNATURE_HASH {
                self.process_pool_created(log, block_ctx, tx_ctx, db).await
            } else if topic0 == PoolStateUpdated::SIGNATURE_HASH {
                self.process_pool_state_updated(log, block_ctx, tx_ctx, db)
                    .await
            } else if topic0 == HookSwap::SIGNATURE_HASH {
                self.process_hook_swap(log, block_ctx, tx_ctx, db).await
            } else if topic0 == PoolFeesDistributed::SIGNATURE_HASH {
                self.process_fees_distributed(log, block_ctx, tx_ctx, db)
                    .await
            } else if topic0 == ReferrerFeePaid::SIGNATURE_HASH {
                self.process_referrer_fee(log, block_ctx, tx_ctx, db).await
            } else if topic0 == BidWallInitialized::SIGNATURE_HASH
                || topic0 == BidWallRepositioned::SIGNATURE_HASH
                || topic0 == BidWallClosed::SIGNATURE_HASH
                || topic0 == BidWallDeposit::SIGNATURE_HASH
                || topic0 == BidWallRewardsTransferred::SIGNATURE_HASH
                || topic0 == BidWallDisabledStateUpdated::SIGNATURE_HASH
            {
                self.process_bidwall_event(log, block_ctx, tx_ctx, db).await
            } else if topic0 == FairLaunchCreated::SIGNATURE_HASH
                || topic0 == FairLaunchEnded::SIGNATURE_HASH
            {
                self.process_fairlaunch_event(log, block_ctx, tx_ctx, db)
                    .await
            } else if topic0 == Deposit::SIGNATURE_HASH || topic0 == Withdrawal::SIGNATURE_HASH {
                self.process_fee_escrow_event(log, block_ctx, tx_ctx, db)
                    .await
            } else {
                continue;
            };

            match result {
                Ok(()) => count += 1,
                Err(e) => {
                    warn!(
                        "Failed to process RexPump event in tx {}: {:?}",
                        tx_ctx.transaction_hash, e
                    );
                }
            }
        }

        if count > 0 {
            debug!(
                "Processed {} RexPump events in block {}",
                count, block_ctx.block_number
            );
        }

        Ok(count)
    }
}

/// Format unix timestamp to ClickHouse datetime string
fn format_timestamp(ts: u64) -> String {
    Utc.timestamp_opt(ts as i64, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "1970-01-01 00:00:00".to_string())
}
