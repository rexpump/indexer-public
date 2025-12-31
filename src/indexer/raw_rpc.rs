//! Raw RPC client for chains with non-standard transaction types (like Zilliqa)
//!
//! This module provides a fallback for fetching block data when alloy cannot
//! parse non-standard fields like custom transaction types.

use alloy::primitives::{Address, Bytes};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Minimal transaction structure - only fields we need for RexSwap calldata parsing
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTransaction {
    pub hash: String,
    #[serde(default)]
    pub transaction_index: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default)]
    pub input: Option<String>,
    // We ignore 'type' and other non-standard fields
}

impl RawTransaction {
    /// Get transaction index as u32
    pub fn tx_index(&self) -> u32 {
        self.transaction_index
            .as_ref()
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0) as u32
    }

    /// Get 'to' address if present
    pub fn to_address(&self) -> Option<Address> {
        self.to.as_ref().and_then(|s| s.parse().ok())
    }

    /// Get input data as bytes
    pub fn input_bytes(&self) -> Bytes {
        self.input
            .as_ref()
            .and_then(|s| hex::decode(s.trim_start_matches("0x")).ok())
            .map(Bytes::from)
            .unwrap_or_default()
    }
}

/// Minimal block structure with transactions
/// Uses flatten to ignore extra Zilliqa-specific fields (quorumCertificate, view, etc.)
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RawBlock {
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub transactions: Vec<RawTransaction>,
    // Ignore all other fields from different chains
    #[serde(flatten, default)]
    _extra: serde_json::Value,
}

/// Block structure with only transaction hashes (for fallback)
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RawBlockWithHashes {
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub transactions: Vec<String>, // Just hashes
    // Ignore all other fields
    #[serde(flatten, default)]
    _extra: serde_json::Value,
}

impl RawBlock {
    /// Get block number as u64
    pub fn block_number(&self) -> u64 {
        u64::from_str_radix(self.number.trim_start_matches("0x"), 16).unwrap_or(0)
    }

    /// Get timestamp as u64
    pub fn block_timestamp(&self) -> u64 {
        u64::from_str_radix(self.timestamp.trim_start_matches("0x"), 16).unwrap_or(0)
    }
}

/// JSON-RPC request
#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'a str,
    method: &'a str,
    params: serde_json::Value,
    id: u64,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    #[allow(dead_code)]
    jsonrpc: String,
    result: Option<T>,
    error: Option<JsonRpcError>,
    #[allow(dead_code)]
    id: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Raw RPC client with fallback and batch support
pub struct RawRpcClient {
    client: reqwest::Client,
    primary_url: String,
    fallback_url: Option<String>,
    /// Batch size for block requests
    batch_blocks: usize,
    /// Batch size for transaction requests
    batch_transactions: usize,
}

impl RawRpcClient {
    /// Create new raw RPC client
    pub fn new(url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            primary_url: url.to_string(),
            fallback_url: None,
            batch_blocks: 50,
            batch_transactions: 50,
        }
    }

    /// Create new raw RPC client with fallback URL and batch settings
    pub fn with_fallback(
        url: &str,
        fallback_url: Option<String>,
        batch_blocks: usize,
        batch_transactions: usize,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            primary_url: url.to_string(),
            fallback_url,
            batch_blocks,
            batch_transactions,
        }
    }

    /// Execute RPC request to a specific URL
    async fn execute_request<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        request: &JsonRpcRequest<'_>,
    ) -> Result<Option<T>> {
        let response: JsonRpcResponse<T> = self
            .client
            .post(url)
            .json(request)
            .send()
            .await
            .context("Failed to send RPC request")?
            .json()
            .await
            .context("Failed to parse RPC response")?;

        if let Some(err) = response.error {
            anyhow::bail!("RPC error {}: {}", err.code, err.message);
        }

        Ok(response.result)
    }

    /// Fetch block with full transactions using raw JSON
    /// Tries primary URL first, falls back to fallback_url on error
    /// If "too many transactions" error occurs, fetches transactions individually by hash
    pub async fn get_block_with_transactions(&self, block_num: u64) -> Result<Option<RawBlock>> {
        let block_hex = format!("0x{:x}", block_num);
        
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: "eth_getBlockByNumber",
            params: serde_json::json!([block_hex, true]),
            id: 1,
        };

        // Try primary URL first
        let result = self.execute_request::<RawBlock>(&self.primary_url, &request).await;
        
        match result {
            Ok(Some(block)) => return Ok(Some(block)),
            Ok(None) => {
                // Primary returned null - block not found on this node
                // Try fallback if available (the node might not have this block)
                if let Some(ref fallback) = self.fallback_url {
                    tracing::debug!(
                        "Primary RPC returned null for block {}, trying fallback",
                        block_num
                    );
                    
                    match self.execute_request::<RawBlock>(fallback, &request).await {
                        Ok(Some(block)) => {
                            tracing::debug!("Fallback RPC found block {}", block_num);
                            return Ok(Some(block));
                        }
                        Ok(None) => {
                            tracing::debug!("Fallback also returned null for block {}", block_num);
                            return Ok(None);
                        }
                        Err(fallback_err) => {
                            let fallback_err_str = fallback_err.to_string();
                            if fallback_err_str.contains("too many transactions") {
                                tracing::warn!(
                                    "Block {} has too many transactions on fallback, fetching individually",
                                    block_num
                                );
                                return self.get_block_with_transactions_fallback(fallback, block_num).await;
                            }
                            // Fallback failed with error - propagate so retry can happen
                            tracing::warn!("Fallback failed for block {}: {}", block_num, fallback_err);
                            return Err(fallback_err);
                        }
                    }
                }
                return Ok(None);
            }
            Err(err) => {
                let err_str = err.to_string();
                
                // Check if this is "too many transactions" error
                if err_str.contains("too many transactions") {
                    tracing::warn!(
                        "Block {} has too many transactions, fetching individually",
                        block_num
                    );
                    return self.get_block_with_transactions_fallback(&self.primary_url, block_num).await;
                }
                
                // Try fallback URL if available
                if let Some(ref fallback) = self.fallback_url {
                    tracing::debug!(
                        "Primary RPC failed for block {}, trying fallback: {}",
                        block_num,
                        err
                    );
                    
                    let fallback_result = self.execute_request::<RawBlock>(fallback, &request).await;
                    
                    match fallback_result {
                        Ok(Some(block)) => {
                            tracing::debug!("Fallback RPC succeeded for block {}", block_num);
                            return Ok(Some(block));
                        }
                        Ok(None) => {
                            tracing::debug!("Fallback returned null for block {}", block_num);
                            return Ok(None);
                        }
                        Err(fallback_err) => {
                            let fallback_err_str = fallback_err.to_string();
                            
                            // Check if fallback also has "too many transactions" error
                            if fallback_err_str.contains("too many transactions") {
                                tracing::warn!(
                                    "Fallback also has too many transactions for block {}, fetching individually",
                                    block_num
                                );
                                return self.get_block_with_transactions_fallback(fallback, block_num).await;
                            }
                            
                            // Both failed
                            anyhow::bail!(
                                "Primary RPC failed: {}; Fallback RPC failed: {}",
                                err,
                                fallback_err
                            );
                        }
                    }
                } else {
                    return Err(err);
                }
            }
        }
    }

    /// Fallback method: get block with hashes, then fetch transactions via batch request
    async fn get_block_with_transactions_fallback(
        &self,
        url: &str,
        block_num: u64,
    ) -> Result<Option<RawBlock>> {
        let block_hex = format!("0x{:x}", block_num);
        
        // Step 1: Get block with transaction hashes only (false)
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: "eth_getBlockByNumber",
            params: serde_json::json!([block_hex, false]),
            id: 1,
        };
        
        let block_with_hashes: Option<RawBlockWithHashes> = self.execute_request(url, &request).await?;
        
        let Some(block) = block_with_hashes else {
            return Ok(None);
        };
        
        let tx_count = block.transactions.len();
        if tx_count == 0 {
            return Ok(Some(RawBlock {
                number: block.number,
                timestamp: block.timestamp,
                transactions: vec![],
                ..Default::default()
            }));
        }
        
        tracing::debug!(
            "Block {} has {} transaction hashes, fetching via batch",
            block_num,
            tx_count
        );
        
        // Step 2: Fetch all transactions via batch request
        let transactions = self
            .batch_get_transactions(url, &block.transactions, self.batch_transactions)
            .await?;
        
        tracing::info!(
            "Fetched {}/{} transactions for block {} via batch",
            transactions.len(),
            tx_count,
            block_num
        );
        
        Ok(Some(RawBlock {
            number: block.number,
            timestamp: block.timestamp,
            transactions,
            ..Default::default()
        }))
    }

    /// Fetch multiple blocks using batch JSON-RPC (with internal chunking)
    /// Returns blocks in order, handles "too many transactions" errors for individual blocks
    #[allow(dead_code)]
    pub async fn batch_get_blocks(
        &self,
        block_numbers: &[u64],
    ) -> Result<Vec<(u64, RawBlock)>> {
        if block_numbers.is_empty() {
            return Ok(vec![]);
        }

        let mut all_blocks = Vec::with_capacity(block_numbers.len());
        
        // Process in chunks according to batch_blocks setting
        for chunk in block_numbers.chunks(self.batch_blocks) {
            let blocks = self.batch_get_blocks_chunk(&self.primary_url, chunk).await?;
            all_blocks.extend(blocks);
        }
        
        Ok(all_blocks)
    }
    
    /// Fetch blocks in a single batch RPC request (no internal chunking)
    /// Use this when batch_size is already controlled by the caller
    pub async fn batch_get_blocks_direct(
        &self,
        block_numbers: &[u64],
    ) -> Result<Vec<(u64, RawBlock)>> {
        if block_numbers.is_empty() {
            return Ok(vec![]);
        }
        
        // Try primary URL first
        let primary_result = self.batch_get_blocks_chunk(&self.primary_url, block_numbers).await;
        
        match primary_result {
            Ok(blocks) => {
                // If we got significantly fewer blocks than requested, try fallback
                // (primary might be pruned and returning nulls)
                if blocks.len() < block_numbers.len() / 2 && self.fallback_url.is_some() {
                    tracing::debug!(
                        "Primary returned only {}/{} blocks, trying fallback",
                        blocks.len(),
                        block_numbers.len()
                    );
                    let fallback = self.fallback_url.as_ref().unwrap();
                    match self.batch_get_blocks_chunk(fallback, block_numbers).await {
                        Ok(fallback_blocks) if fallback_blocks.len() > blocks.len() => {
                            return Ok(fallback_blocks);
                        }
                        _ => {}
                    }
                }
                Ok(blocks)
            }
            Err(e) => {
                // Try fallback if available
                if let Some(ref fallback) = self.fallback_url {
                    tracing::debug!("Primary batch failed, trying fallback: {}", e);
                    self.batch_get_blocks_chunk(fallback, block_numbers).await
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Fetch a chunk of blocks via batch request
    async fn batch_get_blocks_chunk(
        &self,
        url: &str,
        block_numbers: &[u64],
    ) -> Result<Vec<(u64, RawBlock)>> {
        // Build batch request for blocks with full transactions
        let batch_requests: Vec<serde_json::Value> = block_numbers
            .iter()
            .enumerate()
            .map(|(i, &block_num)| {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "eth_getBlockByNumber",
                    "params": [format!("0x{:x}", block_num), true],
                    "id": i
                })
            })
            .collect();

        // Send batch request
        let response = self
            .client
            .post(url)
            .json(&batch_requests)
            .send()
            .await
            .context("Failed to send batch blocks request")?;

        let batch_responses: Vec<JsonRpcResponse<RawBlock>> = response
            .json()
            .await
            .context("Failed to parse batch blocks response")?;

        let mut results = Vec::with_capacity(block_numbers.len());
        let mut failed_blocks: Vec<u64> = Vec::new();

        // Process responses
        for (i, resp) in batch_responses.into_iter().enumerate() {
            let block_num = block_numbers[i];
            
            if let Some(err) = resp.error {
                let err_msg = err.message.to_lowercase();
                if err_msg.contains("too many transactions") {
                    tracing::debug!(
                        "Block {} has too many transactions in batch, will fetch separately",
                        block_num
                    );
                    failed_blocks.push(block_num);
                } else {
                    tracing::warn!(
                        "Batch block {} fetch error: {} - {}",
                        block_num,
                        err.code,
                        err.message
                    );
                }
                continue;
            }
            
            if let Some(block) = resp.result {
                results.push((block_num, block));
            } else {
                // Block returned null (not found / pruned node)
                tracing::debug!("Block {} returned null from {}", block_num, url);
                failed_blocks.push(block_num);
            }
        }

        // Handle blocks that returned null or "too many transactions"
        // Try with fallback URL if available, otherwise try individual fetch on same URL
        if !failed_blocks.is_empty() {
            let fetch_url = self.fallback_url.as_deref().unwrap_or(url);
            let is_using_fallback = self.fallback_url.is_some() && fetch_url != url;
            
            if is_using_fallback {
                tracing::debug!(
                    "Fetching {} missing blocks from fallback URL",
                    failed_blocks.len()
                );
            }
            
            for block_num in failed_blocks {
                tracing::debug!("Fetching block {} individually from {}", block_num, fetch_url);
                match self.get_block_with_transactions_fallback(fetch_url, block_num).await {
                    Ok(Some(block)) => {
                        results.push((block_num, block));
                    }
                    Ok(None) => {
                        // Block really doesn't exist on any node
                        tracing::debug!("Block {} not found on any node", block_num);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to fetch block {} individually: {}", block_num, e);
                    }
                }
            }
        }

        Ok(results)
    }

    /// Fetch multiple transactions by hash using batch JSON-RPC
    async fn batch_get_transactions(
        &self,
        url: &str,
        tx_hashes: &[String],
        batch_size: usize,
    ) -> Result<Vec<RawTransaction>> {
        let mut all_transactions = Vec::with_capacity(tx_hashes.len());
        
        // Process in chunks to avoid hitting batch limits
        for (chunk_idx, chunk) in tx_hashes.chunks(batch_size).enumerate() {
            // Build batch request
            let batch_requests: Vec<serde_json::Value> = chunk
                .iter()
                .enumerate()
                .map(|(i, hash)| {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "eth_getTransactionByHash",
                        "params": [hash],
                        "id": chunk_idx * batch_size + i
                    })
                })
                .collect();
            
            // Send batch request
            let response = self
                .client
                .post(url)
                .json(&batch_requests)
                .send()
                .await
                .context("Failed to send batch RPC request")?;
            
            let batch_responses: Vec<JsonRpcResponse<RawTransaction>> = response
                .json()
                .await
                .context("Failed to parse batch RPC response")?;
            
            // Extract transactions from responses
            for resp in batch_responses {
                if let Some(err) = resp.error {
                    tracing::warn!("Batch tx fetch error: {} - {}", err.code, err.message);
                    continue;
                }
                if let Some(tx) = resp.result {
                    all_transactions.push(tx);
                }
            }
        }
        
        Ok(all_transactions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_raw_transaction() {
        let json = r#"{
            "hash": "0xabc123",
            "transactionIndex": "0x0",
            "from": "0x1234567890123456789012345678901234567890",
            "to": "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd",
            "input": "0x1234",
            "type": "0xdd870"
        }"#;

        let tx: RawTransaction = serde_json::from_str(json).unwrap();
        assert_eq!(tx.hash, "0xabc123");
        assert_eq!(tx.tx_index(), 0);
        assert!(tx.to_address().is_some());
        assert_eq!(tx.input_bytes().len(), 2);
    }
}
