//! RexPump memecoin launchpad endpoints
//!
//! Tokens:
//! - GET /api/rexpump/tokens                         - List all tokens
//! - GET /api/rexpump/tokens/trending                - Get trending tokens (24h activity)
//! - GET /api/rexpump/tokens/:pool_id                - Get token details
//! - GET /api/rexpump/tokens/:pool_id/swaps          - Get token swaps
//! - GET /api/rexpump/tokens/:pool_id/chart          - Get price history for charts
//! - GET /api/rexpump/tokens/:pool_id/candles        - Get OHLCV candles
//!
//! User:
//! - GET /api/rexpump/user/:address/created          - Get tokens created by user
//! - GET /api/rexpump/user/:address/swaps            - Get user's swaps

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::api::dto::{
    CandleDto, ListResponse, PaginationParams, PricePointDto, RexPumpSwapDto, RexPumpTokenDto,
    TokenDetailDto, TokenStatsDto, TrendingTokenDto,
};
use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::queries::rexpump::RexPumpQueries;

/// Build RexPump routes
pub fn routes() -> Router<AppState> {
    Router::new()
        // Tokens
        .route("/rexpump/tokens", get(get_tokens))
        .route("/rexpump/tokens/trending", get(get_trending))
        .route("/rexpump/tokens/{pool_id}", get(get_token))
        .route("/rexpump/tokens/{pool_id}/swaps", get(get_token_swaps))
        .route("/rexpump/tokens/{pool_id}/chart", get(get_price_history))
        .route("/rexpump/tokens/{pool_id}/candles", get(get_candles))
        // User
        .route("/rexpump/user/{address}/created", get(get_user_created))
        .route("/rexpump/user/{address}/swaps", get(get_user_swaps))
}

// ============================================================================
// Token endpoints
// ============================================================================

/// GET /api/rexpump/tokens
///
/// Get all memecoins with pagination.
async fn get_tokens(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<RexPumpTokenDto>>, ApiError> {
    let pagination = pagination.validate();

    let (tokens, total) = RexPumpQueries::get_tokens(
        &state.db,
        &state.config.network.name,
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let items: Vec<RexPumpTokenDto> = tokens.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}

/// Query params for trending
#[derive(Debug, Deserialize)]
pub struct TrendingQuery {
    #[serde(default = "default_trending_limit")]
    pub limit: u32,
}

fn default_trending_limit() -> u32 {
    20
}

/// GET /api/rexpump/tokens/trending
///
/// Get trending tokens by 24h activity.
async fn get_trending(
    State(state): State<AppState>,
    Query(query): Query<TrendingQuery>,
) -> Result<Json<Vec<TrendingTokenDto>>, ApiError> {
    let limit = query.limit.min(100).max(1);

    let tokens =
        RexPumpQueries::get_trending_tokens(&state.db, &state.config.network.name, limit).await?;

    let items: Vec<TrendingTokenDto> = tokens.into_iter().map(Into::into).collect();

    Ok(Json(items))
}

/// GET /api/rexpump/tokens/:pool_id
///
/// Get token details with statistics.
async fn get_token(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
) -> Result<Json<TokenDetailDto>, ApiError> {
    let token = RexPumpQueries::get_token(&state.db, &state.config.network.name, &pool_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Token not found: {}", pool_id)))?;

    let stats = RexPumpQueries::get_token_stats(&state.db, &state.config.network.name, &pool_id)
        .await?
        .map(TokenStatsDto::from);

    Ok(Json(TokenDetailDto {
        token: token.into(),
        stats,
    }))
}

/// GET /api/rexpump/tokens/:pool_id/swaps
///
/// Get swaps for a specific token.
async fn get_token_swaps(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<RexPumpSwapDto>>, ApiError> {
    let pagination = pagination.validate();

    let (swaps, total) = RexPumpQueries::get_token_swaps(
        &state.db,
        &state.config.network.name,
        &pool_id,
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let items: Vec<RexPumpSwapDto> = swaps.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}

/// Query params for chart
#[derive(Debug, Deserialize)]
pub struct ChartQuery {
    /// Number of data points (default: 100, max: 1000)
    #[serde(default = "default_chart_limit")]
    pub limit: u32,
}

fn default_chart_limit() -> u32 {
    100
}

/// GET /api/rexpump/tokens/:pool_id/chart
///
/// Get price history for charting.
async fn get_price_history(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
    Query(query): Query<ChartQuery>,
) -> Result<Json<Vec<PricePointDto>>, ApiError> {
    let limit = query.limit.min(1000).max(1);

    let history =
        RexPumpQueries::get_price_history(&state.db, &state.config.network.name, &pool_id, limit)
            .await?;

    let items: Vec<PricePointDto> = history.into_iter().map(Into::into).collect();

    Ok(Json(items))
}

/// Query params for candles
#[derive(Debug, Deserialize)]
pub struct CandlesQuery {
    /// Candle interval in minutes (default: 60 = 1 hour)
    #[serde(default = "default_candle_interval")]
    pub interval: u32,
    /// Number of candles (default: 100, max: 500)
    #[serde(default = "default_chart_limit")]
    pub limit: u32,
}

fn default_candle_interval() -> u32 {
    60
}

/// GET /api/rexpump/tokens/:pool_id/candles
///
/// Get OHLCV candles for charting.
async fn get_candles(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
    Query(query): Query<CandlesQuery>,
) -> Result<Json<Vec<CandleDto>>, ApiError> {
    // Validate interval (1min, 5min, 15min, 30min, 1hour, 4hour, 1day)
    let valid_intervals = [1, 5, 15, 30, 60, 240, 1440];
    let interval = if valid_intervals.contains(&query.interval) {
        query.interval
    } else {
        60 // default to 1 hour
    };
    let limit = query.limit.min(500).max(1);

    let candles = RexPumpQueries::get_ohlcv(
        &state.db,
        &state.config.network.name,
        &pool_id,
        interval,
        limit,
    )
    .await?;

    let items: Vec<CandleDto> = candles.into_iter().map(Into::into).collect();

    Ok(Json(items))
}

// ============================================================================
// User endpoints
// ============================================================================

/// GET /api/rexpump/user/:address/created
///
/// Get tokens created by a user.
async fn get_user_created(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Result<Json<Vec<RexPumpTokenDto>>, ApiError> {
    let tokens =
        RexPumpQueries::get_user_created_tokens(&state.db, &state.config.network.name, &address)
            .await?;

    let items: Vec<RexPumpTokenDto> = tokens.into_iter().map(Into::into).collect();

    Ok(Json(items))
}

/// GET /api/rexpump/user/:address/swaps
///
/// Get user's swaps across all tokens.
async fn get_user_swaps(
    State(state): State<AppState>,
    Path(address): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<RexPumpSwapDto>>, ApiError> {
    let pagination = pagination.validate();

    let (swaps, total) = RexPumpQueries::get_user_swaps(
        &state.db,
        &state.config.network.name,
        &address,
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let items: Vec<RexPumpSwapDto> = swaps.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}
