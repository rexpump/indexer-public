//! Core schema - indexer state tracking
//!
//! This table is shared by all modules to track sync progress.

use super::TableSchema;

pub struct CoreSchema;

impl TableSchema for CoreSchema {
    fn module_name() -> &'static str {
        "core"
    }

    fn create_tables_sql() -> &'static str {
        r#"
-- Indexer state tracking table
-- Tracks last synced block per network
CREATE TABLE IF NOT EXISTS indexer_state (
    network String,
    last_synced_block UInt64,
    updated_at DateTime DEFAULT now()
) ENGINE = ReplacingMergeTree(updated_at)
ORDER BY network
"#
    }

    fn drop_tables_sql() -> &'static str {
        "DROP TABLE IF EXISTS indexer_state"
    }

    fn table_names() -> &'static [&'static str] {
        &["indexer_state"]
    }
}
