//! ERC-20 token transfer and metadata queries

use anyhow::Result;
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use crate::db::ClickHouseClient;
use crate::db::schema::erc20::Erc20TokenRecord;

/// ERC-20 transfer record from database
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct TokenTransferRow {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub token_address: String,
    pub from_address: String,
    pub to_address: String,
    pub amount: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_decimals: Option<u8>,
}

/// Query parameters for token transfers
#[derive(Debug, Default)]
pub struct TransferQuery {
    pub wallet: Option<String>,
    pub token: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

/// ERC-20 query methods
pub struct Erc20Queries;

impl Erc20Queries {
    /// Get token transfers with filters
    pub async fn get_transfers(
        db: &ClickHouseClient,
        network: &str,
        query: &TransferQuery,
    ) -> Result<(Vec<TokenTransferRow>, u64)> {
        let mut conditions = vec![format!("network = '{}'", network)];

        // Filter by wallet (from or to)
        if let Some(wallet) = &query.wallet {
            let wallet_lower = wallet.to_lowercase();
            conditions.push(format!(
                "(from_address = '{}' OR to_address = '{}')",
                wallet_lower, wallet_lower
            ));
        }

        // Filter by token
        if let Some(token) = &query.token {
            conditions.push(format!("token_address = '{}'", token.to_lowercase()));
        }

        let where_clause = conditions.join(" AND ");

        // Get total count
        let count_sql = format!(
            "SELECT count() as count FROM token_transfers WHERE {}",
            where_clause
        );
        
        #[derive(Row, Deserialize)]
        struct CountRow {
            count: u64,
        }
        
        let total = db
            .raw()
            .query(&count_sql)
            .fetch_one::<CountRow>()
            .await
            .map(|r| r.count)
            .unwrap_or(0);

        // Get transfers
        let sql = format!(
            r#"SELECT 
                id, transaction_hash, log_index, token_address, 
                from_address, to_address, amount, block_number,
                toString(block_time) as block_time, network,
                token_symbol, token_decimals
            FROM token_transfers 
            WHERE {} 
            ORDER BY block_number DESC, log_index DESC 
            LIMIT {} OFFSET {}"#,
            where_clause, query.limit, query.offset
        );

        let transfers = db
            .raw()
            .query(&sql)
            .fetch_all::<TokenTransferRow>()
            .await?;

        Ok((transfers, total))
    }

    /// Get transfers for a specific token
    pub async fn get_token_transfers(
        db: &ClickHouseClient,
        network: &str,
        token_address: &str,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<TokenTransferRow>, u64)> {
        Self::get_transfers(
            db,
            network,
            &TransferQuery {
                token: Some(token_address.to_string()),
                limit,
                offset,
                ..Default::default()
            },
        )
        .await
    }

    /// Get unique tokens for a wallet (from wallet_tokens table)
    pub async fn get_wallet_tokens(
        db: &ClickHouseClient,
        network: &str,
        wallet: &str,
    ) -> Result<Vec<String>> {
        let wallet_lower = wallet.to_lowercase();
        let sql = format!(
            r#"SELECT token_address 
            FROM wallet_tokens FINAL
            WHERE network = '{}' 
              AND wallet_address = '{}'
            ORDER BY last_interaction DESC"#,
            network, wallet_lower
        );

        #[derive(Row, Deserialize)]
        struct TokenRow {
            token_address: String,
        }

        let rows = db.raw().query(&sql).fetch_all::<TokenRow>().await?;
        Ok(rows.into_iter().map(|r| r.token_address).collect())
    }

    /// Get unique tokens for a wallet with metadata (from wallet_tokens table)
    pub async fn get_wallet_tokens_with_metadata(
        db: &ClickHouseClient,
        network: &str,
        wallet: &str,
    ) -> Result<Vec<Erc20TokenRow>> {
        let wallet_lower = wallet.to_lowercase();
        let sql = format!(
            r#"SELECT 
                t.address,
                t.name,
                t.symbol,
                t.decimals,
                t.network,
                t.first_seen_block
            FROM erc20_tokens t
            WHERE t.network = '{network}'
              AND t.address IN (
                  SELECT token_address 
                  FROM wallet_tokens FINAL
                  WHERE network = '{network}' 
                    AND wallet_address = '{wallet}'
              )
            ORDER BY t.symbol"#,
            network = network,
            wallet = wallet_lower
        );

        let rows = db.raw().query(&sql).fetch_all::<Erc20TokenRow>().await?;
        Ok(rows)
    }

    /// Get token metadata by address
    pub async fn get_token(
        db: &ClickHouseClient,
        network: &str,
        address: &str,
    ) -> Result<Option<Erc20TokenRow>> {
        let sql = format!(
            r#"SELECT address, name, symbol, decimals, network, first_seen_block
            FROM erc20_tokens 
            WHERE network = '{}' AND address = '{}' 
            LIMIT 1"#,
            network,
            address.to_lowercase()
        );

        let rows = db.raw().query(&sql).fetch_all::<Erc20TokenRow>().await?;
        Ok(rows.into_iter().next())
    }

    /// Check if token exists in database
    pub async fn token_exists(
        db: &ClickHouseClient,
        network: &str,
        address: &str,
    ) -> Result<bool> {
        let sql = format!(
            r#"SELECT count() as cnt FROM erc20_tokens 
            WHERE network = '{}' AND address = '{}'"#,
            network,
            address.to_lowercase()
        );

        #[derive(Row, Deserialize)]
        struct CountRow {
            cnt: u64,
        }

        let result = db.raw().query(&sql).fetch_one::<CountRow>().await?;
        Ok(result.cnt > 0)
    }

    /// Insert token metadata
    pub async fn insert_token(
        db: &ClickHouseClient,
        token: &Erc20TokenRecord,
    ) -> Result<()> {
        use crate::db::client::escape_clickhouse_string;
        
        let sql = format!(
            r#"INSERT INTO erc20_tokens (address, name, symbol, decimals, network, first_seen_block)
            VALUES ('{}', '{}', '{}', {}, '{}', {})"#,
            escape_clickhouse_string(&token.address.to_lowercase()),
            escape_clickhouse_string(&token.name),
            escape_clickhouse_string(&token.symbol),
            token.decimals,
            escape_clickhouse_string(&token.network),
            token.first_seen_block
        );

        db.raw().query(&sql).execute().await?;
        Ok(())
    }
}

/// ERC-20 token metadata row from database
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct Erc20TokenRow {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub network: String,
    pub first_seen_block: u64,
}
