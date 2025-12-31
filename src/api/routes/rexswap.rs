//! RexSwap DEX endpoints
//!
//! Pools:
//! - GET /api/rexswap/pools                    - List all pools
//! - GET /api/rexswap/pools/:pool_id           - Get pool details
//! - GET /api/rexswap/pools/search             - Search pools by token
//! - GET /api/rexswap/pools/:pool_id/swaps     - Get swaps in a pool
//! - GET /api/rexswap/pools/:pool_id/liquidity - Get liquidity changes in a pool
//!
//! Swaps:
//! - GET /api/rexswap/swaps                    - Get all swaps (paginated)
//!
//! User:
//! - GET /api/rexswap/user/:address/swaps      - Get user's swaps
//! - GET /api/rexswap/user/:address/positions  - Get user's LP positions

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::api::dto::{
    LiquidityChangeDto, ListResponse, PaginationParams, PoolDto, SwapDto, UserPositionDto,
};
use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::queries::rexswap::RexSwapQueries;

/// Build RexSwap routes
pub fn routes() -> Router<AppState> {
    Router::new()
        // Pools
        .route("/rexswap/pools", get(get_pools))
        .route("/rexswap/pools/search", get(search_pools))
        .route("/rexswap/pools/{pool_id}", get(get_pool))
        .route("/rexswap/pools/{pool_id}/swaps", get(get_pool_swaps))
        .route("/rexswap/pools/{pool_id}/liquidity", get(get_pool_liquidity))
        // Swaps
        .route("/rexswap/swaps", get(get_swaps))
        // User
        .route("/rexswap/user/{address}/swaps", get(get_user_swaps))
        .route("/rexswap/user/{address}/positions", get(get_user_positions))
}

// ============================================================================
// Pool endpoints
// ============================================================================

/// GET /api/rexswap/pools
///
/// Get all liquidity pools with pagination.
async fn get_pools(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<PoolDto>>, ApiError> {
    let pagination = pagination.validate();

    let (pools, total) = RexSwapQueries::get_pools(
        &state.db,
        &state.config.network.name,
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let items: Vec<PoolDto> = pools.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}

/// Search query params
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// Token address to search for (in base or quote)
    pub token: String,
}

/// GET /api/rexswap/pools/search
///
/// Search pools by token address.
async fn search_pools(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<PoolDto>>, ApiError> {
    let pools =
        RexSwapQueries::search_pools(&state.db, &state.config.network.name, &query.token).await?;

    let items: Vec<PoolDto> = pools.into_iter().map(Into::into).collect();

    Ok(Json(items))
}

/// GET /api/rexswap/pools/:pool_id
///
/// Get details of a specific pool.
async fn get_pool(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
) -> Result<Json<PoolDto>, ApiError> {
    let pool = RexSwapQueries::get_pool(&state.db, &state.config.network.name, &pool_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Pool not found: {}", pool_id)))?;

    Ok(Json(pool.into()))
}

/// GET /api/rexswap/pools/:pool_id/swaps
///
/// Get swaps for a specific pool.
async fn get_pool_swaps(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<SwapDto>>, ApiError> {
    let pagination = pagination.validate();

    let (swaps, total) = RexSwapQueries::get_swaps(
        &state.db,
        &state.config.network.name,
        Some(&pool_id),
        None,
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let items: Vec<SwapDto> = swaps.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}

/// GET /api/rexswap/pools/:pool_id/liquidity
///
/// Get liquidity changes for a specific pool.
async fn get_pool_liquidity(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<LiquidityChangeDto>>, ApiError> {
    let pagination = pagination.validate();

    let (changes, total) = RexSwapQueries::get_liquidity_changes(
        &state.db,
        &state.config.network.name,
        Some(&pool_id),
        None,
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let items: Vec<LiquidityChangeDto> = changes.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}

// ============================================================================
// Swap endpoints
// ============================================================================

/// GET /api/rexswap/swaps
///
/// Get all swaps with pagination.
async fn get_swaps(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<SwapDto>>, ApiError> {
    let pagination = pagination.validate();

    let (swaps, total) = RexSwapQueries::get_swaps(
        &state.db,
        &state.config.network.name,
        None,
        None,
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let items: Vec<SwapDto> = swaps.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}

// ============================================================================
// User endpoints
// ============================================================================

/// GET /api/rexswap/user/:address/swaps
///
/// Get all swaps for a user.
async fn get_user_swaps(
    State(state): State<AppState>,
    Path(address): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<SwapDto>>, ApiError> {
    let pagination = pagination.validate();

    let (swaps, total) = RexSwapQueries::get_swaps(
        &state.db,
        &state.config.network.name,
        None,
        Some(&address),
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let items: Vec<SwapDto> = swaps.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}

/// GET /api/rexswap/user/:address/positions
///
/// Get user's LP positions (aggregated by pool).
async fn get_user_positions(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Result<Json<Vec<UserPositionDto>>, ApiError> {
    let positions =
        RexSwapQueries::get_user_positions(&state.db, &state.config.network.name, &address).await?;

    let items: Vec<UserPositionDto> = positions.into_iter().map(Into::into).collect();

    Ok(Json(items))
}
