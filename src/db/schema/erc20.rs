//! ERC-20 schema - fungible token transfers tracking
//!
//! Tracks Transfer events for ERC-20 tokens only.
//! NFTs (ERC-721) are stored in separate nft_transfers table.

use super::TableSchema;

pub struct Erc20Schema;

impl TableSchema for Erc20Schema {
    fn module_name() -> &'static str {
        "erc20"
    }

    fn create_tables_sql() -> &'static str {
        r#"
-- ERC-20 Token Metadata
-- Stores token information (name, symbol, decimals) for known tokens
CREATE TABLE IF NOT EXISTS erc20_tokens (
    address String,
    name String,
    symbol String,
    decimals UInt8,
    network String,
    first_seen_block UInt64,
    created_at DateTime DEFAULT now()
) ENGINE = ReplacingMergeTree(created_at)
ORDER BY (network, address);

-- Wallet-Token interactions
-- Tracks which tokens a wallet has interacted with and when
-- Uses ReplacingMergeTree to keep only the latest interaction per wallet+token pair
CREATE TABLE IF NOT EXISTS wallet_tokens (
    wallet_address String,
    token_address String,
    last_interaction DateTime,
    network String
) ENGINE = ReplacingMergeTree(last_interaction)
ORDER BY (network, wallet_address, token_address);
-- TTL is applied from config.yaml via init-db or update-ttl command

-- ERC-20 Token Transfers
-- Tracks all Transfer(from, to, value) events for fungible tokens
CREATE TABLE IF NOT EXISTS token_transfers (
    id String,
    transaction_hash String,
    log_index UInt32,
    token_address String,
    from_address String,
    to_address String,
    amount String,  -- Token amount as string (for precision)
    block_number UInt64,
    block_time DateTime,
    network String,
    
    -- Optional: token metadata (can be filled later)
    token_symbol Nullable(String),
    token_decimals Nullable(UInt8)
) ENGINE = MergeTree()
ORDER BY (network, token_address, block_number, log_index)
PARTITION BY toYYYYMM(block_time);
-- TTL is applied from config.yaml via init-db or update-ttl command

-- Example queries:
-- Token transfers: SELECT * FROM token_transfers WHERE token_address = '0x...'
-- Wallet history: SELECT * FROM token_transfers WHERE from_address = '0x...' OR to_address = '0x...'
-- Token info: SELECT * FROM erc20_tokens WHERE address = '0x...'
-- Wallet tokens: SELECT * FROM wallet_tokens WHERE wallet_address = '0x...'
"#
    }

    fn drop_tables_sql() -> &'static str {
        "DROP TABLE IF EXISTS token_transfers; DROP TABLE IF EXISTS erc20_tokens; DROP TABLE IF EXISTS wallet_tokens"
    }

    fn table_names() -> &'static [&'static str] {
        &["token_transfers", "erc20_tokens", "wallet_tokens"]
    }
}

// ============================================================================
// Record types for database insertion
// ============================================================================

/// ERC-20 Token Transfer record
#[derive(Debug, Clone)]
pub struct TokenTransferRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub token_address: String,
    pub from_address: String,
    pub to_address: String,
    pub amount: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
    pub token_symbol: Option<String>,
    pub token_decimals: Option<u8>,
}

/// ERC-20 Token metadata record
#[derive(Debug, Clone)]
pub struct Erc20TokenRecord {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub network: String,
    pub first_seen_block: u64,
}

/// Wallet-Token interaction record
#[derive(Debug, Clone)]
pub struct WalletTokenRecord {
    pub wallet_address: String,
    pub token_address: String,
    pub last_interaction: String, // DateTime as string
    pub network: String,
}
