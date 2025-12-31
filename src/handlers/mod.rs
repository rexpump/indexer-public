//! Event handlers module
//!
//! Each handler is responsible for processing specific types of blockchain events:
//! - `erc20`: Parses Transfer events for ERC-20 fungible tokens
//! - `erc721`: Parses Transfer events for ERC-721 NFTs
//! - `rexpump`: Parses events from RexPump contracts (PositionManager, BidWall, etc.)
//! - `scilla`: Parses Scilla token transfers (Zilliqa native tokens like USDT/USDC)
//!
//! Uses enum dispatch instead of dyn trait for async compatibility.

pub mod erc20;
pub mod erc721;
pub mod rexpump;
pub mod scilla;

use alloy::primitives::B256;
use alloy::rpc::types::Log;
use anyhow::Result;
use std::collections::HashSet;

use crate::db::ClickHouseClient;

/// Block context passed to handlers
#[derive(Debug, Clone)]
pub struct BlockContext {
    pub block_number: u64,
    pub block_time: u64,
    pub network: String,
}

/// Transaction context passed to handlers
#[derive(Debug, Clone)]
pub struct TxContext {
    pub transaction_hash: String,
    pub transaction_index: u32,
}

/// Enum-based handler dispatch (avoids dyn trait issues with async)
pub enum Handler {
    Erc20(erc20::Erc20Handler),
    Erc721(erc721::Erc721Handler),
    RexPump(rexpump::RexPumpHandler),
    Scilla(scilla::ScillaHandler),
}

impl Handler {
    pub fn name(&self) -> &'static str {
        match self {
            Handler::Erc20(h) => h.name(),
            Handler::Erc721(h) => h.name(),
            Handler::RexPump(h) => h.name(),
            Handler::Scilla(h) => h.name(),
        }
    }

    pub fn topic_signatures(&self) -> Vec<B256> {
        match self {
            Handler::Erc20(h) => h.topic_signatures(),
            Handler::Erc721(h) => h.topic_signatures(),
            Handler::RexPump(h) => h.topic_signatures().to_vec(),
            Handler::Scilla(h) => h.topic_signatures(),
        }
    }

    pub fn matches_log(&self, log: &Log) -> bool {
        match self {
            Handler::Erc20(h) => h.matches_log(log),
            Handler::Erc721(h) => h.matches_log(log),
            Handler::RexPump(h) => h.matches_log(log),
            Handler::Scilla(h) => h.matches_log(log),
        }
    }

    pub async fn process_logs(
        &self,
        logs: &[Log],
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<usize> {
        match self {
            Handler::Erc20(h) => h.process_logs(logs, block_ctx, tx_ctx, db).await,
            Handler::Erc721(h) => h.process_logs(logs, block_ctx, tx_ctx, db).await,
            Handler::RexPump(h) => h.process_logs(logs, block_ctx, tx_ctx, db).await,
            Handler::Scilla(h) => h.process_logs(logs, block_ctx, tx_ctx, db).await,
        }
    }
}

/// Registry of all event handlers
pub struct HandlerRegistry {
    handlers: Vec<Handler>,
    /// Cached set of all topic signatures for fast filtering
    all_topics: HashSet<B256>,
}

impl HandlerRegistry {
    /// Create a new handler registry
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            all_topics: HashSet::new(),
        }
    }

    /// Register ERC-20 handler
    pub fn register_erc20(&mut self, handler: erc20::Erc20Handler) {
        for topic in handler.topic_signatures() {
            self.all_topics.insert(topic);
        }
        self.handlers.push(Handler::Erc20(handler));
    }

    /// Register ERC-721 (NFT) handler
    pub fn register_erc721(&mut self, handler: erc721::Erc721Handler) {
        for topic in handler.topic_signatures() {
            self.all_topics.insert(topic);
        }
        self.handlers.push(Handler::Erc721(handler));
    }

    /// Register RexPump handler
    pub fn register_rexpump(&mut self, handler: rexpump::RexPumpHandler) {
        for topic in handler.topic_signatures() {
            self.all_topics.insert(*topic);
        }
        self.handlers.push(Handler::RexPump(handler));
    }

    /// Register Scilla handler (Zilliqa native tokens)
    pub fn register_scilla(&mut self, handler: scilla::ScillaHandler) {
        for topic in handler.topic_signatures() {
            self.all_topics.insert(topic);
        }
        self.handlers.push(Handler::Scilla(handler));
    }

    /// Get all registered handlers
    pub fn handlers(&self) -> &[Handler] {
        &self.handlers
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Check if any handler is interested in this log
    pub fn has_matching_handler(&self, log: &Log) -> bool {
        if log.topics().is_empty() {
            return false;
        }
        self.all_topics.contains(&log.topics()[0])
    }

    /// Process logs with all matching handlers
    pub async fn process_logs(
        &self,
        logs: &[Log],
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<usize> {
        let mut total_processed = 0;

        for handler in &self.handlers {
            // Filter logs for this handler
            let matching_logs: Vec<Log> = logs
                .iter()
                .filter(|log| handler.matches_log(log))
                .cloned()
                .collect();

            if !matching_logs.is_empty() {
                let count = handler
                    .process_logs(&matching_logs, block_ctx, tx_ctx, db)
                    .await?;
                total_processed += count;
            }
        }

        Ok(total_processed)
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
