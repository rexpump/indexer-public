# Архитектура RexSwap Indexer

## Обзор

Универсальный блокчейн-индексер для:
- **RexSwap DEX** — свопы, пулы, ликвидность (через парсинг calldata)
- **RexPump** — мемкоины, BidWall, FairLaunch (через события)
- **ERC-20 токены** — Transfer события fungible токенов
- **ERC-721 (NFT)** — Transfer события NFT (автоматическое определение)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           RexSwap Indexer                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌──────────┐    ┌───────────────────────────────────────────────────────┐  │
│  │  Config  │    │                    Indexer Loop                        │  │
│  │  (YAML)  │───▶│  for block in start..=latest:                         │  │
│  └──────────┘    │      block = get_block_by_number(block, Full)         │  │
│                  │      receipts = get_block_receipts(block)              │  │
│                  │                                                        │  │
│                  │      // 1. RexSwap calldata (tx.to == DEX)            │  │
│                  │      for tx in block.transactions:                     │  │
│                  │          if tx.to == DEX_ADDRESS:                      │  │
│                  │              decode_calldata(tx.input)                 │  │
│                  │                                                        │  │
│                  │      // 2. Events (logs from receipts)                 │  │
│                  │      for receipt in receipts:                          │  │
│                  │          for log in receipt.logs:                      │  │
│                  │              if handler.matches(log):                  │  │
│                  │                  handler.process(log)                  │  │
│                  └───────────────────────────────────────────────────────┘  │
│                                       │                                      │
│        ┌──────────────────────────────┼──────────────────────────────┐      │
│        ▼                              ▼                              ▼      │
│  ┌───────────────┐           ┌───────────────┐           ┌───────────────┐  │
│  │   RexSwap     │           │  ERC-20/721   │           │   RexPump     │  │
│  │   Handler     │           │    Handler    │           │   Handler     │  │
│  │               │           │               │           │               │  │
│  │ decode swap() │           │ Transfer()    │           │ PoolCreated() │  │
│  │ decode LP ops │           │ (auto-detect) │           │ HookSwap()    │  │
│  │ decode pools  │           │               │           │ BidWall...    │  │
│  └───────┬───────┘           └───────┬───────┘           └───────┬───────┘  │
│          │                           │                           │          │
│          └───────────────────────────┼───────────────────────────┘          │
│                                      ▼                                      │
│                         ┌─────────────────────────┐                         │
│                         │       ClickHouse        │                         │
│                         │                         │                         │
│                         │  swaps, pools,          │                         │
│                         │  token_transfers,       │                         │
│                         │  rexpump_swaps,         │                         │
│                         │  bidwall_events, ...    │                         │
│                         └─────────────────────────┘                         │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Структура проекта

```
src/
├── main.rs                 # CLI: start, init-db, drop-db, status
├── config.rs               # Загрузка YAML + env overrides
│
├── db/                     # База данных (ClickHouse)
│   ├── mod.rs              # Реэкспорт типов
│   ├── client.rs           # ClickHouseClient + методы insert
│   └── schema/             # Модульные схемы таблиц
│       ├── mod.rs          # Trait TableSchema + collect_all_schemas()
│       ├── core.rs         # indexer_state
│       ├── rexswap.rs      # swaps, pools, liquidity_changes
│       ├── erc20.rs        # token_transfers (ERC-20)
│       ├── erc721.rs       # nft_transfers (ERC-721)
│       └── rexpump.rs      # rexpump_* таблицы
│
├── handlers/               # Обработчики событий
│   ├── mod.rs              # Handler enum + HandlerRegistry
│   ├── erc20.rs            # ERC-20 Transfer handler
│   ├── erc721.rs           # ERC-721 (NFT) Transfer handler
│   └── rexpump.rs          # RexPump events handler
│
├── events/                 # RexSwap calldata декодирование
│   ├── mod.rs              
│   ├── abi.rs              # ABI определения событий RexSwap
│   ├── calldata_decoder.rs # Декодер swap(), userCmd()
│   └── types.rs            # SwapEvent, LiquidityChangeEvent, etc.
│
└── indexer/                # Основной цикл индексации
    ├── mod.rs              # run() — главная функция
    ├── processor.rs        # EventProcessor для RexSwap calldata
    └── sync.rs             # SyncState — управление прогрессом
```

---

## Алгоритм работы (детально)

### 1. Запуск и инициализация

```
┌─────────────────────────────────────────────────────────┐
│  1. Загрузка конфига (config.yaml + env vars)           │
│  2. Подключение к ClickHouse                            │
│  3. Инициализация схемы (CREATE TABLE IF NOT EXISTS)    │
│  4. Определение start_block:                            │
│     - CLI override (--from-block)                       │
│     - Или last_synced_block + 1 из БД                   │
│     - Или config.start_block                            │
│  5. Подключение к RPC, получение latest_block           │
│  6. Создание HandlerRegistry с enabled handlers         │
└─────────────────────────────────────────────────────────┘
```

### 2. Historic Sync (догоняем сеть)

```rust
// Псевдокод
let mut current_block = start_block;
let end_block = latest_block;

while current_block <= end_block {
    // Батч блоков (по умолчанию 2000)
    let batch_end = min(current_block + batch_size - 1, end_block);
    
    for block_num in current_block..=batch_end {
        // ========================================
        // Шаг 1: Получить блок с транзакциями
        // ========================================
        let block = provider
            .get_block_by_number(block_num, BlockTransactionsKind::Full)
            .await?;
        
        // ========================================
        // Шаг 2: Обработать RexSwap calldata
        // ========================================
        for tx in block.transactions {
            if tx.to == DEX_ADDRESS {
                let decoded = CalldataDecoder::decode(&tx.input)?;
                match decoded {
                    DecodedTransaction::Swap(swap) => {
                        db.insert_swap(swap).await?;
                    }
                    DecodedTransaction::LiquidityChange(liq) => {
                        db.insert_liquidity_change(liq).await?;
                    }
                    DecodedTransaction::PoolInit(pool) => {
                        db.insert_pool(pool).await?;
                    }
                    _ => continue,
                }
            }
        }
        
        // ========================================
        // Шаг 3: Получить receipts для событий
        // ========================================
        if !handler_registry.is_empty() {
            let receipts = provider
                .get_block_receipts(block_num)
                .await?;
            
            for receipt in receipts {
                for log in receipt.logs {
                    // Проверяем topic0 — сигнатуру события
                    if handler_registry.has_matching_handler(&log) {
                        handler_registry.process_logs(&[log], &block_ctx, &tx_ctx, &db).await?;
                    }
                }
            }
        }
    }
    
    // Сохранить прогресс
    db.update_last_synced_block(batch_end).await?;
    current_block = batch_end + 1;
    
    // Логирование прогресса
    info!("Progress: {:.2}% (block {})", progress_percent, current_block);
}
```

### 3. Live Indexing (реалтайм)

```rust
// После historic sync
loop {
    // Получаем последний безопасный блок (с учётом reorg)
    let latest = provider.get_block_number().await?;
    let safe_block = latest - confirmations;  // default: 12
    
    if safe_block > last_processed_block {
        // Обрабатываем новые блоки
        for block_num in (last_processed_block + 1)..=safe_block {
            process_block(block_num).await?;
        }
        
        last_processed_block = safe_block;
        db.update_last_synced_block(safe_block).await?;
    }
    
    // Ждём следующий poll
    sleep(poll_interval_ms).await;  // default: 1000ms
}
```

### 4. Обработка событий (Handler flow)

```
Log from Receipt
      │
      ▼
┌─────────────────────────────────────────┐
│ 1. Проверка topic0 (event signature)    │
│    topic0 = keccak256("Transfer(...)")  │
│    topic0 = keccak256("HookSwap(...)")  │
└─────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────┐
│ 2. Проверка адреса контракта            │
│    ERC-20: watched_tokens (или все)     │
│    RexPump: position_manager, bidwall.. │
└─────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────┐
│ 3. Декодирование данных события         │
│    SolEvent::decode_log(log, true)      │
└─────────────────────────────────────────┘
      │
      ▼
┌─────────────────────────────────────────┐
│ 4. Создание записи и вставка в БД       │
│    db.insert_xxx(&record).await?        │
└─────────────────────────────────────────┘
```

---

## Добавление нового модуля (пошагово)

### Примечание: ERC-20 и ERC-721 уже поддерживаются!

ERC-20 и ERC-721 используют одинаковый topic для Transfer события, но разную структуру:
- **3 topics** → ERC-20 (value в data) → `token_transfers`
- **4 topics** → ERC-721 (tokenId indexed) → `nft_transfers`

Каждый тип имеет свой handler и свою таблицу.

### Пример: Добавляем ERC-1155 (Multi-Token) handler

#### Шаг 1: Создать схему БД

Файл `src/db/schema/erc1155.rs`:

```rust
use super::TableSchema;

pub struct Erc1155Schema;

impl TableSchema for Erc1155Schema {
    fn module_name() -> &'static str {
        "erc1155"
    }

    fn create_tables_sql() -> &'static str {
        r#"
        CREATE TABLE IF NOT EXISTS erc1155_transfers (
            id String,
            transaction_hash String,
            log_index UInt32,
            contract_address String,
            operator String,
            from_address String,
            to_address String,
            token_id String,
            amount String,
            block_number UInt64,
            block_time DateTime,
            network String,
            is_batch UInt8  -- 1 for TransferBatch, 0 for TransferSingle
        ) ENGINE = MergeTree()
        ORDER BY (network, contract_address, block_number, log_index)
        PARTITION BY toYYYYMM(block_time)
        "#
    }

    fn drop_tables_sql() -> &'static str {
        "DROP TABLE IF EXISTS erc1155_transfers"
    }

    fn table_names() -> &'static [&'static str] {
        &["erc1155_transfers"]
    }
}

#[derive(Debug, Clone)]
pub struct Erc1155TransferRecord {
    pub id: String,
    pub transaction_hash: String,
    pub log_index: u32,
    pub contract_address: String,
    pub operator: String,
    pub from_address: String,
    pub to_address: String,
    pub token_id: String,
    pub amount: String,
    pub block_number: u64,
    pub block_time: String,
    pub network: String,
    pub is_batch: bool,
}
```

#### Шаг 2: Зарегистрировать схему

В `src/db/schema/mod.rs`:

```rust
mod erc1155;
pub use erc1155::*;

pub fn collect_create_schemas() -> Vec<(&'static str, &'static str)> {
    vec![
        // ... существующие
        (Erc1155Schema::module_name(), Erc1155Schema::create_tables_sql()),
    ]
}
```

#### Шаг 3: Добавить метод insert в client

В `src/db/client.rs`:

```rust
pub async fn insert_erc1155_transfer(&self, transfer: &Erc1155TransferRecord) -> Result<()> {
    let sql = format!(
        r#"INSERT INTO erc1155_transfers (...) VALUES (...)"#,
        // ...
    );
    self.client.query(&sql).execute().await?;
    Ok(())
}
```

#### Шаг 4: Создать handler

Файл `src/handlers/erc1155.rs`:

```rust
use alloy::sol;
use alloy::sol_types::SolEvent;

sol! {
    // ERC-1155 события
    #[derive(Debug)]
    event TransferSingle(
        address indexed operator,
        address indexed from,
        address indexed to,
        uint256 id,
        uint256 value
    );
    
    #[derive(Debug)]
    event TransferBatch(
        address indexed operator,
        address indexed from,
        address indexed to,
        uint256[] ids,
        uint256[] values
    );
}

pub struct Erc1155Handler {
    watched_contracts: Vec<Address>,
    single_topic: B256,
    batch_topic: B256,
}

impl Erc1155Handler {
    pub fn new(contracts: Vec<Address>) -> Self {
        Self {
            watched_contracts: contracts,
            single_topic: TransferSingle::SIGNATURE_HASH,
            batch_topic: TransferBatch::SIGNATURE_HASH,
        }
    }

    pub fn topic_signatures(&self) -> Vec<B256> {
        vec![self.single_topic, self.batch_topic]
    }

    pub fn matches_log(&self, log: &Log) -> bool {
        if log.topics().is_empty() {
            return false;
        }
        let topic0 = log.topics()[0];
        topic0 == self.single_topic || topic0 == self.batch_topic
    }

    pub async fn process_logs(...) -> Result<usize> {
        // Декодируем и вставляем
    }
}
```

#### Шаг 5: Зарегистрировать в HandlerRegistry

В `src/handlers/mod.rs`:

```rust
pub enum Handler {
    Erc20(erc20::Erc20Handler),
    RexPump(rexpump::RexPumpHandler),
    Erc1155(erc1155::Erc1155Handler),  // <- добавить
}

impl Handler {
    pub fn matches_log(&self, log: &Log) -> bool {
        match self {
            // ...
            Handler::Erc1155(h) => h.matches_log(log),
        }
    }
    // ... и остальные методы
}

impl HandlerRegistry {
    pub fn register_erc1155(&mut self, handler: erc1155::Erc1155Handler) {
        for topic in handler.topic_signatures() {
            self.all_topics.insert(topic);
        }
        self.handlers.push(Handler::Erc1155(handler));
    }
}
```

#### Шаг 6: Добавить флаг в конфиг

```yaml
indexer:
  track_erc1155: true

contracts:
  erc1155_contracts:
    - "0x..."  # Multi-token контракт
```

#### Шаг 7: Добавить TTL в конфиг

В `src/config.rs` добавить поле в `TtlConfig`:

```rust
pub struct TtlConfig {
    // ... существующие поля
    
    /// TTL for erc1155_transfers table
    #[serde(default)]
    pub erc1155_transfers: u32,
}
```

В `src/db/client.rs` в методе `update_ttl` добавить таблицу:

```rust
let ttl_updates = vec![
    // ... существующие
    ("erc1155_transfers", "block_time", ttl_config.erc1155_transfers),
];
```

В `config.yaml`:

```yaml
indexer:
  ttl:
    # ... существующие
    erc1155_transfers: 90  # или 0 для хранения вечно
```

---

## Конфигурация

### config.yaml

```yaml
networks:
  zilliqa_testnet:
    name: zilliqa_testnet
    chain_id: 33101
    rpc_url: "https://dev-api.zilliqa.com"
    start_block: 17820324

clickhouse:
  host: "localhost"
  port: 8123
  user: "default"
  password: ""
  database: "rexswap"

contracts:
  # RexSwap DEX (обязательно для calldata parsing)
  rexswap_dex: "0x8630616b0198e7e449d0aad3603b73f665188025"
  
  # RexPump контракты
  rexpump:
    position_manager: "0x789d72235bff6215b37ecf0f8deaa8f43a92f0dc"
    bidwall: "0x3d5052fc64dcec858fb178afec0011ee41f47f00"
    fairlaunch: "0x2a0849e2773164031d877614bb8bd8c7e6a6cd9f"
    fee_escrow: "0x58abe9779d3a3b399d9003b9d2675e65e76899fc"
  
  # ERC-20 токены для отслеживания (пустой = не трекаем)
  erc20_tokens:
    - "0x..."  # Memecoin 1
    - "0x..."  # Memecoin 2

indexer:
  batch_size: 2000         # Блоков за раз
  poll_interval_ms: 1000   # Интервал live polling
  confirmations: 12        # Защита от reorg
  live_indexing: true      # Включить live после sync
  
  # Флаги включения handlers
  track_erc20: true        # ERC-20 Transfer
  track_all_erc20: false   # ВСЕ токены (осторожно!)
  track_rexpump: true      # RexPump события
```

### Environment overrides

```bash
# ClickHouse
export CLICKHOUSE_HOST=localhost
export CLICKHOUSE_PORT=8123
export CLICKHOUSE_USER=default
export CLICKHOUSE_PASSWORD=secret
export CLICKHOUSE_DB=rexswap

# RPC
export ZILLIQA_TESTNET_RPC_URL=https://...

# Контракты
export REXSWAP_DEX_ADDRESS=0x...
export REXPUMP_POSITION_MANAGER=0x...
```

---

## Команды CLI

```bash
# Инициализация БД (создаёт все таблицы)
./indexer init-db --network zilliqa_testnet

# Запуск индексера
./indexer start --network zilliqa_testnet

# С указанием стартового блока
./indexer start --network zilliqa_testnet --from-block 17820324

# Статус
./indexer status --network zilliqa_testnet

# Удаление всех таблиц (ОСТОРОЖНО!)
./indexer drop-db --network zilliqa_testnet --confirm
```

---

## Примеры SQL запросов

### RexSwap — свопы

```sql
-- Все свопы за последние 24 часа
SELECT 
    transaction_hash,
    pool_id,
    is_buy,
    qty,
    base_flow,
    quote_flow,
    price,
    block_time
FROM swaps
WHERE network = 'zilliqa_testnet'
  AND block_time >= now() - INTERVAL 24 HOUR
ORDER BY block_time DESC
LIMIT 100;

-- Объём свопов по пулам
SELECT 
    pool_id,
    count() as swap_count,
    sum(abs(toInt256(base_flow))) as total_base_volume
FROM swaps
WHERE network = 'zilliqa_testnet'
GROUP BY pool_id
ORDER BY swap_count DESC;

-- Топ трейдеры (если user_address заполнен)
SELECT 
    user_address,
    count() as trades,
    uniqExact(pool_id) as pools_traded
FROM swaps
WHERE network = 'zilliqa_testnet'
  AND user_address != '0x0'
GROUP BY user_address
ORDER BY trades DESC
LIMIT 20;
```

### RexSwap — пулы и ликвидность

```sql
-- Все созданные пулы
SELECT 
    id as pool_id,
    base,
    quote,
    block_create,
    time_create
FROM pools
WHERE network = 'zilliqa_testnet'
ORDER BY block_create DESC;

-- Ликвидность: mint vs burn по пулу
SELECT 
    pool_id,
    change_type,
    count() as operations,
    sum(toInt128OrNull(liq)) as total_liq
FROM liquidity_changes
WHERE network = 'zilliqa_testnet'
GROUP BY pool_id, change_type
ORDER BY pool_id, change_type;
```

### RexPump — мемкоины

```sql
-- Все созданные токены
SELECT 
    pool_id,
    memecoin_address,
    creator_address,
    creator_fee_allocation,
    block_time
FROM rexpump_pools
WHERE network = 'zilliqa_testnet'
ORDER BY block_time DESC;

-- Свопы по мемкоину
SELECT 
    rp.memecoin_address,
    count(rs.id) as swap_count,
    min(rs.block_time) as first_swap,
    max(rs.block_time) as last_swap
FROM rexpump_pools rp
JOIN rexpump_swaps rs ON rp.pool_id = rs.pool_id
WHERE rp.network = 'zilliqa_testnet'
GROUP BY rp.memecoin_address
ORDER BY swap_count DESC;

-- Распределение комиссий
SELECT 
    pool_id,
    sum(toUInt256OrZero(creator_amount)) as total_creator_fees,
    sum(toUInt256OrZero(bidwall_amount)) as total_bidwall_fees,
    sum(toUInt256OrZero(protocol_amount)) as total_protocol_fees
FROM rexpump_fee_distributions
WHERE network = 'zilliqa_testnet'
GROUP BY pool_id;

-- BidWall события
SELECT 
    pool_id,
    event_type,
    eth_amount,
    tick_lower,
    tick_upper,
    block_time
FROM rexpump_bidwall_events
WHERE network = 'zilliqa_testnet'
ORDER BY block_time DESC
LIMIT 50;

-- Fair Launch статус
SELECT 
    pool_id,
    event_type,
    tokens,
    starts_at,
    ends_at,
    revenue,
    supply
FROM rexpump_fairlaunch_events
WHERE network = 'zilliqa_testnet'
ORDER BY block_time DESC;
```

### ERC-20 — токен трансферы

Таблица `token_transfers` хранит ERC-20 события (TTL: 30 дней).

```sql
-- Трансферы токена
SELECT 
    transaction_hash,
    from_address,
    to_address,
    amount,
    block_time
FROM token_transfers
WHERE network = 'zilliqa_testnet'
  AND token_address = '0x...'
ORDER BY block_time DESC
LIMIT 100;

-- Баланс движения по кошельку
SELECT 
    token_address,
    sumIf(toInt256(amount), to_address = '0xWALLET') as received,
    sumIf(toInt256(amount), from_address = '0xWALLET') as sent,
    sumIf(toInt256(amount), to_address = '0xWALLET') 
        - sumIf(toInt256(amount), from_address = '0xWALLET') as balance_change
FROM token_transfers
WHERE network = 'zilliqa_testnet'
  AND (from_address = '0xWALLET' OR to_address = '0xWALLET')
GROUP BY token_address;

-- История ERC-20 трансферов кошелька
SELECT 
    block_time,
    transaction_hash,
    token_address,
    CASE WHEN from_address = '0xWALLET' THEN 'OUT' ELSE 'IN' END as direction,
    amount
FROM token_transfers
WHERE network = 'zilliqa_testnet'
  AND (from_address = '0xWALLET' OR to_address = '0xWALLET')
ORDER BY block_time DESC;
```

### ERC-721 — NFT трансферы

Таблица `nft_transfers` хранит ERC-721 события (TTL: 1 год).

```sql
-- NFT трансферы по контракту
SELECT 
    transaction_hash,
    from_address,
    to_address,
    token_id,
    block_time
FROM nft_transfers
WHERE network = 'zilliqa_testnet'
  AND contract_address = '0x...'
ORDER BY block_time DESC
LIMIT 100;

-- История конкретного NFT
SELECT 
    from_address,
    to_address,
    block_time,
    transaction_hash
FROM nft_transfers
WHERE network = 'zilliqa_testnet'
  AND contract_address = '0x...'
  AND token_id = '123'
ORDER BY block_time ASC;

-- NFT владения кошелька по коллекциям
SELECT 
    contract_address,
    countIf(to_address = '0xWALLET') as nfts_received,
    countIf(from_address = '0xWALLET') as nfts_sent
FROM nft_transfers
WHERE network = 'zilliqa_testnet'
  AND (from_address = '0xWALLET' OR to_address = '0xWALLET')
GROUP BY contract_address;

-- История NFT кошелька
SELECT 
    block_time,
    contract_address,
    token_id,
    CASE WHEN from_address = '0xWALLET' THEN 'OUT' ELSE 'IN' END as direction
FROM nft_transfers
WHERE network = 'zilliqa_testnet'
  AND (from_address = '0xWALLET' OR to_address = '0xWALLET')
ORDER BY block_time DESC;
```

### Общие запросы

```sql
-- Статус индексера
SELECT 
    network,
    last_synced_block,
    updated_at
FROM indexer_state FINAL;

-- Количество записей по таблицам
SELECT 'swaps' as table_name, count() as rows FROM swaps WHERE network = 'zilliqa_testnet'
UNION ALL
SELECT 'pools', count() FROM pools WHERE network = 'zilliqa_testnet'
UNION ALL
SELECT 'token_transfers', count() FROM token_transfers WHERE network = 'zilliqa_testnet'
UNION ALL
SELECT 'rexpump_swaps', count() FROM rexpump_swaps WHERE network = 'zilliqa_testnet';

-- Активность по дням
SELECT 
    toDate(block_time) as day,
    count() as transactions
FROM swaps
WHERE network = 'zilliqa_testnet'
GROUP BY day
ORDER BY day DESC
LIMIT 30;
```

---

## TTL — автоматическая очистка данных

ClickHouse поддерживает автоматическое удаление старых данных через TTL (Time To Live).
TTL настраивается в `config.yaml` и применяется автоматически.

### Конфигурация TTL

```yaml
indexer:
  ttl:
    # ERC Tokens
    erc20_transfers: 30    # token_transfers (дней, 0 = без TTL)
    erc721_transfers: 365  # nft_transfers (1 год)
    
    # RexSwap DEX
    rexswap_swaps: 0       # swaps (хранить вечно)
    rexswap_pools: 0       # pools
    rexswap_liquidity: 0   # liquidity_changes (mint/burn)
    
    # RexPump Launchpad
    rexpump_swaps: 0       # rexpump_swaps
    rexpump_pools: 0       # rexpump_pools (memecoins)
```

### Команды для работы с TTL

```bash
# Применить TTL из конфига при инициализации
./indexer init-db --network zilliqa_testnet

# Обновить TTL на существующих таблицах после изменения конфига
./indexer update-ttl --network zilliqa_testnet

# Посмотреть текущие настройки TTL
./indexer status --network zilliqa_testnet
```

### Как работает

1. При `init-db` создаются таблицы без TTL
2. Затем применяется TTL из конфига через `ALTER TABLE`
3. При изменении конфига — запустить `update-ttl`
4. `0` в конфиге = без TTL (данные хранятся вечно)

### Ручное изменение TTL (альтернатива)

```sql
-- Через SQL напрямую
ALTER TABLE token_transfers MODIFY TTL block_time + INTERVAL 7 DAY;
ALTER TABLE nft_transfers MODIFY TTL block_time + INTERVAL 6 MONTH;
ALTER TABLE swaps REMOVE TTL;  -- Отключить TTL
```

### Принудительная очистка

TTL применяется в фоне при merge операциях. Для немедленной очистки:

```sql
-- Форсировать merge (удалит просроченные данные)
OPTIMIZE TABLE token_transfers FINAL;
```

### Проверка TTL

```sql
-- Посмотреть TTL таблицы
SELECT name, engine, partition_key, sorting_key, 
       primary_key, data_paths, metadata_path
FROM system.tables 
WHERE database = 'rexswap' AND name = 'token_transfers';

-- Или через SHOW CREATE TABLE
SHOW CREATE TABLE token_transfers;
```

---

## Retry логика (устойчивость к сбоям)

Индексер автоматически повторяет RPC запросы при ошибках сети/ноды.

### Конфигурация

```yaml
indexer:
  retry:
    max_retries: 10            # Максимум попыток
    initial_delay_secs: 30     # Первая задержка 30 сек
    max_delay_secs: 300        # Максимум 5 минут между попытками
    exponential_backoff: true  # Экспоненциальное увеличение задержки
```

### Как работает

1. При ошибке RPC (нода недоступна, timeout и т.д.)
2. Ждём `initial_delay_secs` (30 сек)
3. Пробуем снова
4. Если снова ошибка — удваиваем задержку (60 сек)
5. Продолжаем до `max_retries` попыток
6. Задержка не превышает `max_delay_secs`

### Пример логов при сбое ноды

```
WARN  fetch block 12345 failed (attempt 1/10), retrying in 30 seconds: connection refused
WARN  fetch block 12345 failed (attempt 2/10), retrying in 60 seconds: connection refused
WARN  fetch block 12345 failed (attempt 3/10), retrying in 120 seconds: connection refused
INFO  Processed block 12345 (recovered)
```

### Что происходит после исчерпания попыток

Если все попытки исчерпаны — индексер завершается с ошибкой.
При использовании systemd с `Restart=always` он перезапустится.

---

## Ограничения и особенности

### RexSwap (calldata parsing)

| Данные | Доступность | Комментарий |
|--------|-------------|-------------|
| Входные параметры swap | ✅ | Из tx.input |
| baseFlow / quoteFlow | ⚠️ | Нужен trace (не реализовано) |
| user_address | ✅ | tx.from |
| Успех транзакции | ⚠️ | Нужен receipt.status |

### Events (ERC-20, RexPump)

| Данные | Доступность | Комментарий |
|--------|-------------|-------------|
| Все параметры события | ✅ | Из logs |
| Sender транзакции | ⚠️ | Нужен tx lookup |
| Internal transactions | ❌ | Нужен trace |

### Производительность

- **Historic sync**: ~500-1000 блоков/сек (зависит от RPC)
- **Live indexing**: задержка ~12 блоков (confirmations)
- **Storage**: ~1GB на 1M свопов (зависит от данных)

---

## Troubleshooting

### "Block not found"
RPC не синхронизирован или блок ещё не появился.

### "Failed to decode calldata"
Транзакция не к DEX контракту или неизвестная функция — это нормально.

### "Table already exists"
`init-db` безопасен — использует `CREATE TABLE IF NOT EXISTS`.

### Высокое потребление памяти
Уменьшите `batch_size` в конфиге.

### Пропущенные события
Проверьте что адреса контрактов в конфиге правильные.
