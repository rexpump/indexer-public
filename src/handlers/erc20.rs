//! ERC-20 Transfer event handler
//!
//! Tracks Transfer events for fungible tokens.
//! Writes to token_transfers and wallet_tokens tables.

use std::collections::HashSet;

use alloy::primitives::{Address, B256, U256};
use alloy::rpc::types::Log;
use alloy::sol;
use alloy::sol_types::SolEvent;
use anyhow::Result;
use chrono::{TimeZone, Utc};
use tracing::debug;

use super::{BlockContext, TxContext};
use crate::db::schema::erc20::WalletTokenRecord;
use crate::db::{ClickHouseClient, TokenTransferRecord};

// ERC-20 Transfer event - value is NOT indexed (3 topics, value in data)
// topic0: 0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef
sol! {
    #[derive(Debug)]
    event Transfer(address indexed from, address indexed to, uint256 value);
}

/// ERC-20 event handler
pub struct Erc20Handler {
    /// List of token addresses to track (empty = track all)
    watched_tokens: Vec<Address>,
    /// Transfer event topic signature
    transfer_topic: B256,
}

impl Erc20Handler {
    /// Create a new ERC-20 handler
    ///
    /// If `tokens` is empty, will track ALL ERC-20 transfers (use with caution!)
    pub fn new(tokens: Vec<Address>) -> Self {
        Self {
            watched_tokens: tokens,
            transfer_topic: Transfer::SIGNATURE_HASH,
        }
    }

    pub fn name(&self) -> &'static str {
        "erc20"
    }

    pub fn topic_signatures(&self) -> Vec<B256> {
        vec![self.transfer_topic]
    }

    pub fn matches_log(&self, log: &Log) -> bool {
        // Check topic signature
        if log.topics().is_empty() || log.topics()[0] != self.transfer_topic {
            return false;
        }

        // ERC-20 Transfer has 3 topics: sig, from, to (value in data)
        // ERC-721 Transfer has 4 topics: sig, from, to, tokenId (all indexed)
        // We only handle ERC-20 here (3 topics)
        if log.topics().len() != 3 {
            return false;
        }

        // If we have a watch list, check token address
        if !self.watched_tokens.is_empty() && !self.watched_tokens.contains(&log.address()) {
            return false;
        }

        true
    }

    /// Parse ERC-20 Transfer event from log
    fn parse_transfer(&self, log: &Log) -> Option<(Address, Address, U256)> {
        match Transfer::decode_log(log.as_ref()) {
            Ok(event) => Some((event.from, event.to, event.value)),
            Err(e) => {
                debug!("Failed to decode ERC-20 Transfer: {:?}", e);
                None
            }
        }
    }

    pub async fn process_logs(
        &self,
        logs: &[Log],
        block_ctx: &BlockContext,
        tx_ctx: &TxContext,
        db: &ClickHouseClient,
    ) -> Result<usize> {
        let mut records = Vec::new();
        let block_time = format_timestamp(block_ctx.block_time);
        
        // Track unique wallet-token pairs for wallet_tokens table
        let mut wallet_token_pairs: HashSet<(String, String)> = HashSet::new();

        for log in logs {
            if let Some((from, to, value)) = self.parse_transfer(log) {
                let log_index = log.log_index.unwrap_or(0) as u32;
                let token_address = format!("{:?}", log.address());
                let from_address = format!("{:?}", from);
                let to_address = format!("{:?}", to);

                let record = TokenTransferRecord {
                    id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                    transaction_hash: tx_ctx.transaction_hash.clone(),
                    log_index,
                    token_address: token_address.clone(),
                    from_address: from_address.clone(),
                    to_address: to_address.clone(),
                    amount: value.to_string(),
                    block_number: block_ctx.block_number,
                    block_time: block_time.clone(),
                    network: block_ctx.network.clone(),
                    token_symbol: None,
                    token_decimals: None,
                };

                records.push(record);
                
                // Track wallet-token interactions (skip zero address)
                let zero_addr = "0x0000000000000000000000000000000000000000";
                if from_address != zero_addr {
                    wallet_token_pairs.insert((from_address, token_address.clone()));
                }
                if to_address != zero_addr {
                    wallet_token_pairs.insert((to_address, token_address));
                }
            }
        }

        let count = records.len();
        if !records.is_empty() {
            db.insert_token_transfers_batch(&records).await?;
            debug!(
                "Inserted {} ERC-20 transfers in block {}",
                count, block_ctx.block_number
            );
            
            // Insert wallet-token interactions
            if !wallet_token_pairs.is_empty() {
                let wallet_token_records: Vec<WalletTokenRecord> = wallet_token_pairs
                    .into_iter()
                    .map(|(wallet, token)| WalletTokenRecord {
                        wallet_address: wallet,
                        token_address: token,
                        last_interaction: block_time.clone(),
                        network: block_ctx.network.clone(),
                    })
                    .collect();
                
                db.insert_wallet_tokens_batch(&wallet_token_records).await?;
                debug!(
                    "Updated {} wallet-token interactions in block {}",
                    wallet_token_records.len(),
                    block_ctx.block_number
                );
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_topic() {
        // ERC-20 Transfer topic:
        // keccak256("Transfer(address,address,uint256)")
        let expected = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
        assert_eq!(format!("{:?}", Transfer::SIGNATURE_HASH), expected);
    }
}
