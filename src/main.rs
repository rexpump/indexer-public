mod api;
mod config;
mod db;
mod events;
mod handlers;
mod indexer;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser)]
#[command(name = "indexer")]
#[command(about = "Universal blockchain indexer for EVM chains", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Network to use (mainnet or testnet)
    #[arg(short, long, default_value = "testnet")]
    network: String,

    /// Path to config file
    #[arg(short, long, default_value = "config.yaml")]
    config: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the indexer
    Start {
        /// Start block (overrides config)
        #[arg(long)]
        from_block: Option<u64>,
        /// Also start API server
        #[arg(long)]
        with_api: bool,
        /// API server port (overrides config and env)
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Start only API server (no indexing)
    Serve {
        /// API server port (overrides config and env)
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Initialize database schema
    InitDb,
    /// Update TTL settings for all tables from config
    UpdateTtl,
    /// Drop all tables (WARNING: destructive!)
    DropDb {
        /// Confirm deletion
        #[arg(long)]
        confirm: bool,
    },
    /// Show indexer status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if exists
    dotenv::dotenv().ok();

    // Initialize logging
    FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .pretty()
        .init();

    let cli = Cli::parse();

    info!("Indexer starting...");
    info!("Network: {}", cli.network);
    info!("Config: {}", cli.config);

    // Load configuration
    let app_config = config::load_config(&cli.config, &cli.network)?;
    info!(
        "Loaded config for {} (chain_id: {})",
        app_config.network.name, app_config.network.chain_id
    );

    match cli.command {
        Commands::Start {
            from_block,
            with_api,
            port,
        } => {
            // Override port from CLI if provided
            let mut app_config = app_config;
            if let Some(p) = port {
                app_config.api.port = p;
            }

            // Start API if: --with-api flag OR api.enabled in config
            let start_api = with_api || app_config.api.enabled;

            if start_api {
                info!("Starting indexer with API server...");
                
                // Start API server in background
                let api_config = app_config.clone();
                let api_handle = tokio::spawn(async move {
                    if let Err(e) = api::run(api_config).await {
                        tracing::error!("API server error: {}", e);
                    }
                });

                // Run indexer
                indexer::run(app_config, from_block).await?;
                
                // Wait for API server (should not reach here normally)
                api_handle.abort();
            } else {
                info!("Starting indexer...");
                indexer::run(app_config, from_block).await?;
            }
        }
        Commands::Serve { port } => {
            // Override port from CLI if provided
            let mut app_config = app_config;
            if let Some(p) = port {
                app_config.api.port = p;
            }

            info!("Starting API server only...");
            api::run(app_config).await?;
        }
        Commands::InitDb => {
            info!("Initializing database schema...");
            let client = db::ClickHouseClient::new(&app_config.clickhouse).await?;
            client.init_schema().await?;
            // Apply TTL settings from config
            info!("Applying TTL settings from config...");
            client.update_ttl(&app_config.indexer.ttl).await?;
            info!("Database schema initialized successfully!");
        }
        Commands::UpdateTtl => {
            info!("Updating TTL settings from config...");
            let ttl = &app_config.indexer.ttl;
            println!("\n=== TTL Configuration (days, 0 = forever) ===");
            println!("ERC Tokens:");
            println!("  erc20_transfers:    {}", ttl.erc20_transfers);
            println!("  erc721_transfers:   {}", ttl.erc721_transfers);
            println!("RexSwap DEX:");
            println!("  rexswap_swaps:      {}", ttl.rexswap_swaps);
            println!("  rexswap_pools:      {}", ttl.rexswap_pools);
            println!("  rexswap_liquidity:  {}", ttl.rexswap_liquidity);
            println!("RexPump Launchpad:");
            println!("  rexpump_swaps:      {}", ttl.rexpump_swaps);
            println!("  rexpump_pools:      {}", ttl.rexpump_pools);
            println!();
            
            let client = db::ClickHouseClient::new(&app_config.clickhouse).await?;
            client.update_ttl(&app_config.indexer.ttl).await?;
            info!("TTL settings updated successfully!");
        }
        Commands::DropDb { confirm } => {
            if !confirm {
                eprintln!("ERROR: To drop all tables, run with --confirm flag");
                std::process::exit(1);
            }
            info!("Dropping all tables...");
            let client = db::ClickHouseClient::new(&app_config.clickhouse).await?;
            client.drop_all_tables().await?;
            info!("All tables dropped!");
        }
        Commands::Status => {
            use alloy::providers::{Provider, ProviderBuilder};
            
            let client = db::ClickHouseClient::new(&app_config.clickhouse).await?;
            let status = client.get_indexer_status(&app_config.network.name).await?;
            
            // Try to get chain head from RPC
            let chain_head = match app_config.network.rpc_url.parse() {
                Ok(url) => {
                    let provider = ProviderBuilder::new().on_http(url);
                    provider.get_block_number().await.ok()
                }
                Err(_) => None,
            };
            
            println!("\n╔════════════════════════════════════════╗");
            println!("║          INDEXER STATUS                ║");
            println!("╚════════════════════════════════════════╝");
            
            // Network info
            println!("\n📡 Network");
            println!("   Name:          {}", app_config.network.name);
            println!("   Chain ID:      {}", app_config.network.chain_id);
            println!("   Start block:   {}", app_config.network.start_block);
            
            // Sync status
            println!("\n🔄 Sync Status");
            println!("   Last synced:   {}", format_number(status.last_synced_block));
            
            if let Some(head) = chain_head {
                let confirmations = app_config.indexer.confirmations;
                let safe_head = head.saturating_sub(confirmations);
                let blocks_behind = safe_head.saturating_sub(status.last_synced_block);
                
                println!("   Chain head:    {}", format_number(head));
                println!("   Safe head:     {} ({} confirmations)", format_number(safe_head), confirmations);
                
                if blocks_behind == 0 {
                    println!("   Status:        ✅ SYNCED (live mode)");
                } else if blocks_behind < 100 {
                    println!("   Status:        🟡 CATCHING UP ({} blocks behind)", blocks_behind);
                } else {
                    let progress = if safe_head > app_config.network.start_block {
                        let total = safe_head - app_config.network.start_block;
                        let done = status.last_synced_block.saturating_sub(app_config.network.start_block);
                        (done as f64 / total as f64) * 100.0
                    } else {
                        0.0
                    };
                    println!("   Status:        🔴 SYNCING ({} blocks behind, {:.1}%)", 
                        format_number(blocks_behind), progress);
                }
            } else {
                println!("   Chain head:    ⚠️  RPC unavailable");
                println!("   Status:        ❓ Unknown (cannot reach RPC)");
            }
            
            // Records count
            println!("\n📊 Records");
            println!("   RexSwap swaps:       {}", format_number(status.swaps_count));
            println!("   RexSwap pools:       {}", format_number(status.pools_count));
            println!("   RexSwap liquidity:   {}", format_number(status.liquidity_changes_count));
            println!("   ERC-20 transfers:    {}", format_number(status.token_transfers_count));
            println!("   ERC-721 transfers:   {}", format_number(status.nft_transfers_count));
            println!("   RexPump swaps:       {}", format_number(status.rexpump_swaps_count));
            
            // Active modules
            println!("\n⚙️  Active Modules");
            let idx = &app_config.indexer;
            
            // RexSwap DEX tracking
            let dex_addr = &app_config.contracts.rexswap_dex;
            let has_dex_addr = !dex_addr.is_empty() 
                && dex_addr != "0x0000000000000000000000000000000000000000";
            let rexswap_active = idx.track_rexswap && has_dex_addr;
            println!("   {} RexSwap DEX (calldata)", if rexswap_active { "✅" } else { "⬚ " });
            if rexswap_active {
                println!("      └─ {}", dex_addr);
            } else if idx.track_rexswap && !has_dex_addr {
                println!("      └─ ⚠️  No DEX address configured");
            }
            
            println!("   {} RexPump tracking", if idx.track_rexpump { "✅" } else { "⬚ " });
            println!("   {} ERC-20 tracking {}", 
                if idx.track_erc20 || idx.track_all_erc20 { "✅" } else { "⬚ " },
                if idx.track_all_erc20 { "(ALL tokens)" } else { "" });
            println!("   {} ERC-721 tracking {}", 
                if idx.track_erc721 || idx.track_all_erc721 { "✅" } else { "⬚ " },
                if idx.track_all_erc721 { "(ALL NFTs)" } else { "" });
            
            // TTL config
            let ttl = &app_config.indexer.ttl;
            println!("\n🗑️  TTL Settings (days, 0 = forever)");
            println!("   ERC-20 transfers:    {}", format_ttl(ttl.erc20_transfers));
            println!("   ERC-721 transfers:   {}", format_ttl(ttl.erc721_transfers));
            println!("   RexSwap swaps:       {}", format_ttl(ttl.rexswap_swaps));
            println!("   RexSwap pools:       {}", format_ttl(ttl.rexswap_pools));
            println!("   RexSwap liquidity:   {}", format_ttl(ttl.rexswap_liquidity));
            println!("   RexPump swaps:       {}", format_ttl(ttl.rexpump_swaps));
            println!("   RexPump pools:       {}", format_ttl(ttl.rexpump_pools));
            
            println!();
        }
    }

    Ok(())
}

/// Format number with thousand separators
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format TTL value
fn format_ttl(days: u32) -> String {
    if days == 0 {
        "∞ (forever)".to_string()
    } else {
        format!("{} days", days)
    }
}
