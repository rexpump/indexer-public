//! API Routes
//!
//! Each submodule handles a specific domain:
//! - health: Health check and status endpoints
//! - erc20: ERC-20 token transfer endpoints
//! - nft: ERC-721 (NFT) transfer endpoints
//! - rexswap: DEX swap and pool endpoints
//! - rexpump: Memecoin launchpad endpoints

mod erc20;
mod health;
mod nft;
mod rexpump;
mod rexswap;

use axum::Router;

use super::state::AppState;

/// Build all API routes
pub fn build_routes() -> Router<AppState> {
    Router::new()
        // Health & status
        .merge(health::routes())
        // Token transfers
        .merge(erc20::routes())
        .merge(nft::routes())
        // DEX
        .merge(rexswap::routes())
        // Launchpad
        .merge(rexpump::routes())
}
