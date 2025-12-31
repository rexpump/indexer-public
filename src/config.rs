//! Configuration loading and management

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

/// Main application configuration (final resolved config, not directly deserialized)
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub network: NetworkConfig,
    pub clickhouse: ClickHouseConfig,
    pub contracts: ContractsConfig,
    pub indexer: IndexerConfig,
    pub api: ApiConfig,
}

// ============================================================================
// API Server Configuration
// ============================================================================

/// API server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    /// Enable API server (default: false)
    #[serde(default)]
    pub enabled: bool,
    /// Host to bind to (default: 0.0.0.0)
    #[serde(default = "default_api_host")]
    pub host: String,
    /// Port to listen on (default: 8080)
    #[serde(default = "default_api_port")]
    pub port: u16,
    /// CORS allowed origins (empty = allow all)
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_api_host(),
            port: default_api_port(),
            cors_origins: Vec::new(),
        }
    }
}

impl ApiConfig {
    /// Get the socket address for binding
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn default_api_host() -> String {
    "0.0.0.0".to_string()
}

fn default_api_port() -> u16 {
    8080
}

/// Network-specific configuration (for parsing from YAML)
#[derive(Debug, Clone, Deserialize)]
struct NetworkConfigFile {
    name: String,
    chain_id: u64,
    rpc_url: String,
    #[serde(default)]
    fallback_rpc_url: Option<String>,
    start_block: u64,
    #[serde(default)]
    end_block: Option<u64>,
    /// Per-network contract addresses
    #[serde(default)]
    contracts: Option<ContractsConfigFile>,
    /// Per-network indexer settings (overrides global defaults)
    #[serde(default)]
    indexer: Option<NetworkIndexerConfig>,
    /// Per-network API settings (overrides global defaults)
    #[serde(default)]
    api: Option<ApiConfig>,
}

/// Per-network indexer settings (all optional, falls back to global defaults)
#[derive(Debug, Clone, Deserialize, Default)]
struct NetworkIndexerConfig {
    #[serde(default)]
    batch_size: Option<u64>,
    #[serde(default)]
    batch_delay_ms: Option<u64>,
    #[serde(default)]
    poll_interval_ms: Option<u64>,
    #[serde(default)]
    confirmations: Option<u64>,
    #[serde(default)]
    live_indexing: Option<bool>,
    #[serde(default)]
    track_rexswap: Option<bool>,
    #[serde(default)]
    track_erc20: Option<bool>,
    #[serde(default)]
    track_erc721: Option<bool>,
    #[serde(default)]
    track_rexpump: Option<bool>,
    #[serde(default)]
    track_all_erc20: Option<bool>,
    #[serde(default)]
    track_all_erc721: Option<bool>,
    #[serde(default)]
    track_scilla_tokens: Option<bool>,
    #[serde(default)]
    ttl: Option<TtlConfig>,
    #[serde(default)]
    retry: Option<RetryConfig>,
}

/// Network-specific configuration (final resolved config)
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub name: String,
    pub chain_id: u64,
    pub rpc_url: String,
    /// Fallback RPC URL (used when primary is unavailable)
    pub fallback_rpc_url: Option<String>,
    pub start_block: u64,
    pub end_block: Option<u64>,
}

/// ClickHouse connection configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ClickHouseConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}

impl ClickHouseConfig {
    /// Get connection URL for ClickHouse HTTP interface
    pub fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Smart contracts addresses
#[derive(Debug, Clone, Deserialize)]
pub struct ContractsConfig {
    // RexSwap DEX
    pub rexswap_dex: String,
    #[serde(default)]
    pub hot_proxy: Option<String>,
    #[serde(default)]
    pub cold_path: Option<String>,
    #[serde(default)]
    pub warm_path: Option<String>,
    #[serde(default)]
    pub micro_paths: Option<String>,
    #[serde(default)]
    pub knockout_path: Option<String>,

    // RexPump contracts
    #[serde(default)]
    pub rexpump: Option<RexPumpContracts>,

    // ERC-20 tokens to track (empty = track none, use with caution for "all")
    #[serde(default)]
    pub erc20_tokens: Vec<String>,
}

/// RexPump contract addresses
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RexPumpContracts {
    /// PositionManager (hooks) contract - main contract with swap/pool events
    #[serde(default)]
    pub position_manager: Option<String>,
    /// BidWall contract
    #[serde(default)]
    pub bidwall: Option<String>,
    /// FairLaunch contract
    #[serde(default)]
    pub fairlaunch: Option<String>,
    /// FeeEscrow contract
    #[serde(default)]
    pub fee_escrow: Option<String>,
    /// RexPump NFT contract (token ownership)
    #[serde(default)]
    pub rexpump_nft: Option<String>,
}

/// Indexer-specific configuration
#[derive(Debug, Clone, Deserialize)]
pub struct IndexerConfig {
    /// Number of blocks to fetch per batch
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
    /// Delay between batch requests in milliseconds (0 = no delay)
    /// Helps avoid rate limiting from RPC providers
    #[serde(default)]
    pub batch_delay_ms: u64,
    /// Polling interval in milliseconds for live indexing
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,
    /// How far behind head to index (for reorg safety)
    #[serde(default = "default_confirmations")]
    pub confirmations: u64,
    /// Enable live indexing after catching up
    #[serde(default = "default_live_indexing")]
    pub live_indexing: bool,
    /// Enable RexSwap DEX calldata tracking (swaps, pools, liquidity)
    #[serde(default = "default_track_rexswap")]
    pub track_rexswap: bool,
    /// Enable ERC-20 transfer tracking
    #[serde(default)]
    pub track_erc20: bool,
    /// Enable ERC-721 (NFT) transfer tracking
    #[serde(default)]
    pub track_erc721: bool,
    /// Enable RexPump event tracking
    #[serde(default)]
    pub track_rexpump: bool,
    /// Track all ERC-20 transfers (WARNING: high load!)
    #[serde(default)]
    pub track_all_erc20: bool,
    /// Track all NFT transfers (WARNING: high load!)
    #[serde(default)]
    pub track_all_erc721: bool,
    /// Enable Scilla token tracking (Zilliqa native tokens like USDT/USDC)
    /// Only relevant for Zilliqa network
    #[serde(default)]
    pub track_scilla_tokens: bool,
    /// TTL settings for tables (in days, 0 = no TTL)
    #[serde(default)]
    pub ttl: TtlConfig,
    /// Retry settings for RPC errors
    #[serde(default)]
    pub retry: RetryConfig,
}

/// Retry configuration for handling RPC failures
#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts before giving up
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Initial delay between retries in seconds
    #[serde(default = "default_initial_delay_secs")]
    pub initial_delay_secs: u64,
    /// Maximum delay between retries in seconds (for exponential backoff)
    #[serde(default = "default_max_delay_secs")]
    pub max_delay_secs: u64,
    /// Use exponential backoff (true) or fixed delay (false)
    #[serde(default = "default_exponential_backoff")]
    pub exponential_backoff: bool,
    /// Batch size for fetching multiple blocks in one RPC request
    #[serde(default = "default_rpc_batch_blocks")]
    pub rpc_batch_blocks: usize,
    /// Batch size for fetching multiple transactions in one RPC request
    #[serde(default = "default_rpc_batch_transactions")]
    pub rpc_batch_transactions: usize,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            initial_delay_secs: default_initial_delay_secs(),
            max_delay_secs: default_max_delay_secs(),
            exponential_backoff: default_exponential_backoff(),
            rpc_batch_blocks: default_rpc_batch_blocks(),
            rpc_batch_transactions: default_rpc_batch_transactions(),
        }
    }
}

fn default_max_retries() -> u32 {
    10 // 10 attempts before giving up
}

fn default_initial_delay_secs() -> u64 {
    30 // Start with 30 seconds
}

fn default_max_delay_secs() -> u64 {
    300 // Max 5 minutes between retries
}

fn default_exponential_backoff() -> bool {
    true // Use exponential backoff by default
}

fn default_rpc_batch_blocks() -> usize {
    50 // Fetch up to 50 blocks per batch request
}

fn default_rpc_batch_transactions() -> usize {
    50 // Fetch up to 50 transactions per batch request
}

/// TTL configuration for tables (in days, 0 = no TTL)
#[derive(Debug, Clone, Deserialize)]
pub struct TtlConfig {
    // ========== ERC Tokens ==========
    /// TTL for token_transfers table (ERC-20 fungible tokens)
    #[serde(default = "default_ttl_erc20")]
    pub erc20_transfers: u32,
    /// TTL for wallet_tokens table (wallet-token interactions)
    #[serde(default)]
    pub wallet_tokens: u32,
    /// TTL for nft_transfers table (ERC-721 NFTs)
    #[serde(default = "default_ttl_nft")]
    pub erc721_transfers: u32,

    // ========== RexSwap DEX ==========
    /// TTL for swaps table (RexSwap DEX swaps)
    #[serde(default)]
    pub rexswap_swaps: u32,
    /// TTL for pools table (RexSwap DEX pools)
    #[serde(default)]
    pub rexswap_pools: u32,
    /// TTL for liquidity_changes table (RexSwap DEX mint/burn)
    #[serde(default)]
    pub rexswap_liquidity: u32,

    // ========== RexPump Launchpad ==========
    /// TTL for rexpump_swaps table
    #[serde(default)]
    pub rexpump_swaps: u32,
    /// TTL for rexpump_pools table (memecoins)
    #[serde(default)]
    pub rexpump_pools: u32,
}

impl Default for TtlConfig {
    fn default() -> Self {
        Self {
            // ERC tokens
            erc20_transfers: default_ttl_erc20(),
            wallet_tokens: 0, // Store forever by default
            erc721_transfers: default_ttl_nft(),
            // RexSwap DEX
            rexswap_swaps: 0,
            rexswap_pools: 0,
            rexswap_liquidity: 0,
            // RexPump
            rexpump_swaps: 0,
            rexpump_pools: 0,
        }
    }
}

fn default_ttl_erc20() -> u32 {
    30 // 30 days
}

fn default_ttl_nft() -> u32 {
    365 // 1 year
}

fn default_batch_size() -> u64 {
    1000
}

fn default_poll_interval() -> u64 {
    1000
}

fn default_confirmations() -> u64 {
    12
}

fn default_live_indexing() -> bool {
    true
}

fn default_track_rexswap() -> bool {
    true // RexSwap DEX tracking enabled by default
}

/// Full configuration file structure with multiple networks
#[derive(Debug, Deserialize)]
struct ConfigFile {
    networks: HashMap<String, NetworkConfigFile>,
    clickhouse: ClickHouseConfig,
    #[serde(default)]
    indexer: Option<IndexerConfig>,
    #[serde(default)]
    api: Option<ApiConfig>,
}

/// Contracts section in config file
#[derive(Debug, Clone, Deserialize, Default)]
struct ContractsConfigFile {
    #[serde(default)]
    rexswap_dex: Option<String>,
    #[serde(default)]
    hot_proxy: Option<String>,
    #[serde(default)]
    cold_path: Option<String>,
    #[serde(default)]
    warm_path: Option<String>,
    #[serde(default)]
    micro_paths: Option<String>,
    #[serde(default)]
    knockout_path: Option<String>,
    #[serde(default)]
    rexpump: Option<RexPumpContracts>,
    #[serde(default)]
    erc20_tokens: Vec<String>,
}

/// Load configuration for a specific network
pub fn load_config(config_path: &str, network: &str) -> Result<AppConfig> {
    let contents = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path))?;

    let config_file: ConfigFile = serde_yaml::from_str(&contents)
        .with_context(|| format!("Failed to parse config file: {}", config_path))?;

    let network_config_file = config_file
        .networks
        .get(network)
        .cloned()
        .with_context(|| {
            format!(
                "Network '{}' not found in config. Available: {:?}",
                network,
                config_file.networks.keys().collect::<Vec<_>>()
            )
        })?;

    // Convert to final NetworkConfig
    let network_config = NetworkConfig {
        name: network_config_file.name,
        chain_id: network_config_file.chain_id,
        rpc_url: network_config_file.rpc_url,
        fallback_rpc_url: network_config_file.fallback_rpc_url,
        start_block: network_config_file.start_block,
        end_block: network_config_file.end_block,
    };

    // Get contracts from network config (env vars can override)
    let network_contracts = network_config_file.contracts.unwrap_or_default();

    let contracts = ContractsConfig {
        rexswap_dex: std::env::var("REXSWAP_DEX_ADDRESS")
            .ok()
            .or(network_contracts.rexswap_dex)
            .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".to_string()),
        hot_proxy: std::env::var("HOT_PROXY_ADDRESS").ok().or(network_contracts.hot_proxy),
        cold_path: std::env::var("COLD_PATH_ADDRESS").ok().or(network_contracts.cold_path),
        warm_path: std::env::var("WARM_PATH_ADDRESS").ok().or(network_contracts.warm_path),
        micro_paths: std::env::var("MICRO_PATHS_ADDRESS").ok().or(network_contracts.micro_paths),
        knockout_path: std::env::var("KNOCKOUT_PATH_ADDRESS").ok().or(network_contracts.knockout_path),
        rexpump: {
            let network_rexpump = network_contracts.rexpump.unwrap_or_default();
            let rexpump = RexPumpContracts {
                position_manager: std::env::var("REXPUMP_POSITION_MANAGER")
                    .ok()
                    .or(network_rexpump.position_manager),
                bidwall: std::env::var("REXPUMP_BIDWALL")
                    .ok()
                    .or(network_rexpump.bidwall),
                fairlaunch: std::env::var("REXPUMP_FAIRLAUNCH")
                    .ok()
                    .or(network_rexpump.fairlaunch),
                fee_escrow: std::env::var("REXPUMP_FEE_ESCROW")
                    .ok()
                    .or(network_rexpump.fee_escrow),
                rexpump_nft: std::env::var("REXPUMP_NFT")
                    .ok()
                    .or(network_rexpump.rexpump_nft),
            };
            // Return Some only if at least one address is set
            if rexpump.position_manager.is_some()
                || rexpump.bidwall.is_some()
                || rexpump.fairlaunch.is_some()
                || rexpump.fee_escrow.is_some()
                || rexpump.rexpump_nft.is_some()
            {
                Some(rexpump)
            } else {
                None
            }
        },
        erc20_tokens: network_contracts.erc20_tokens,
    };

    // Get global indexer defaults
    let global_indexer = config_file.indexer.unwrap_or_else(|| IndexerConfig {
        batch_size: default_batch_size(),
        batch_delay_ms: 0,
        poll_interval_ms: default_poll_interval(),
        confirmations: default_confirmations(),
        live_indexing: default_live_indexing(),
        track_rexswap: default_track_rexswap(),
        track_erc20: false,
        track_erc721: false,
        track_rexpump: false,
        track_all_erc20: false,
        track_all_erc721: false,
        track_scilla_tokens: false,
        ttl: TtlConfig::default(),
        retry: RetryConfig::default(),
    });

    // Apply per-network overrides
    let network_indexer = network_config_file.indexer.unwrap_or_default();
    let indexer = IndexerConfig {
        batch_size: network_indexer.batch_size.unwrap_or(global_indexer.batch_size),
        batch_delay_ms: network_indexer.batch_delay_ms.unwrap_or(global_indexer.batch_delay_ms),
        poll_interval_ms: network_indexer.poll_interval_ms.unwrap_or(global_indexer.poll_interval_ms),
        confirmations: network_indexer.confirmations.unwrap_or(global_indexer.confirmations),
        live_indexing: network_indexer.live_indexing.unwrap_or(global_indexer.live_indexing),
        track_rexswap: network_indexer.track_rexswap.unwrap_or(global_indexer.track_rexswap),
        track_erc20: network_indexer.track_erc20.unwrap_or(global_indexer.track_erc20),
        track_erc721: network_indexer.track_erc721.unwrap_or(global_indexer.track_erc721),
        track_rexpump: network_indexer.track_rexpump.unwrap_or(global_indexer.track_rexpump),
        track_all_erc20: network_indexer.track_all_erc20.unwrap_or(global_indexer.track_all_erc20),
        track_all_erc721: network_indexer.track_all_erc721.unwrap_or(global_indexer.track_all_erc721),
        track_scilla_tokens: network_indexer.track_scilla_tokens.unwrap_or(global_indexer.track_scilla_tokens),
        ttl: network_indexer.ttl.unwrap_or(global_indexer.ttl),
        retry: network_indexer.retry.unwrap_or(global_indexer.retry),
    };

    // Override clickhouse config from environment if available
    let clickhouse = ClickHouseConfig {
        host: std::env::var("CLICKHOUSE_HOST").unwrap_or(config_file.clickhouse.host),
        port: std::env::var("CLICKHOUSE_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(config_file.clickhouse.port),
        user: std::env::var("CLICKHOUSE_USER").unwrap_or(config_file.clickhouse.user),
        password: std::env::var("CLICKHOUSE_PASSWORD").unwrap_or(config_file.clickhouse.password),
        database: std::env::var("CLICKHOUSE_DB").unwrap_or(config_file.clickhouse.database),
    };

    // Override RPC URL from environment if available
    let network_config = NetworkConfig {
        rpc_url: std::env::var(format!("{}_RPC_URL", network.to_uppercase()))
            .unwrap_or(network_config.rpc_url),
        fallback_rpc_url: std::env::var(format!("{}_FALLBACK_RPC_URL", network.to_uppercase()))
            .ok()
            .or(network_config.fallback_rpc_url),
        ..network_config
    };

    // API config: network -> global -> defaults, ENV can override
    let global_api = config_file.api.unwrap_or_default();
    let network_api = network_config_file.api.unwrap_or(global_api);
    let api = ApiConfig {
        enabled: std::env::var("API_ENABLED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(network_api.enabled),
        host: std::env::var("API_HOST").unwrap_or(network_api.host),
        port: std::env::var("API_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(network_api.port),
        cors_origins: std::env::var("API_CORS_ORIGINS")
            .ok()
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or(network_api.cors_origins),
    };

    Ok(AppConfig {
        network: network_config,
        clickhouse,
        contracts,
        indexer,
        api,
    })
}
