//! ERC-721 (NFT) Transfer event handler
//!
//! Tracks Transfer events for non-fungible tokens.
//! Writes to nft_transfers table.

use alloy::primitives::{Address, B256, U256};
use alloy::rpc::types::Log;
use alloy::sol;
use alloy::sol_types::SolEvent;
use anyhow::Result;
use chrono::{TimeZone, Utc};
use tracing::debug;

use super::{BlockContext, TxContext};
use crate::db::{ClickHouseClient, NftTransferRecord};

// ERC-721 Transfer event - tokenId is indexed (4 topics total)
// topic0: 0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef
sol! {
    #[derive(Debug)]
    event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);
}

/// ERC-721 (NFT) event handler
pub struct Erc721Handler {
    /// List of NFT contract addresses to track (empty = track all)
    watched_contracts: Vec<Address>,
    /// Transfer event topic signature
    transfer_topic: B256,
}

impl Erc721Handler {
    /// Create a new ERC-721 handler
    ///
    /// If `contracts` is empty, will track ALL NFT transfers (use with caution!)
    pub fn new(contracts: Vec<Address>) -> Self {
        Self {
            watched_contracts: contracts,
            transfer_topic: Transfer::SIGNATURE_HASH,
        }
    }

    pub fn name(&self) -> &'static str {
        "erc721"
    }

    pub fn topic_signatures(&self) -> Vec<B256> {
        vec![self.transfer_topic]
    }

    pub fn matches_log(&self, log: &Log) -> bool {
        // Check topic signature
        if log.topics().is_empty() || log.topics()[0] != self.transfer_topic {
            return false;
        }

        // ERC-721 Transfer has 4 topics: sig, from, to, tokenId (all indexed)
        // ERC-20 Transfer has 3 topics: sig, from, to (value in data)
        if log.topics().len() != 4 {
            return false;
        }

        // If we have a watch list, check contract address
        if !self.watched_contracts.is_empty() && !self.watched_contracts.contains(&log.address()) {
            return false;
        }

        true
    }

    /// Parse NFT Transfer event from log
    fn parse_transfer(&self, log: &Log) -> Option<(Address, Address, U256)> {
        match Transfer::decode_log(log.as_ref()) {
            Ok(event) => Some((event.from, event.to, event.tokenId)),
            Err(e) => {
                debug!("Failed to decode ERC-721 Transfer: {:?}", e);
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

        for log in logs {
            if let Some((from, to, token_id)) = self.parse_transfer(log) {
                let log_index = log.log_index.unwrap_or(0) as u32;

                let record = NftTransferRecord {
                    id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                    transaction_hash: tx_ctx.transaction_hash.clone(),
                    log_index,
                    contract_address: format!("{:?}", log.address()),
                    from_address: format!("{:?}", from),
                    to_address: format!("{:?}", to),
                    token_id: token_id.to_string(),
                    block_number: block_ctx.block_number,
                    block_time: block_time.clone(),
                    network: block_ctx.network.clone(),
                    collection_name: None,
                    token_uri: None,
                };

                records.push(record);
            }
        }

        let count = records.len();
        if !records.is_empty() {
            db.insert_nft_transfers_batch(&records).await?;
            debug!(
                "Inserted {} NFT transfers in block {}",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_topic() {
        // ERC-721 uses same Transfer topic as ERC-20:
        // keccak256("Transfer(address,address,uint256)")
        let expected = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
        assert_eq!(format!("{:?}", Transfer::SIGNATURE_HASH), expected);
    }
}
