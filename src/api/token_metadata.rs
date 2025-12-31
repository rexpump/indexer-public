//! Token metadata fetcher
//!
//! Fetches ERC-20 token metadata (name, symbol, decimals) from RPC node.
//! Also supports Scilla tokens (Zilliqa native) via GetSmartContractInit API.

use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::{debug, warn};

use crate::db::queries::erc20::{Erc20Queries, Erc20TokenRow};
use crate::db::schema::erc20::Erc20TokenRecord;
use crate::db::ClickHouseClient;

// Function selectors for ERC-20 view functions
const NAME_SELECTOR: [u8; 4] = [0x06, 0xfd, 0xde, 0x03]; // name()
const SYMBOL_SELECTOR: [u8; 4] = [0x95, 0xd8, 0x9b, 0x41]; // symbol()
const DECIMALS_SELECTOR: [u8; 4] = [0x31, 0x3c, 0xe5, 0x67]; // decimals()

/// Scilla contract init variable
#[derive(Debug, Deserialize)]
struct ScillaInitVar {
    vname: String,
    value: String,
    #[serde(rename = "type")]
    _var_type: String,
}

/// Scilla API response
#[derive(Debug, Deserialize)]
struct ScillaRpcResponse {
    result: Option<Vec<ScillaInitVar>>,
}

/// Fetch token metadata from RPC
pub struct TokenMetadataFetcher {
    rpc_url: String,
    /// Zilliqa Scilla API URL (for native tokens)
    scilla_api_url: Option<String>,
}

impl TokenMetadataFetcher {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            scilla_api_url: None,
        }
    }

    /// Create with Scilla API support (for Zilliqa networks)
    pub fn with_scilla_api(rpc_url: &str, scilla_api_url: &str) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            scilla_api_url: Some(scilla_api_url.to_string()),
        }
    }

    /// Fetch metadata for a token from RPC
    /// Falls back to Scilla API if ERC-20 calls fail (for Zilliqa native tokens)
    pub async fn fetch_metadata(&self, token_address: &str) -> Result<TokenMetadata> {
        let address: Address = token_address
            .parse()
            .context("Invalid token address")?;

        let provider = ProviderBuilder::new()
            .on_http(self.rpc_url.parse().context("Invalid RPC URL")?);

        // Fetch name
        let name = self.call_string(&provider, address, &NAME_SELECTOR).await.unwrap_or_else(|e| {
            debug!("Failed to fetch name for {}: {}", token_address, e);
            "Unknown".to_string()
        });

        // Fetch symbol
        let symbol = self.call_string(&provider, address, &SYMBOL_SELECTOR).await.unwrap_or_else(|e| {
            debug!("Failed to fetch symbol for {}: {}", token_address, e);
            "???".to_string()
        });

        // Fetch decimals
        let decimals = self.call_uint8(&provider, address, &DECIMALS_SELECTOR).await.unwrap_or_else(|e| {
            debug!("Failed to fetch decimals for {}: {}", token_address, e);
            18 // Default to 18
        });

        // If ERC-20 calls failed, try Scilla API (Zilliqa native tokens)
        if (name == "Unknown" || symbol == "???") && self.scilla_api_url.is_some() {
            debug!("ERC-20 metadata failed, trying Scilla API for {}", token_address);
            if let Ok(scilla_meta) = self.fetch_scilla_metadata(token_address).await {
                return Ok(scilla_meta);
            }
        }

        Ok(TokenMetadata {
            address: token_address.to_lowercase(),
            name,
            symbol,
            decimals,
        })
    }

    /// Fetch metadata for a Scilla token via GetSmartContractInit API
    async fn fetch_scilla_metadata(&self, token_address: &str) -> Result<TokenMetadata> {
        let scilla_url = self.scilla_api_url.as_ref()
            .context("Scilla API URL not configured")?;
        
        let client = reqwest::Client::new();
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "GetSmartContractInit",
            "params": [token_address],
            "id": 1
        });
        
        let response = client
            .post(scilla_url)
            .json(&request_body)
            .send()
            .await
            .context("Failed to call Scilla API")?;
        
        let data: ScillaRpcResponse = response
            .json()
            .await
            .context("Failed to parse Scilla API response")?;
        
        let init_vars = data.result.context("No result from Scilla API")?;
        
        let mut name = "Unknown".to_string();
        let mut symbol = "???".to_string();
        let mut decimals: u8 = 18;
        
        for var in init_vars {
            match var.vname.as_str() {
                "name" => name = var.value,
                "symbol" => symbol = var.value,
                "decimals" => {
                    if let Ok(d) = var.value.parse::<u8>() {
                        decimals = d;
                    }
                }
                _ => {}
            }
        }
        
        debug!(
            "Scilla metadata for {}: name={}, symbol={}, decimals={}",
            token_address, name, symbol, decimals
        );
        
        Ok(TokenMetadata {
            address: token_address.to_lowercase(),
            name,
            symbol,
            decimals,
        })
    }

    async fn call_string<P: Provider>(&self, provider: &P, address: Address, selector: &[u8; 4]) -> Result<String> {
        let tx = alloy::rpc::types::TransactionRequest::default()
            .to(address)
            .input(Bytes::copy_from_slice(selector).into());
        
        let result = provider
            .call(tx)
            .await
            .context("eth_call failed")?;
        
        // Try to decode as ABI-encoded string
        if result.len() >= 64 {
            // Standard ABI encoding: offset (32 bytes) + length (32 bytes) + data
            let offset = U256::from_be_slice(&result[0..32]).to::<usize>();
            if offset < result.len() {
                let len_start = offset.min(result.len() - 32);
                let length = U256::from_be_slice(&result[len_start..len_start + 32]).to::<usize>();
                let data_start = (len_start + 32).min(result.len());
                let data_end = (data_start + length).min(result.len());
                if let Ok(s) = String::from_utf8(result[data_start..data_end].to_vec()) {
                    return Ok(s.trim_end_matches('\0').to_string());
                }
            }
        }
        
        // Try bytes32 format (some tokens use this)
        if result.len() >= 32 {
            let s = String::from_utf8_lossy(&result[..32])
                .trim_end_matches('\0')
                .to_string();
            if !s.is_empty() {
                return Ok(s);
            }
        }

        anyhow::bail!("Failed to decode string response")
    }

    async fn call_uint8<P: Provider>(&self, provider: &P, address: Address, selector: &[u8; 4]) -> Result<u8> {
        let tx = alloy::rpc::types::TransactionRequest::default()
            .to(address)
            .input(Bytes::copy_from_slice(selector).into());
        
        let result = provider
            .call(tx)
            .await
            .context("eth_call failed")?;
        
        if result.len() >= 32 {
            // Decimals is returned as uint256, take last byte
            let value = U256::from_be_slice(&result[..32]);
            return Ok(value.to::<u8>());
        }
        
        anyhow::bail!("Failed to decode uint8 response")
    }
}

/// Token metadata
#[derive(Debug, Clone)]
pub struct TokenMetadata {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
}

/// Get token metadata, fetching from RPC if not in database
pub async fn get_or_fetch_token(
    db: &ClickHouseClient,
    fetcher: &TokenMetadataFetcher,
    network: &str,
    address: &str,
) -> Result<Erc20TokenRow> {
    // Check if we have it in database
    if let Some(token) = Erc20Queries::get_token(db, network, address).await? {
        return Ok(token);
    }

    // Fetch from RPC
    debug!("Fetching metadata for token {} from RPC", address);
    let metadata = fetcher.fetch_metadata(address).await?;

    // Save to database
    let record = Erc20TokenRecord {
        address: metadata.address.clone(),
        name: metadata.name.clone(),
        symbol: metadata.symbol.clone(),
        decimals: metadata.decimals,
        network: network.to_string(),
        first_seen_block: 0, // Unknown
    };
    
    if let Err(e) = Erc20Queries::insert_token(db, &record).await {
        warn!("Failed to save token metadata: {}", e);
    }

    Ok(Erc20TokenRow {
        address: metadata.address,
        name: metadata.name,
        symbol: metadata.symbol,
        decimals: metadata.decimals,
        network: network.to_string(),
        first_seen_block: 0,
    })
}

/// Get multiple tokens metadata, fetching missing ones from RPC
pub async fn get_or_fetch_tokens(
    db: &ClickHouseClient,
    fetcher: &TokenMetadataFetcher,
    network: &str,
    addresses: &[String],
) -> Vec<Erc20TokenRow> {
    let mut result = Vec::with_capacity(addresses.len());

    for address in addresses {
        match get_or_fetch_token(db, fetcher, network, address).await {
            Ok(token) => result.push(token),
            Err(e) => {
                warn!("Failed to get metadata for {}: {}", address, e);
                // Add placeholder
                result.push(Erc20TokenRow {
                    address: address.to_lowercase(),
                    name: "Unknown".to_string(),
                    symbol: "???".to_string(),
                    decimals: 18,
                    network: network.to_string(),
                    first_seen_block: 0,
                });
            }
        }
    }

    result
}
