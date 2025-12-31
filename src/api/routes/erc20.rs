//! ERC-20 token transfer endpoints
//!
//! - GET /api/erc20/transfers          - Get transfers (filter by wallet, token)
//! - GET /api/erc20/tokens/:token/transfers - Get all transfers for a token
//! - GET /api/erc20/wallet/:address/tokens  - Get tokens a wallet interacted with (with metadata)
//! - GET /api/erc20/token/:address     - Get token metadata

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::api::dto::{Erc20TokenDto, ListResponse, PaginationParams, TokenTransferDto};
use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::api::token_metadata;
use crate::db::queries::erc20::{Erc20Queries, TransferQuery};

/// Build ERC-20 routes
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/erc20/transfers", get(get_transfers))
        .route("/erc20/tokens/{token}/transfers", get(get_token_transfers))
        .route("/erc20/wallet/{address}/tokens", get(get_wallet_tokens))
        .route("/erc20/token/{address}", get(get_token_info))
}

/// Query parameters for transfers endpoint
#[derive(Debug, Deserialize)]
pub struct TransfersQuery {
    /// Filter by wallet address (from or to)
    pub wallet: Option<String>,
    /// Filter by token address
    pub token: Option<String>,
    /// Pagination
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

/// GET /api/erc20/transfers
///
/// Get ERC-20 token transfers with optional filters.
///
/// Query parameters:
/// - `wallet` - Filter by wallet address (matches from or to)
/// - `token` - Filter by token contract address
/// - `limit` - Max items per page (default: 50, max: 1000)
/// - `offset` - Items to skip
async fn get_transfers(
    State(state): State<AppState>,
    Query(query): Query<TransfersQuery>,
) -> Result<Json<ListResponse<TokenTransferDto>>, ApiError> {
    let pagination = query.pagination.validate();
    
    let db_query = TransferQuery {
        wallet: query.wallet,
        token: query.token,
        limit: pagination.limit,
        offset: pagination.offset,
    };

    let (transfers, total) = Erc20Queries::get_transfers(
        &state.db,
        &state.config.network.name,
        &db_query,
    )
    .await?;

    let items: Vec<TokenTransferDto> = transfers.into_iter().map(Into::into).collect();
    
    Ok(Json(ListResponse::new(items, total, &pagination)))
}

/// GET /api/erc20/tokens/:token/transfers
///
/// Get all transfers for a specific token.
async fn get_token_transfers(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<TokenTransferDto>>, ApiError> {
    let pagination = pagination.validate();

    let (transfers, total) = Erc20Queries::get_token_transfers(
        &state.db,
        &state.config.network.name,
        &token,
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let items: Vec<TokenTransferDto> = transfers.into_iter().map(Into::into).collect();
    
    Ok(Json(ListResponse::new(items, total, &pagination)))
}

/// GET /api/erc20/wallet/:address/tokens
///
/// Get list of tokens a wallet has interacted with (with metadata).
/// Metadata is fetched from RPC if not cached.
async fn get_wallet_tokens(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Result<Json<Vec<Erc20TokenDto>>, ApiError> {
    // First get token addresses
    let token_addresses = Erc20Queries::get_wallet_tokens(
        &state.db,
        &state.config.network.name,
        &address,
    )
    .await?;

    // Get metadata for each token (fetching from RPC if needed)
    let tokens = token_metadata::get_or_fetch_tokens(
        &state.db,
        &state.token_fetcher,
        &state.config.network.name,
        &token_addresses,
    )
    .await;

    let dto: Vec<Erc20TokenDto> = tokens.into_iter().map(Into::into).collect();
    Ok(Json(dto))
}

/// GET /api/erc20/token/:address
///
/// Get token metadata by address.
/// Fetches from RPC if not cached.
async fn get_token_info(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Result<Json<Erc20TokenDto>, ApiError> {
    let token = token_metadata::get_or_fetch_token(
        &state.db,
        &state.token_fetcher,
        &state.config.network.name,
        &address,
    )
    .await?;

    Ok(Json(token.into()))
}
