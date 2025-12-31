//! RexSwap Indexer - Transaction and Event-based indexing
//!
//! This indexer supports multiple modes:
//! - RexSwap: Parses transaction calldata (no events emitted)
//! - ERC-20: Parses Transfer events
//! - RexPump: Parses events from RexPump contracts

mod processor;
mod raw_rpc;
mod sync;

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use alloy::primitives::Address;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::BlockNumberOrTag;

use crate::config::{AppConfig, RetryConfig};
use crate::db::ClickHouseClient;
use crate::events::{CalldataDecoder, DecodedTransaction};
use crate::handlers::{BlockContext, HandlerRegistry, TxContext};
use crate::handlers::rexpump::RexPumpContracts;

pub use processor::EventProcessor;
pub use sync::SyncState;

/// Execute an async operation with retry logic
async fn with_retry<F, Fut, T>(
    operation_name: &str,
    retry_config: &RetryConfig,
    mut operation: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut attempt = 0;
    let mut delay_secs = retry_config.initial_delay_secs;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempt += 1;
                
                if attempt > retry_config.max_retries {
                    error!(
                        "{} failed after {} attempts: {}",
                        operation_name, retry_config.max_retries, e
                    );
                    return Err(e);
                }

                warn!(
                    "{} failed (attempt {}/{}), retrying in {} seconds: {}",
                    operation_name,
                    attempt,
                    retry_config.max_retries,
                    delay_secs,
                    e
                );

                tokio::time::sleep(Duration::from_secs(delay_secs)).await;

                // Calculate next delay
                if retry_config.exponential_backoff {
                    delay_secs = (delay_secs * 2).min(retry_config.max_delay_secs);
                }
            }
        }
    }
}

/// Execute an async operation with fallback and retry logic
/// First tries primary operation, if it fails tries fallback (if provided), then retries
async fn with_fallback_retry<F1, F2, Fut1, Fut2, T>(
    operation_name: &str,
    retry_config: &RetryConfig,
    mut primary_op: F1,
    mut fallback_op: Option<F2>,
) -> Result<T>
where
    F1: FnMut() -> Fut1,
    F2: FnMut() -> Fut2,
    Fut1: std::future::Future<Output = Result<T>>,
    Fut2: std::future::Future<Output = Result<T>>,
{
    let mut attempt = 0;
    let mut delay_secs = retry_config.initial_delay_secs;

    loop {
        // Try primary first
        match primary_op().await {
            Ok(result) => return Ok(result),
            Err(primary_err) => {
                // Try fallback if available
                if let Some(ref mut fallback) = fallback_op {
                    debug!(
                        "{} primary failed, trying fallback: {}",
                        operation_name, primary_err
                    );
                    match fallback().await {
                        Ok(result) => {
                            debug!("{} fallback succeeded", operation_name);
                            return Ok(result);
                        }
                        Err(fallback_err) => {
                            // Both failed, proceed to retry logic
                            attempt += 1;
                            
                            if attempt > retry_config.max_retries {
                                error!(
                                    "{} failed after {} attempts. Primary: {}; Fallback: {}",
                                    operation_name, retry_config.max_retries, primary_err, fallback_err
                                );
                                return Err(primary_err);
                            }

                            warn!(
                                "{} failed (attempt {}/{}), retrying in {} seconds. Primary: {}; Fallback: {}",
                                operation_name,
                                attempt,
                                retry_config.max_retries,
                                delay_secs,
                                primary_err,
                                fallback_err
                            );
                        }
                    }
                } else {
                    // No fallback, standard retry
                    attempt += 1;
                    
                    if attempt > retry_config.max_retries {
                        error!(
                            "{} failed after {} attempts: {}",
                            operation_name, retry_config.max_retries, primary_err
                        );
                        return Err(primary_err);
                    }

                    warn!(
                        "{} failed (attempt {}/{}), retrying in {} seconds: {}",
                        operation_name,
                        attempt,
                        retry_config.max_retries,
                        delay_secs,
                        primary_err
                    );
                }

                tokio::time::sleep(Duration::from_secs(delay_secs)).await;

                // Calculate next delay
                if retry_config.exponential_backoff {
                    delay_secs = (delay_secs * 2).min(retry_config.max_delay_secs);
                }
            }
        }
    }
}

/// Main indexer entry point
pub async fn run(config: AppConfig, from_block_override: Option<u64>) -> Result<()> {
    info!(
        "Initializing indexer for {}",
        config.network.name
    );

    // Connect to ClickHouse
    let db_client = Arc::new(ClickHouseClient::new(&config.clickhouse).await?);

    // Ensure schema exists
    db_client.init_schema().await?;

    // Determine starting block
    let start_block = if let Some(block) = from_block_override {
        info!("Using override start block: {}", block);
        block
    } else if let Some(last_block) = db_client
        .get_last_synced_block(&config.network.name)
        .await?
    {
        info!("Resuming from last synced block: {}", last_block + 1);
        last_block + 1
    } else {
        info!(
            "Starting from configured block: {}",
            config.network.start_block
        );
        config.network.start_block
    };

    // Connect to RPC (primary)
    let provider = ProviderBuilder::new()
        .on_http(config.network.rpc_url.parse().context("Invalid RPC URL")?);
    
    // Create fallback provider if configured
    let fallback_provider = if let Some(ref fallback_url) = config.network.fallback_rpc_url {
        info!("Fallback RPC configured: {}", fallback_url);
        Some(ProviderBuilder::new()
            .on_http(fallback_url.parse().context("Invalid fallback RPC URL")?))
    } else {
        None
    };
    
    // Create raw RPC client for chains with non-standard transaction types (with fallback and batch settings)
    let raw_rpc = Arc::new(raw_rpc::RawRpcClient::with_fallback(
        &config.network.rpc_url,
        config.network.fallback_rpc_url.clone(),
        config.indexer.retry.rpc_batch_blocks,
        config.indexer.retry.rpc_batch_transactions,
    ));

    // Get current chain head (with fallback and retry)
    let latest_block = with_fallback_retry(
        "get latest block number",
        &config.indexer.retry,
        || async {
            provider
                .get_block_number()
                .await
                .context("Failed to get latest block")
        },
        fallback_provider.as_ref().map(|fp| {
            let fp = fp.clone();
            move || {
                let fp = fp.clone();
                async move {
                    fp.get_block_number()
                        .await
                        .context("Failed to get latest block (fallback)")
                }
            }
        }),
    )
    .await?;

    info!(
        "Chain head: {}, Starting from: {}, Blocks to sync: {}",
        latest_block,
        start_block,
        latest_block.saturating_sub(start_block)
    );

    // Initialize sync state - batch_size from config determines how many blocks we process at once
    // rpc_batch_blocks (in retry config) limits individual RPC calls within the batch
    let sync_state = Arc::new(Mutex::new(SyncState::new(
        start_block,
        config.network.end_block.unwrap_or(latest_block),
        config.indexer.batch_size,
    )));

    // Initialize event processor for RexSwap calldata
    let processor = Arc::new(EventProcessor::new(
        db_client.clone(),
        config.network.name.clone(),
    ));

    // Get DEX contract address (if RexSwap tracking is enabled)
    let track_rexswap = config.indexer.track_rexswap;
    let dex_address: Address = if track_rexswap {
        config
            .contracts
            .rexswap_dex
            .parse()
            .context("Invalid DEX address")?
    } else {
        Address::ZERO
    };

    if track_rexswap && dex_address != Address::ZERO {
        info!("RexSwap DEX tracking enabled at: {:?}", dex_address);
    } else if track_rexswap {
        warn!("RexSwap tracking enabled but no valid DEX address configured");
    }

    // Setup handler registry for event-based indexing
    let handler_registry = setup_handlers(&config)?;
    let handler_registry = Arc::new(handler_registry);

    // Log enabled handlers
    for handler in handler_registry.handlers() {
        info!("Enabled handler: {}", handler.name());
    }

    // Historic sync loop
    info!("Starting historic sync loop...");
    loop {
        let (from_block, to_block) = {
            let state = sync_state.lock().await;
            if state.is_complete() {
                break;
            }
            state.next_batch()
        };

        if from_block > to_block {
            break;
        }

        let batch_start = std::time::Instant::now();

        // Process batch of blocks efficiently
        let mut processed_txs = 0;
        let mut processed_events = 0;

        match process_blocks_batch(
            &provider,
            fallback_provider.as_ref(),
            &raw_rpc,
            &processor,
            &handler_registry,
            &dex_address,
            from_block,
            to_block,
            &config.network.name,
            &db_client,
            &config.indexer.retry,
        )
        .await
        {
            Ok((tx_count, event_count)) => {
                processed_txs = tx_count;
                processed_events = event_count;
            }
            Err(e) => {
                warn!("Error processing blocks {} - {}: {:?}", from_block, to_block, e);
                // Reduce batch size and retry
                let mut state = sync_state.lock().await;
                state.reduce_batch_size();
                continue;
            }
        }

        // Update sync state
        {
            let mut state = sync_state.lock().await;
            state.update_progress(to_block);
        }

        // Update database
        db_client
            .update_last_synced_block(&config.network.name, to_block)
            .await?;

        // Log progress with timing
        let batch_elapsed = batch_start.elapsed();
        let state = sync_state.lock().await;
        let progress = state.progress_percent();
        info!(
            "Sync progress: {:.2}% (block {}, {} txs, {} events, {}ms)",
            progress, to_block, processed_txs, processed_events, batch_elapsed.as_millis()
        );
        
        // Delay between batches to avoid rate limiting
        if config.indexer.batch_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(config.indexer.batch_delay_ms)).await;
        }
    }

    info!("Historic sync completed!");

    // Live indexing mode
    if config.indexer.live_indexing {
        info!("Entering live indexing mode...");

        let confirmations = config.indexer.confirmations;
        let poll_interval = Duration::from_millis(config.indexer.poll_interval_ms);
        let mut last_processed_block = {
            let state = sync_state.lock().await;
            state.current_block
        };

        loop {
            // Get latest block with confirmations (with fallback and retry on failure)
            let latest = match with_fallback_retry(
                "get latest block (live)",
                &config.indexer.retry,
                || async {
                    provider
                        .get_block_number()
                        .await
                        .context("Failed to get latest block")
                },
                fallback_provider.as_ref().map(|fp| {
                    let fp = fp.clone();
                    move || {
                        let fp = fp.clone();
                        async move {
                            fp.get_block_number()
                                .await
                                .context("Failed to get latest block (fallback)")
                        }
                    }
                }),
            )
            .await
            {
                Ok(n) => n.saturating_sub(confirmations),
                Err(e) => {
                    error!("RPC unavailable after retries: {:?}", e);
                    // Wait longer before next attempt
                    tokio::time::sleep(Duration::from_secs(config.indexer.retry.max_delay_secs)).await;
                    continue;
                }
            };

            if latest > last_processed_block {
                let from_block = last_processed_block + 1;
                let to_block = latest;

                debug!("Live: processing blocks {} - {}", from_block, to_block);

                let mut processed_txs = 0;
                let mut processed_events = 0;

                for block_num in from_block..=to_block {
                    match process_block(
                        &provider,
                        fallback_provider.as_ref(),
                        &raw_rpc,
                        &processor,
                        &handler_registry,
                        &dex_address,
                        block_num,
                        &config.network.name,
                        &db_client,
                        &config.indexer.retry,
                    )
                    .await
                    {
                        Ok((tx_count, event_count)) => {
                            processed_txs += tx_count;
                            processed_events += event_count;
                        }
                        Err(e) => {
                            warn!("Failed to process live block {}: {:?}", block_num, e);
                        }
                    }
                }

                if processed_txs > 0 || processed_events > 0 {
                    info!(
                        "Live: processed {} txs, {} events from blocks {} - {}",
                        processed_txs, processed_events, from_block, to_block
                    );
                }

                last_processed_block = to_block;
                db_client
                    .update_last_synced_block(&config.network.name, to_block)
                    .await?;
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    Ok(())
}

/// Setup event handlers based on config
fn setup_handlers(config: &AppConfig) -> Result<HandlerRegistry> {
    // Parse ERC-20 token addresses
    let erc20_tokens: Vec<Address> = if config.indexer.track_all_erc20 {
        vec![] // Empty = track all
    } else if config.indexer.track_erc20 {
        config
            .contracts
            .erc20_tokens
            .iter()
            .filter_map(|addr| addr.parse().ok())
            .collect()
    } else {
        vec![] // Will not be used since we check track_erc20 flag
    };

    // Parse RexPump contract addresses
    let rexpump_contracts = if config.indexer.track_rexpump {
        if let Some(ref rp) = config.contracts.rexpump {
            RexPumpContracts {
                position_manager: rp
                    .position_manager
                    .as_ref()
                    .and_then(|a| a.parse().ok())
                    .unwrap_or(Address::ZERO),
                bidwall: rp
                    .bidwall
                    .as_ref()
                    .and_then(|a| a.parse().ok())
                    .unwrap_or(Address::ZERO),
                fairlaunch: rp
                    .fairlaunch
                    .as_ref()
                    .and_then(|a| a.parse().ok())
                    .unwrap_or(Address::ZERO),
                fee_escrow: rp
                    .fee_escrow
                    .as_ref()
                    .and_then(|a| a.parse().ok())
                    .unwrap_or(Address::ZERO),
            }
        } else {
            RexPumpContracts {
                position_manager: Address::ZERO,
                bidwall: Address::ZERO,
                fairlaunch: Address::ZERO,
                fee_escrow: Address::ZERO,
            }
        }
    } else {
        RexPumpContracts {
            position_manager: Address::ZERO,
            bidwall: Address::ZERO,
            fairlaunch: Address::ZERO,
            fee_escrow: Address::ZERO,
        }
    };

    // Create registry
    let mut registry = HandlerRegistry::new();

    // Add ERC-20 handler if tracking is enabled
    if config.indexer.track_erc20 || config.indexer.track_all_erc20 {
        info!(
            "ERC-20 tracking enabled for {} tokens",
            if erc20_tokens.is_empty() {
                "ALL".to_string()
            } else {
                erc20_tokens.len().to_string()
            }
        );
        registry.register_erc20(crate::handlers::erc20::Erc20Handler::new(erc20_tokens));
    }

    // Add ERC-721 (NFT) handler if tracking is enabled
    if config.indexer.track_erc721 || config.indexer.track_all_erc721 {
        info!(
            "ERC-721 (NFT) tracking enabled for {} contracts",
            if config.indexer.track_all_erc721 {
                "ALL".to_string()
            } else {
                "specified".to_string()
            }
        );
        // For now, track all NFTs if enabled (can add erc721_contracts list later)
        registry.register_erc721(crate::handlers::erc721::Erc721Handler::new(vec![]));
    }

    // Add RexPump handler if tracking is enabled
    if config.indexer.track_rexpump {
        info!("RexPump tracking enabled");
        info!("  PositionManager: {:?}", rexpump_contracts.position_manager);
        info!("  BidWall: {:?}", rexpump_contracts.bidwall);
        info!("  FairLaunch: {:?}", rexpump_contracts.fairlaunch);
        info!("  FeeEscrow: {:?}", rexpump_contracts.fee_escrow);
        registry.register_rexpump(crate::handlers::rexpump::RexPumpHandler::new(
            rexpump_contracts,
        ));
    }

    // Add Scilla handler if tracking is enabled (Zilliqa native tokens)
    if config.indexer.track_scilla_tokens {
        info!("Scilla token tracking enabled (Zilliqa native tokens: USDT, USDC, etc.)");
        registry.register_scilla(crate::handlers::scilla::ScillaHandler::new());
    }

    Ok(registry)
}

/// Process a single block - both calldata and events
/// Process a batch of blocks efficiently using batch RPC calls
async fn process_blocks_batch<P: Provider + Clone>(
    provider: &P,
    fallback_provider: Option<&P>,
    raw_rpc: &raw_rpc::RawRpcClient,
    processor: &EventProcessor,
    handler_registry: &HandlerRegistry,
    dex_address: &Address,
    from_block: u64,
    to_block: u64,
    network: &str,
    db: &ClickHouseClient,
    retry_config: &RetryConfig,
) -> Result<(usize, usize)> {
    use alloy::rpc::types::Filter;
    
    let mut processed_txs = 0;
    let mut processed_events = 0;
    
    // 1. Fetch all blocks in one batch RPC request
    let block_numbers: Vec<u64> = (from_block..=to_block).collect();
    let blocks = with_retry(
        &format!("fetch blocks {} - {}", from_block, to_block),
        retry_config,
        || {
            let nums = block_numbers.clone();
            async move {
                // Direct batch request - no internal chunking
                raw_rpc.batch_get_blocks_direct(&nums).await
            }
        },
    )
    .await?;
    
    // 2. Process DEX calldata from transactions
    for (block_num, block) in &blocks {
        let block_time = block.block_timestamp();
        
        for tx in &block.transactions {
            if let Some(to) = tx.to_address() {
                if to == *dex_address {
                    let input = tx.input_bytes();
                    if !input.is_empty() {
                        match CalldataDecoder::decode(&input) {
                            Ok(decoded) => {
                                if !matches!(decoded, DecodedTransaction::Unknown) {
                                    if let Some(event) = CalldataDecoder::to_event(&decoded, &tx.hash) {
                                        if let Err(e) = processor
                                            .process_decoded_event(
                                                &event,
                                                *block_num,
                                                block_time,
                                                &tx.hash,
                                                tx.tx_index(),
                                            )
                                            .await
                                        {
                                            warn!("Failed to process tx {}: {:?}", tx.hash, e);
                                        } else {
                                            processed_txs += 1;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                debug!("Failed to decode tx {}: {:?}", tx.hash, e);
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Count total transactions in fetched blocks
    let total_txs_in_blocks: usize = blocks.iter().map(|(_, b)| b.transactions.len()).sum();
    
    // 3. Fetch logs for the entire range in one request
    if !handler_registry.is_empty() {
        let filter = Filter::new()
            .from_block(from_block)
            .to_block(to_block);
        
        // Try primary first, fall back if 0 logs but blocks have transactions
        // (handles pruned nodes that return empty results for old blocks)
        let primary_logs = with_retry(
            &format!("fetch logs for blocks {} - {}", from_block, to_block),
            retry_config,
            || {
                let f = filter.clone();
                async move {
                    provider.get_logs(&f).await.context("Failed to fetch logs")
                }
            },
        )
        .await?;
        
        // If primary returned 0 logs BUT blocks have transactions, try fallback
        let logs = if primary_logs.is_empty() && total_txs_in_blocks > 0 {
            if let Some(fallback) = fallback_provider {
                debug!(
                    "Primary returned 0 logs but {} txs in blocks, trying fallback...",
                    total_txs_in_blocks
                );
                let fallback_logs = with_retry(
                    &format!("fetch logs for blocks {} - {} (fallback)", from_block, to_block),
                    retry_config,
                    || {
                        let f = filter.clone();
                        async move {
                            fallback
                                .get_logs(&f)
                                .await
                                .context("Failed to fetch logs (fallback)")
                        }
                    },
                )
                .await?;
                
                if !fallback_logs.is_empty() {
                    debug!(
                        "Fallback returned {} logs for blocks {} - {}",
                        fallback_logs.len(), from_block, to_block
                    );
                }
                fallback_logs
            } else {
                primary_logs
            }
        } else {
            primary_logs
        };
        
        debug!("Fetched {} logs for blocks {} - {}", logs.len(), from_block, to_block);
        
        if !logs.is_empty() {
            // Build block timestamps map from our fetched blocks
            let block_times: std::collections::HashMap<u64, u64> = blocks
                .iter()
                .map(|(num, block)| (*num, block.block_timestamp()))
                .collect();
            
            // Group logs by block, then by transaction
            let mut logs_by_block: std::collections::HashMap<u64, Vec<_>> = std::collections::HashMap::new();
            for log in &logs {
                let log_block_num = log.block_number.unwrap_or(0);
                logs_by_block.entry(log_block_num).or_default().push(log.clone());
            }
            
            // Process each block's logs
            for (log_block_num, block_logs) in logs_by_block {
                let block_time = block_times.get(&log_block_num).copied().unwrap_or(0);
                
                let block_ctx = BlockContext {
                    block_number: log_block_num,
                    block_time,
                    network: network.to_string(),
                };
                
                // Group logs by transaction
                let mut logs_by_tx: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
                for log in block_logs {
                    let tx_hash = log.transaction_hash
                        .map(|h| format!("{:?}", h))
                        .unwrap_or_else(|| "0x0".to_string());
                    logs_by_tx.entry(tx_hash).or_default().push(log);
                }
                
                for (tx_hash, tx_logs) in logs_by_tx {
                    let tx_index = tx_logs.first()
                        .and_then(|l| l.transaction_index)
                        .unwrap_or(0) as u32;

                    let tx_ctx = TxContext {
                        transaction_hash: tx_hash.clone(),
                        transaction_index: tx_index,
                    };

                    // Filter logs that match any handler
                    let matching_logs: Vec<_> = tx_logs
                        .iter()
                        .filter(|log| handler_registry.has_matching_handler(log))
                        .cloned()
                        .collect();

                    if !matching_logs.is_empty() {
                        match handler_registry
                            .process_logs(&matching_logs, &block_ctx, &tx_ctx, db)
                            .await
                        {
                            Ok(count) => processed_events += count,
                            Err(e) => {
                                warn!(
                                    "Failed to process events in tx {}: {:?}",
                                    tx_ctx.transaction_hash, e
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok((processed_txs, processed_events))
}

async fn process_block<P: Provider + Clone>(
    provider: &P,
    fallback_provider: Option<&P>,
    raw_rpc: &raw_rpc::RawRpcClient,
    processor: &EventProcessor,
    handler_registry: &HandlerRegistry,
    dex_address: &Address,
    block_num: u64,
    network: &str,
    db: &ClickHouseClient,
    retry_config: &RetryConfig,
) -> Result<(usize, usize)> {
    let mut processed_txs = 0;
    let mut processed_events = 0;

    // 1. Process RexSwap calldata using raw RPC (works with non-standard chains)
    // We use raw JSON parsing to handle chains like Zilliqa with custom tx types
    // Note: raw_rpc already has fallback support built-in
    debug!("Fetching raw block {} via RPC...", block_num);
    let raw_block = with_retry(
        &format!("fetch raw block {}", block_num),
        retry_config,
        || async {
            debug!("Calling get_block_with_transactions for block {}", block_num);
            raw_rpc
                .get_block_with_transactions(block_num)
                .await?
                .context("Block not found")
        },
    )
    .await?;
    debug!("Got raw block {}", block_num);

    let block_time = raw_block.block_timestamp();

    // Process transactions for RexSwap DEX calldata
    for tx in &raw_block.transactions {
        if let Some(to) = tx.to_address() {
            if to == *dex_address {
                let input = tx.input_bytes();
                if !input.is_empty() {
                    match CalldataDecoder::decode(&input) {
                        Ok(decoded) => {
                            if !matches!(decoded, DecodedTransaction::Unknown) {
                                if let Some(event) = CalldataDecoder::to_event(&decoded, &tx.hash) {
                                    if let Err(e) = processor
                                        .process_decoded_event(
                                            &event,
                                            block_num,
                                            block_time,
                                            &tx.hash,
                                            tx.tx_index(),
                                        )
                                        .await
                                    {
                                        warn!("Failed to process tx {}: {:?}", tx.hash, e);
                                    } else {
                                        processed_txs += 1;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            debug!("Failed to decode tx {}: {:?}", tx.hash, e);
                        }
                    }
                }
            }
        }
    }

    // 2. Process events via eth_getLogs (compatible with non-standard chains like Zilliqa)
    if !handler_registry.is_empty() {
        use alloy::rpc::types::Filter;
        
        // Get logs for this block using eth_getLogs
        let filter = Filter::new()
            .from_block(block_num)
            .to_block(block_num);
        
        // Use fallback for get_logs if available
        let logs = if let Some(fallback) = fallback_provider {
            with_fallback_retry(
                &format!("fetch logs for block {}", block_num),
                retry_config,
                || {
                    let f = filter.clone();
                    async move {
                        provider
                            .get_logs(&f)
                            .await
                            .context("Failed to fetch logs")
                    }
                },
                Some(|| {
                    let f = filter.clone();
                    async move {
                        fallback
                            .get_logs(&f)
                            .await
                            .context("Failed to fetch logs (fallback)")
                    }
                }),
            )
            .await?
        } else {
            with_retry(
                &format!("fetch logs for block {}", block_num),
                retry_config,
                || async {
                    provider
                        .get_logs(&filter)
                        .await
                        .context("Failed to fetch logs")
                },
            )
            .await?
        };

        if !logs.is_empty() {
            let block_ctx = BlockContext {
                block_number: block_num,
                block_time,
                network: network.to_string(),
            };

            // Group logs by transaction
            let mut logs_by_tx: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
            for log in &logs {
                let tx_hash = log.transaction_hash
                    .map(|h| format!("{:?}", h))
                    .unwrap_or_else(|| "0x0".to_string());
                logs_by_tx.entry(tx_hash).or_default().push(log.clone());
            }

            for (tx_hash, tx_logs) in logs_by_tx {
                let tx_index = tx_logs.first()
                    .and_then(|l| l.transaction_index)
                    .unwrap_or(0) as u32;

                let tx_ctx = TxContext {
                    transaction_hash: tx_hash.clone(),
                    transaction_index: tx_index,
                };

                // Filter logs that match any handler
                let matching_logs: Vec<_> = tx_logs
                    .iter()
                    .filter(|log| handler_registry.has_matching_handler(log))
                    .cloned()
                    .collect();

                if !matching_logs.is_empty() {
                    match handler_registry
                        .process_logs(&matching_logs, &block_ctx, &tx_ctx, db)
                        .await
                    {
                        Ok(count) => processed_events += count,
                        Err(e) => {
                            warn!(
                                "Failed to process events in tx {}: {:?}",
                                tx_ctx.transaction_hash, e
                            );
                        }
                    }
                }
            }
        }
    }

    Ok((processed_txs, processed_events))
}
