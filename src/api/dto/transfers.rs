//! DTOs for ERC20 and ERC721 transfer endpoints

use serde::Serialize;

use crate::db::queries::erc20::{Erc20TokenRow, TokenTransferRow};
use crate::db::queries::erc721::NftTransferRow;

/// ERC20 transfer response
#[derive(Debug, Serialize)]
pub struct TokenTransferDto {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub token_address: String,
    pub from_address: String,
    pub to_address: String,
    /// Amount as decimal string (for precision)
    pub amount: String,
    pub block_number: u64,
    pub block_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_decimals: Option<u8>,
}

impl From<TokenTransferRow> for TokenTransferDto {
    fn from(row: TokenTransferRow) -> Self {
        Self {
            id: row.id,
            transaction_hash: row.transaction_hash,
            log_index: row.log_index,
            token_address: row.token_address,
            from_address: row.from_address,
            to_address: row.to_address,
            amount: row.amount,
            block_number: row.block_number,
            block_time: row.block_time,
            token_symbol: row.token_symbol,
            token_decimals: row.token_decimals,
        }
    }
}

/// NFT transfer response
#[derive(Debug, Serialize)]
pub struct NftTransferDto {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub contract_address: String,
    pub from_address: String,
    pub to_address: String,
    pub token_id: String,
    pub block_number: u64,
    pub block_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_uri: Option<String>,
}

impl From<NftTransferRow> for NftTransferDto {
    fn from(row: NftTransferRow) -> Self {
        Self {
            id: row.id,
            transaction_hash: row.transaction_hash,
            log_index: row.log_index,
            contract_address: row.contract_address,
            from_address: row.from_address,
            to_address: row.to_address,
            token_id: row.token_id,
            block_number: row.block_number,
            block_time: row.block_time,
            collection_name: row.collection_name,
            token_uri: row.token_uri,
        }
    }
}

/// ERC20 token metadata response
#[derive(Debug, Serialize)]
pub struct Erc20TokenDto {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
}

impl From<Erc20TokenRow> for Erc20TokenDto {
    fn from(row: Erc20TokenRow) -> Self {
        Self {
            address: row.address,
            name: row.name,
            symbol: row.symbol,
            decimals: row.decimals,
        }
    }
}
