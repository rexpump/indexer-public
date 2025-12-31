//! API Server Module
//!
//! Provides HTTP API for querying indexed blockchain data.
//!
//! # Architecture
//!
//! ```text
//! api/
//! ├── mod.rs          - Public interface (this file)
//! ├── server.rs       - Axum server setup
//! ├── error.rs        - API error types
//! ├── state.rs        - Shared application state
//! ├── dto/            - Data Transfer Objects (response types)
//! │   ├── mod.rs
//! │   └── common.rs   - Pagination, ListResponse
//! └── routes/         - Route handlers by domain
//!     ├── mod.rs
//!     └── health.rs   - Health check endpoint
//! ```

mod error;
mod routes;
mod server;
mod state;
pub mod token_metadata;

pub mod dto;

pub use error::ApiError;
pub use server::run;
pub use state::AppState;
pub use token_metadata::TokenMetadataFetcher;
