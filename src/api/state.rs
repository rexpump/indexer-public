//! Shared application state for API handlers
//!
//! Contains database client and configuration accessible to all route handlers.

use std::sync::Arc;

use crate::api::token_metadata::TokenMetadataFetcher;
use crate::config::AppConfig;
use crate::db::ClickHouseClient;

/// Shared state passed to all API handlers via Axum's State extractor
#[derive(Clone)]
pub struct AppState {
    /// ClickHouse database client
    pub db: Arc<ClickHouseClient>,
    /// Application configuration
    pub config: Arc<AppConfig>,
    /// Token metadata fetcher
    pub token_fetcher: Arc<TokenMetadataFetcher>,
}

impl AppState {
    /// Create new application state
    pub fn new(db: ClickHouseClient, config: AppConfig) -> Self {
        // If Scilla tracking is enabled, use Scilla API for native token metadata
        // Scilla API URL can be fallback_rpc_url or main rpc_url
        let token_fetcher = if config.indexer.track_scilla_tokens {
            let scilla_api = config.network.fallback_rpc_url
                .as_deref()
                .unwrap_or(&config.network.rpc_url);
            TokenMetadataFetcher::with_scilla_api(&config.network.rpc_url, scilla_api)
        } else {
            TokenMetadataFetcher::new(&config.network.rpc_url)
        };
        
        Self {
            db: Arc::new(db),
            config: Arc::new(config),
            token_fetcher: Arc::new(token_fetcher),
        }
    }
}
