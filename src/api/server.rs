//! Axum HTTP server setup
//!
//! Configures and runs the API server with:
//! - CORS support
//! - Request tracing
//! - Graceful shutdown

use anyhow::{Context, Result};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::config::AppConfig;
use crate::db::ClickHouseClient;

use super::routes;
use super::state::AppState;

/// Run the API server
///
/// This function blocks until the server is shut down.
pub async fn run(config: AppConfig) -> Result<()> {
    // Connect to database
    let db = ClickHouseClient::new(&config.clickhouse)
        .await
        .context("Failed to connect to ClickHouse for API server")?;

    // Create shared state
    let state = AppState::new(db, config.clone());

    // Build CORS layer
    let cors = build_cors_layer(&config.api.cors_origins);

    // Build router
    let app = Router::new()
        .nest("/api", routes::build_routes())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Bind and serve
    let addr = config.api.socket_addr();
    info!("🚀 API server listening on http://{}", addr);
    info!("   Health check: http://{}/api/health", addr);
    info!("   Status:       http://{}/api/status", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {}", addr))?;

    axum::serve(listener, app)
        .await
        .context("API server error")?;

    Ok(())
}

/// Build CORS layer based on configuration
fn build_cors_layer(origins: &[String]) -> CorsLayer {
    if origins.is_empty() {
        // Allow all origins if none specified
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        // Parse allowed origins
        let allowed_origins: Vec<_> = origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();

        CorsLayer::new()
            .allow_origin(allowed_origins)
            .allow_methods(Any)
            .allow_headers(Any)
    }
}
