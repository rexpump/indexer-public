//! ERC-721 (NFT) schema - NFT transfers tracking
//!
//! Tracks Transfer events for non-fungible tokens.
//! Separate from ERC-20 for cleaner data model and different TTL.

use super::TableSchema;

pub struct Erc721Schema;

impl TableSchema for Erc721Schema {
    fn module_name() -> &'static str {
        "erc721"
    }

    fn create_tables_sql() -> &'static str {
        r#"
-- NFT Transfers (ERC-721)
-- Tracks all Transfer(from, to, tokenId) events for NFTs
CREATE TABLE IF NOT EXISTS nft_transfers (
    id String,
    transaction_hash String,
    log_index UInt32,
    contract_address String,
    from_address String,
    to_address String,
    token_id String,  -- NFT token ID
    block_number UInt64,
    block_time DateTime,
    network String,
    
    -- Optional: NFT metadata (can be filled later)
    collection_name Nullable(String),
    token_uri Nullable(String)
) ENGINE = MergeTree()
ORDER BY (network, contract_address, token_id, block_number)
PARTITION BY toYYYYMM(block_time);
-- TTL is applied from config.yaml via init-db or update-ttl command

-- Example queries:
-- All NFTs of contract: SELECT * FROM nft_transfers WHERE contract_address = '0x...'
-- NFT history: SELECT * FROM nft_transfers WHERE contract_address = '0x...' AND token_id = '123'
-- Wallet NFTs: SELECT * FROM nft_transfers WHERE to_address = '0x...'
"#
    }

    fn drop_tables_sql() -> &'static str {
        "DROP TABLE IF EXISTS nft_transfers"
    }

    fn table_names() -> &'static [&'static str] {
        &["nft_transfers"]
    }
}

// ============================================================================
// Record types for database insertion
// ============================================================================

/// NFT Transfer record (ERC-721)
#[derive(Debug, Clone)]
pub struct NftTransferRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub contract_address: String,
    pub from_address: String,
    pub to_address: String,
    pub token_id: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
    pub collection_name: Option<String>,
    pub token_uri: Option<String>,
}
