//! Modular database schema definitions
//!
//! Each module (rexswap, erc20, erc721, rexpump, etc.) defines its own tables.
//! This allows adding new tracking types by simply adding a new schema module.

mod core;
pub mod erc20;
mod erc721;
mod rexpump;
mod rexswap;

pub use core::*;
pub use erc20::*;
pub use erc721::*;
pub use rexpump::*;
pub use rexswap::*;

/// Trait for modular schema definitions
pub trait TableSchema {
    /// Module name for logging
    fn module_name() -> &'static str;

    /// SQL statements to create tables (separated by semicolons)
    fn create_tables_sql() -> &'static str;

    /// SQL statements to drop tables
    fn drop_tables_sql() -> &'static str;

    /// List of table names managed by this module
    fn table_names() -> &'static [&'static str];
}

/// Collect all CREATE TABLE statements from all modules
pub fn collect_create_schemas() -> Vec<(&'static str, &'static str)> {
    vec![
        (CoreSchema::module_name(), CoreSchema::create_tables_sql()),
        (
            RexSwapSchema::module_name(),
            RexSwapSchema::create_tables_sql(),
        ),
        (Erc20Schema::module_name(), Erc20Schema::create_tables_sql()),
        (
            Erc721Schema::module_name(),
            Erc721Schema::create_tables_sql(),
        ),
        (
            RexPumpSchema::module_name(),
            RexPumpSchema::create_tables_sql(),
        ),
    ]
}

/// Collect all DROP TABLE statements from all modules
pub fn collect_drop_schemas() -> Vec<(&'static str, &'static str)> {
    vec![
        (CoreSchema::module_name(), CoreSchema::drop_tables_sql()),
        (
            RexSwapSchema::module_name(),
            RexSwapSchema::drop_tables_sql(),
        ),
        (Erc20Schema::module_name(), Erc20Schema::drop_tables_sql()),
        (
            Erc721Schema::module_name(),
            Erc721Schema::drop_tables_sql(),
        ),
        (
            RexPumpSchema::module_name(),
            RexPumpSchema::drop_tables_sql(),
        ),
    ]
}

/// Indexer status (used by client)
#[derive(Debug, Clone, Default)]
pub struct IndexerStatus {
    pub last_synced_block: u64,
    pub swaps_count: u64,
    pub pools_count: u64,
    pub liquidity_changes_count: u64,
    pub token_transfers_count: u64,
    pub nft_transfers_count: u64,
    pub rexpump_swaps_count: u64,
}
