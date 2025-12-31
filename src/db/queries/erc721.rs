//! ERC-721 (NFT) transfer queries

use anyhow::Result;
use clickhouse::Row;
use serde::{Deserialize, Serialize};

use crate::db::ClickHouseClient;

/// NFT transfer record from database
#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct NftTransferRow {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub contract_address: String,
    pub from_address: String,
    pub to_address: String,
    pub token_id: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_uri: Option<String>,
}

/// Query parameters for NFT transfers
#[derive(Debug, Default)]
pub struct NftTransferQuery {
    pub wallet: Option<String>,
    pub contract: Option<String>,
    pub token_id: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

/// ERC-721 query methods
pub struct Erc721Queries;

impl Erc721Queries {
    /// Get NFT transfers with filters
    pub async fn get_transfers(
        db: &ClickHouseClient,
        network: &str,
        query: &NftTransferQuery,
    ) -> Result<(Vec<NftTransferRow>, u64)> {
        let mut conditions = vec![format!("network = '{}'", network)];

        // Filter by wallet (from or to)
        if let Some(wallet) = &query.wallet {
            let wallet_lower = wallet.to_lowercase();
            conditions.push(format!(
                "(from_address = '{}' OR to_address = '{}')",
                wallet_lower, wallet_lower
            ));
        }

        // Filter by contract
        if let Some(contract) = &query.contract {
            conditions.push(format!("contract_address = '{}'", contract.to_lowercase()));
        }

        // Filter by token ID
        if let Some(token_id) = &query.token_id {
            conditions.push(format!("token_id = '{}'", token_id));
        }

        let where_clause = conditions.join(" AND ");

        // Get total count
        let count_sql = format!(
            "SELECT count() as count FROM nft_transfers WHERE {}",
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
                id, transaction_hash, log_index, contract_address,
                from_address, to_address, token_id, block_number,
                toString(block_time) as block_time, network,
                collection_name, token_uri
            FROM nft_transfers 
            WHERE {} 
            ORDER BY block_number DESC, log_index DESC 
            LIMIT {} OFFSET {}"#,
            where_clause, query.limit, query.offset
        );

        let transfers = db
            .raw()
            .query(&sql)
            .fetch_all::<NftTransferRow>()
            .await?;

        Ok((transfers, total))
    }

    /// Get history of a specific NFT
    pub async fn get_nft_history(
        db: &ClickHouseClient,
        network: &str,
        contract: &str,
        token_id: &str,
    ) -> Result<Vec<NftTransferRow>> {
        let sql = format!(
            r#"SELECT 
                id, transaction_hash, log_index, contract_address,
                from_address, to_address, token_id, block_number,
                toString(block_time) as block_time, network,
                collection_name, token_uri
            FROM nft_transfers 
            WHERE network = '{}' 
              AND contract_address = '{}' 
              AND token_id = '{}'
            ORDER BY block_number ASC, log_index ASC"#,
            network,
            contract.to_lowercase(),
            token_id
        );

        let transfers = db
            .raw()
            .query(&sql)
            .fetch_all::<NftTransferRow>()
            .await?;

        Ok(transfers)
    }

    /// Get collections (contracts) a wallet interacted with
    pub async fn get_wallet_collections(
        db: &ClickHouseClient,
        network: &str,
        wallet: &str,
    ) -> Result<Vec<String>> {
        let wallet_lower = wallet.to_lowercase();
        let sql = format!(
            r#"SELECT DISTINCT contract_address 
            FROM nft_transfers 
            WHERE network = '{}' 
              AND (from_address = '{}' OR to_address = '{}')
            ORDER BY contract_address"#,
            network, wallet_lower, wallet_lower
        );

        #[derive(Row, Deserialize)]
        struct CollectionRow {
            contract_address: String,
        }

        let rows = db.raw().query(&sql).fetch_all::<CollectionRow>().await?;
        Ok(rows.into_iter().map(|r| r.contract_address).collect())
    }
}
