# RexSwap Indexer

Универсальный высокопроизводительный индексатор блокчейна на Rust.

## Что индексирует

| Модуль | Источник | Описание |
|--------|----------|----------|
| **RexSwap DEX** | Calldata транзакций | Свопы, пулы, ликвидность |
| **RexPump** | События (logs) | Мемкоины, BidWall, FairLaunch, комиссии |
| **ERC-20** | События (logs) | Transfer любых fungible токенов |
| **ERC-721 (NFT)** | События (logs) | NFT трансферы (автоматически) |

## Особенности

- 🚀 **Быстрый** — Rust + async + batch processing
- 📊 **ClickHouse** — оптимизирован для аналитики
- 🔌 **Модульный** — легко добавить новые типы событий
- 🔄 **Live sync** — следит за новыми блоками в реальном времени
- 🔁 **Восстановление** — продолжает с последнего блока после рестарта

## Быстрый старт

### Требования

- Rust 1.75+
- ClickHouse 23+
- RPC нода (Alchemy, public RPC, или своя)

### Установка

```bash
git clone <repo-url>
cd Indexer
cargo build --release
```

### Конфигурация

Скопируйте и отредактируйте `config.yaml`:

```yaml
networks:
  zilliqa_testnet:
    name: zilliqa_testnet
    chain_id: 33101
    rpc_url: "https://api.testnet.zilliqa.com"
    start_block: 17820324

clickhouse:
  host: "localhost"
  port: 8123
  database: "rexswap"

contracts:
  rexswap_dex: "0x8630616b0198e7e449d0aad3603b73f665188025"
  
  rexpump:
    position_manager: "0x789d72235bff6215b37ecf0f8deaa8f43a92f0dc"
    bidwall: "0x3d5052fc64dcec858fb178afec0011ee41f47f00"
    fairlaunch: "0x2a0849e2773164031d877614bb8bd8c7e6a6cd9f"
    fee_escrow: "0x58abe9779d3a3b399d9003b9d2675e65e76899fc"
  
  erc20_tokens:
    - "0x..."  # Твои мемкоины

indexer:
  batch_size: 2000
  confirmations: 12
  live_indexing: true
  track_erc20: true      # Включить ERC-20
  track_rexpump: true    # Включить RexPump
```

### Запуск

```bash
# 1. Инициализировать БД
./target/release/indexer init-db --network zilliqa_testnet

# 2. Запустить индексер
./target/release/indexer start --network zilliqa_testnet

# 3. Проверить статус
./target/release/indexer status --network zilliqa_testnet
```

## Структура проекта

```
src/
├── main.rs                 # CLI: start, init-db, status, drop-db
├── config.rs               # Загрузка YAML + env overrides
│
├── db/                     # ClickHouse
│   ├── client.rs           # Клиент + методы insert
│   └── schema/             # Модульные схемы таблиц
│       ├── core.rs         # indexer_state
│       ├── rexswap.rs      # swaps, pools, liquidity_changes
│       ├── erc20.rs        # token_transfers (30 дней TTL)
│       ├── erc721.rs       # nft_transfers (1 год TTL)
│       └── rexpump.rs      # rexpump_* таблицы
│
├── handlers/               # Обработчики событий (logs)
│   ├── erc20.rs            # ERC-20 Transfer
│   ├── erc721.rs           # ERC-721 (NFT) Transfer
│   └── rexpump.rs          # PoolCreated, HookSwap, BidWall...
│
├── events/                 # RexSwap calldata декодирование
│   └── calldata_decoder.rs # decode swap(), userCmd()
│
└── indexer/                # Основной цикл
    ├── mod.rs              # run() — главная функция
    └── sync.rs             # SyncState
```

## Алгоритм работы

```
┌─────────────────────────────────────────────────────────────┐
│  for block in start_block..latest_block:                    │
│                                                             │
│    // 1. Получить блок с транзакциями                      │
│    block = get_block_by_number(block_num, Full)            │
│                                                             │
│    // 2. RexSwap: парсить calldata транзакций к DEX        │
│    for tx in block.transactions:                            │
│        if tx.to == DEX_ADDRESS:                            │
│            decode_calldata(tx.input) → swaps, pools        │
│                                                             │
│    // 3. Events: парсить logs из receipts                  │
│    receipts = get_block_receipts(block_num)                │
│    for log in receipts.logs:                               │
│        if ERC-20/721 Transfer → token_transfers            │
│        if RexPump event → rexpump_*                        │
│                                                             │
│    // 4. Сохранить прогресс                                │
│    update_last_synced_block(block_num)                     │
└─────────────────────────────────────────────────────────────┘
```

## Таблицы ClickHouse

### RexSwap
- `swaps` — история свопов
- `pools` — созданные пулы
- `liquidity_changes` — mint/burn операции

### RexPump
- `rexpump_pools` — созданные мемкоины
- `rexpump_swaps` — свопы через хуки
- `rexpump_pool_states` — состояние пулов (цена, тик)
- `rexpump_fee_distributions` — распределение комиссий
- `rexpump_bidwall_events` — BidWall операции
- `rexpump_fairlaunch_events` — Fair Launch
- `rexpump_referrer_fees` — реферальные выплаты

### ERC-20
- `token_transfers` — ERC-20 Transfer события (TTL: 30 дней)

### ERC-721 (NFT)
- `nft_transfers` — NFT Transfer события (TTL: 1 год)

## Примеры запросов

### RexSwap свопы

```sql
-- Последние свопы
SELECT transaction_hash, pool_id, is_buy, qty, price, block_time
FROM swaps
WHERE network = 'zilliqa_testnet'
ORDER BY block_time DESC
LIMIT 100;
```

### RexPump мемкоины

```sql
-- Созданные токены
SELECT memecoin_address, creator_address, block_time
FROM rexpump_pools
WHERE network = 'zilliqa_testnet'
ORDER BY block_time DESC;

-- Свопы по токену
SELECT rs.pool_id, rs.sender, rs.amount0, rs.amount1, rs.block_time
FROM rexpump_swaps rs
JOIN rexpump_pools rp ON rs.pool_id = rp.pool_id
WHERE rp.memecoin_address = '0x...'
ORDER BY rs.block_time DESC;
```

### ERC-20 трансферы

```sql
-- Трансферы токена
SELECT from_address, to_address, amount, block_time
FROM token_transfers
WHERE token_address = '0x...'
ORDER BY block_time DESC;

-- История кошелька
SELECT block_time, token_address,
    CASE WHEN from_address = '0xWALLET' THEN 'OUT' ELSE 'IN' END as direction,
    amount
FROM token_transfers
WHERE from_address = '0xWALLET' OR to_address = '0xWALLET'
ORDER BY block_time DESC;
```

### NFT (ERC-721) трансферы

```sql
-- NFT трансферы коллекции
SELECT from_address, to_address, token_id, block_time
FROM nft_transfers
WHERE contract_address = '0x...'
ORDER BY block_time DESC;

-- История NFT кошелька
SELECT block_time, contract_address, token_id,
    CASE WHEN from_address = '0xWALLET' THEN 'OUT' ELSE 'IN' END as direction
FROM nft_transfers
WHERE from_address = '0xWALLET' OR to_address = '0xWALLET'
ORDER BY block_time DESC;
```

## Environment Variables

```bash
# ClickHouse
CLICKHOUSE_HOST=localhost
CLICKHOUSE_PORT=8123
CLICKHOUSE_USER=default
CLICKHOUSE_PASSWORD=secret
CLICKHOUSE_DB=rexswap

# RPC (по сетям)
ZILLIQA_TESTNET_RPC_URL=https://dev-api.zilliqa.com

# Контракты
REXSWAP_DEX_ADDRESS=0x...
REXPUMP_POSITION_MANAGER=0x...
REXPUMP_BIDWALL=0x...
```

## Добавление нового модуля

1. Создать `src/db/schema/mymodule.rs` с таблицами
2. Создать `src/handlers/mymodule.rs` с обработчиком событий
3. Зарегистрировать в `HandlerRegistry`
4. Добавить флаг в конфиг

См. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) для деталей.

## CLI команды

```bash
# Инициализация БД (создаёт таблицы + применяет TTL из конфига)
./indexer init-db --network <network>

# Запуск
./indexer start --network <network>
./indexer start --network <network> --from-block 12345

# Статус (включая TTL настройки)
./indexer status --network <network>

# Обновить TTL после изменения конфига
./indexer update-ttl --network <network>

# Удаление всех таблиц
./indexer drop-db --network <network> --confirm
```

## Разработка

```bash
# Dev режим
RUST_LOG=debug cargo run -- --network local start

# Тесты
cargo test

# Форматирование
cargo fmt

# Линтер
cargo clippy
```

## Лицензия

MIT
