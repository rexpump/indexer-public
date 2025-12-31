//! Health check and status endpoints
//!
//! - GET /health - Simple health check
//! - GET /status - Detailed indexer status

use axum::{extract::State, routing::get, Json, Router};

use crate::api::dto::{HealthResponse, StatusCounts, StatusResponse};
use crate::api::error::ApiError;
use crate::api::state::AppState;

/// Build health routes
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_check))
        .route("/status", get(indexer_status))
}

/// GET /health
///
/// Simple health check endpoint. Returns 200 if the server is running.
async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        network: state.config.network.name.clone(),
    })
}

/// GET /status
///
/// Returns detailed indexer status including sync state and record counts.
async fn indexer_status(State(state): State<AppState>) -> Result<Json<StatusResponse>, ApiError> {
    let status = state
        .db
        .get_indexer_status(&state.config.network.name)
        .await?;

    Ok(Json(StatusResponse {
        network: state.config.network.name.clone(),
        chain_id: state.config.network.chain_id,
        last_synced_block: status.last_synced_block,
        counts: StatusCounts {
            swaps: status.swaps_count,
            pools: status.pools_count,
            liquidity_changes: status.liquidity_changes_count,
            token_transfers: status.token_transfers_count,
            nft_transfers: status.nft_transfers_count,
            rexpump_swaps: status.rexpump_swaps_count,
        },
    }))
}
