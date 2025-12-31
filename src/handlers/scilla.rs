//! Scilla Token Transfer handler (Zilliqa native tokens)
//!
//! Parses Scilla ZRC-2 token transfer events which use a custom event format.
//! These are native Zilliqa tokens (USDT, USDC, etc.) written in Scilla language.
//!
//! Scilla events have:
//! - Topic: 0xa5901fdb53ef45260c18811f35461e0eda2b6133d807dabbfb65314dd4fc2fac
//! - Data: ABI-encoded JSON with event details
//!
//! Example decoded data:
//! ```json
//! {
//!   "address": "0x818ca2e217e060ad17b7bd0124a483a1f66930a9",
//!   "_eventname": "TransferSuccess",
//!   "params": [
//!     {"vname": "sender", "value": "0x...", "type": "ByStr20"},
//!     {"vname": "recipient", "value": "0x...", "type": "ByStr20"},
//!     {"vname": "amount", "value": "1000000", "type": "Uint128"}
//!   ]
//! }
//! ```

use std::collections::HashSet;

use alloy::primitives::B256;
use alloy::rpc::types::Log;
use anyhow::Result;
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use tracing::{debug, warn};

use super::{BlockContext, TxContext};
use crate::db::schema::erc20::WalletTokenRecord;
use crate::db::{ClickHouseClient, TokenTransferRecord};

/// Scilla event topic hash
/// This is the topic for ALL Scilla events on Zilliqa EVM
const SCILLA_EVENT_TOPIC: &str = "0xa5901fdb53ef45260c18811f35461e0eda2b6133d807dabbfb65314dd4fc2fac";

/// Scilla event JSON structure
#[derive(Debug, Deserialize)]
struct ScillaEvent {
    /// Contract address that emitted the event
    address: String,
    /// Event name (e.g., "TransferSuccess", "TransferFromSuccess")
    #[serde(rename = "_eventname")]
    event_name: String,
    /// Event parameters
    params: Vec<ScillaParam>,
}

/// Scilla event parameter
#[derive(Debug, Deserialize)]
struct ScillaParam {
    /// Parameter name
    vname: String,
    /// Parameter value (as string)
    value: String,
    /// Parameter type
    #[serde(rename = "type")]
    _param_type: String,
}

/// Parsed Scilla transfer data
#[derive(Debug)]
struct ScillaTransfer {
    /// Token contract address
    token_address: String,
    /// Sender address
    from: String,
    /// Recipient address  
    to: String,
    /// Transfer amount
    amount: String,
}

/// Scilla token handler for Zilliqa native tokens
pub struct ScillaHandler {
    /// Scilla event topic
    scilla_topic: B256,
}

impl ScillaHandler {
    /// Create a new Scilla handler
    pub fn new() -> Self {
        let topic_bytes = hex::decode(&SCILLA_EVENT_TOPIC[2..])
            .expect("Invalid Scilla topic hex");
        let mut topic_array = [0u8; 32];
        topic_array.copy_from_slice(&topic_bytes);
        
        Self {
            scilla_topic: B256::from(topic_array),
        }
    }

    pub fn name(&self) -> &'static str {
        "scilla"
    }

    pub fn topic_signatures(&self) -> Vec<B256> {
        vec![self.scilla_topic]
    }

    pub fn matches_log(&self, log: &Log) -> bool {
        // Check for Scilla event topic
        if log.topics().is_empty() || log.topics()[0] != self.scilla_topic {
            return false;
        }
        
        // Try to parse and check if it's a transfer event
        if let Some(event) = self.parse_scilla_event(log) {
            return Self::is_transfer_event(&event.event_name);
        }
        
        false
    }

    /// Check if event name indicates a transfer
    fn is_transfer_event(event_name: &str) -> bool {
        matches!(
            event_name,
            "TransferSuccess" | "TransferFromSuccess" | "Transfer"
        )
    }

    /// Parse Scilla event from log data
    fn parse_scilla_event(&self, log: &Log) -> Option<ScillaEvent> {
        let data = log.data().data.as_ref();
        
        // Scilla events are ABI-encoded: offset (32 bytes) + length (32 bytes) + JSON data
        if data.len() < 64 {
            return None;
        }
        
        // Read offset (first 32 bytes) - should be 0x20 (32)
        let offset = u64::from_be_bytes(data[24..32].try_into().ok()?) as usize;
        if offset != 32 || data.len() < offset + 32 {
            return None;
        }
        
        // Read length (next 32 bytes)
        let length = u64::from_be_bytes(data[offset + 24..offset + 32].try_into().ok()?) as usize;
        if data.len() < offset + 32 + length {
            return None;
        }
        
        // Extract JSON string
        let json_start = offset + 32;
        let json_bytes = &data[json_start..json_start + length];
        
        // Parse JSON
        let json_str = std::str::from_utf8(json_bytes).ok()?;
        
        match serde_json::from_str::<ScillaEvent>(json_str) {
            Ok(event) => Some(event),
            Err(e) => {
                debug!("Failed to parse Scilla event JSON: {}", e);
                None
            }
        }
    }

    /// Extract transfer data from Scilla event
    fn extract_transfer(&self, event: &ScillaEvent) -> Option<ScillaTransfer> {
        if !Self::is_transfer_event(&event.event_name) {
            return None;
        }
        
        // Find sender, recipient, amount in params
        let mut from = None;
        let mut to = None;
        let mut amount = None;
        
        for param in &event.params {
            match param.vname.as_str() {
                "sender" | "from" | "_sender" => from = Some(param.value.clone()),
                "recipient" | "to" | "_recipient" => to = Some(param.value.clone()),
                "amount" | "value" | "tokens" => amount = Some(param.value.clone()),
                _ => {}
            }
        }
        
        // Check if all required fields are present
        let has_from = from.is_some();
        let has_to = to.is_some();
        let has_amount = amount.is_some();
        
        match (from, to, amount) {
            (Some(from), Some(to), Some(amount)) => Some(ScillaTransfer {
                token_address: event.address.clone(),
                from,
                to,
                amount,
            }),
            _ => {
                debug!(
                    "Scilla transfer missing fields: from={}, to={}, amount={}",
                    has_from, has_to, has_amount
                );
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
        
        // Track unique wallet-token pairs
        let mut wallet_token_pairs: HashSet<(String, String)> = HashSet::new();

        for log in logs {
            let Some(event) = self.parse_scilla_event(log) else {
                continue;
            };
            
            let Some(transfer) = self.extract_transfer(&event) else {
                continue;
            };
            
            let log_index = log.log_index.unwrap_or(0) as u32;
            
            // Normalize addresses to lowercase
            let token_address = transfer.token_address.to_lowercase();
            let from_address = transfer.from.to_lowercase();
            let to_address = transfer.to.to_lowercase();

            let record = TokenTransferRecord {
                id: format!("{}-{}", tx_ctx.transaction_hash, log_index),
                transaction_hash: tx_ctx.transaction_hash.clone(),
                log_index,
                token_address: token_address.clone(),
                from_address: from_address.clone(),
                to_address: to_address.clone(),
                amount: transfer.amount,
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

        let count = records.len();
        if !records.is_empty() {
            db.insert_token_transfers_batch(&records).await?;
            debug!(
                "Inserted {} Scilla transfers in block {}",
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
                    "Updated {} wallet-token interactions (Scilla) in block {}",
                    wallet_token_records.len(),
                    block_ctx.block_number
                );
            }
        }

        Ok(count)
    }
}

impl Default for ScillaHandler {
    fn default() -> Self {
        Self::new()
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
    fn test_scilla_topic() {
        let handler = ScillaHandler::new();
        assert_eq!(
            format!("{:?}", handler.scilla_topic),
            SCILLA_EVENT_TOPIC
        );
    }

    #[test]
    fn test_parse_scilla_json() {
        let json = r#"{"address":"0x818ca2e217e060ad17b7bd0124a483a1f66930a9","_eventname":"TransferSuccess","params":[{"vname":"sender","value":"0x1e2c56421fc7e558245f794f079d93615557d80a","type":"ByStr20"},{"vname":"recipient","value":"0xec6bb19886c9d5f5125dfc739362bf54aa23d51f","type":"ByStr20"},{"vname":"amount","value":"1000000","type":"Uint128"}]}"#;
        
        let event: ScillaEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_name, "TransferSuccess");
        assert_eq!(event.params.len(), 3);
        
        let handler = ScillaHandler::new();
        let transfer = handler.extract_transfer(&event).unwrap();
        assert_eq!(transfer.from, "0x1e2c56421fc7e558245f794f079d93615557d80a");
        assert_eq!(transfer.to, "0xec6bb19886c9d5f5125dfc739362bf54aa23d51f");
        assert_eq!(transfer.amount, "1000000");
    }

    #[test]
    fn test_is_transfer_event() {
        assert!(ScillaHandler::is_transfer_event("TransferSuccess"));
        assert!(ScillaHandler::is_transfer_event("TransferFromSuccess"));
        assert!(ScillaHandler::is_transfer_event("Transfer"));
        assert!(!ScillaHandler::is_transfer_event("Mint"));
        assert!(!ScillaHandler::is_transfer_event("Approval"));
    }
}
