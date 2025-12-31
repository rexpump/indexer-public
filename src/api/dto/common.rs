//! Common DTO types used across all API endpoints

use serde::{Deserialize, Serialize};

/// Pagination parameters for list endpoints
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationParams {
    /// Maximum number of items to return (default: 50, max: 1000)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Number of items to skip (default: 0)
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 {
    50
}

impl PaginationParams {
    /// Validate and clamp pagination parameters
    pub fn validate(&self) -> Self {
        Self {
            limit: self.limit.min(1000).max(1),
            offset: self.offset,
        }
    }
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            offset: 0,
        }
    }
}

/// Generic paginated list response
#[derive(Debug, Serialize)]
pub struct ListResponse<T> {
    /// Items in current page
    pub items: Vec<T>,
    /// Pagination metadata
    pub pagination: PaginationMeta,
}

/// Pagination metadata in response
#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    /// Total number of items matching the query
    pub total: u64,
    /// Current limit
    pub limit: u32,
    /// Current offset
    pub offset: u32,
    /// Whether there are more items after this page
    pub has_more: bool,
}

impl<T> ListResponse<T> {
    /// Create a new paginated response
    pub fn new(items: Vec<T>, total: u64, params: &PaginationParams) -> Self {
        let has_more = (params.offset as u64 + items.len() as u64) < total;
        Self {
            items,
            pagination: PaginationMeta {
                total,
                limit: params.limit,
                offset: params.offset,
                has_more,
            },
        }
    }
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub network: String,
}

/// Indexer status response
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub network: String,
    pub chain_id: u64,
    pub last_synced_block: u64,
    pub counts: StatusCounts,
}

#[derive(Debug, Serialize)]
pub struct StatusCounts {
    pub swaps: u64,
    pub pools: u64,
    pub liquidity_changes: u64,
    pub token_transfers: u64,
    pub nft_transfers: u64,
    pub rexpump_swaps: u64,
}
