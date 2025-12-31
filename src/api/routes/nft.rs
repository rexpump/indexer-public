//! ERC-721 (NFT) transfer endpoints
//!
//! - GET /api/nft/transfers                        - Get transfers (filter by wallet, contract)
//! - GET /api/nft/collections/:contract/transfers  - Get all transfers for a collection
//! - GET /api/nft/collections/:contract/:token_id/history - Get history of a specific NFT
//! - GET /api/nft/wallet/:address/collections      - Get collections a wallet interacted with

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::api::dto::{ListResponse, NftTransferDto, PaginationParams};
use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::db::queries::erc721::{Erc721Queries, NftTransferQuery};

/// Build NFT routes
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/nft/transfers", get(get_transfers))
        .route(
            "/nft/collections/{contract}/transfers",
            get(get_collection_transfers),
        )
        .route(
            "/nft/collections/{contract}/{token_id}/history",
            get(get_nft_history),
        )
        .route("/nft/wallet/{address}/collections", get(get_wallet_collections))
}

/// Query parameters for NFT transfers endpoint
#[derive(Debug, Deserialize)]
pub struct NftTransfersQuery {
    /// Filter by wallet address (from or to)
    pub wallet: Option<String>,
    /// Filter by contract address
    pub contract: Option<String>,
    /// Pagination
    #[serde(flatten)]
    pub pagination: PaginationParams,
}

/// GET /api/nft/transfers
///
/// Get NFT transfers with optional filters.
///
/// Query parameters:
/// - `wallet` - Filter by wallet address (matches from or to)
/// - `contract` - Filter by NFT contract address
/// - `limit` - Max items per page (default: 50, max: 1000)
/// - `offset` - Items to skip
async fn get_transfers(
    State(state): State<AppState>,
    Query(query): Query<NftTransfersQuery>,
) -> Result<Json<ListResponse<NftTransferDto>>, ApiError> {
    let pagination = query.pagination.validate();

    let db_query = NftTransferQuery {
        wallet: query.wallet,
        contract: query.contract,
        token_id: None,
        limit: pagination.limit,
        offset: pagination.offset,
    };

    let (transfers, total) =
        Erc721Queries::get_transfers(&state.db, &state.config.network.name, &db_query).await?;

    let items: Vec<NftTransferDto> = transfers.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}

/// GET /api/nft/collections/:contract/transfers
///
/// Get all transfers for a specific NFT collection.
async fn get_collection_transfers(
    State(state): State<AppState>,
    Path(contract): Path<String>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ListResponse<NftTransferDto>>, ApiError> {
    let pagination = pagination.validate();

    let db_query = NftTransferQuery {
        wallet: None,
        contract: Some(contract),
        token_id: None,
        limit: pagination.limit,
        offset: pagination.offset,
    };

    let (transfers, total) =
        Erc721Queries::get_transfers(&state.db, &state.config.network.name, &db_query).await?;

    let items: Vec<NftTransferDto> = transfers.into_iter().map(Into::into).collect();

    Ok(Json(ListResponse::new(items, total, &pagination)))
}

/// Path parameters for NFT history
#[derive(Debug, Deserialize)]
pub struct NftPath {
    pub contract: String,
    pub token_id: String,
}

/// GET /api/nft/collections/:contract/:token_id/history
///
/// Get the full ownership history of a specific NFT.
async fn get_nft_history(
    State(state): State<AppState>,
    Path(path): Path<NftPath>,
) -> Result<Json<Vec<NftTransferDto>>, ApiError> {
    let transfers = Erc721Queries::get_nft_history(
        &state.db,
        &state.config.network.name,
        &path.contract,
        &path.token_id,
    )
    .await?;

    let items: Vec<NftTransferDto> = transfers.into_iter().map(Into::into).collect();

    Ok(Json(items))
}

/// GET /api/nft/wallet/:address/collections
///
/// Get list of NFT collections a wallet has interacted with.
async fn get_wallet_collections(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Result<Json<Vec<String>>, ApiError> {
    let collections =
        Erc721Queries::get_wallet_collections(&state.db, &state.config.network.name, &address)
            .await?;

    Ok(Json(collections))
}
