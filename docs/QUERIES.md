# Справочник SQL запросов для Indexer

Это руководство содержит примеры SQL запросов для анализа данных, собранных индексером.

> **Примечание:** Все запросы используют полные имена таблиц (`indexer.table_name`), чтобы работать без указания базы данных при подключении.

## Подключение к ClickHouse

```bash
# Интерактивный режим (без указания базы — используем полные имена таблиц)
clickhouse-client --user indexer --password 'YOUR_PASSWORD'

# Интерактивный режим с базой по умолчанию
clickhouse-client --user indexer --password 'YOUR_PASSWORD' --database indexer

# Одиночный запрос
clickhouse-client --user indexer --password 'YOUR_PASSWORD' \
  --query "SELECT * FROM indexer.token_transfers LIMIT 10"

# Через HTTP API
curl -X POST 'http://localhost:8123/?user=indexer&password=YOUR_PASSWORD' \
  -d "SELECT count() FROM indexer.token_transfers"
```

---

## 1. ERC-20 Token Transfers

Таблица: `indexer.token_transfers`

### Структура таблицы

| Поле | Тип | Описание |
|------|-----|----------|
| id | String | Уникальный ID (tx_hash + log_index) |
| transaction_hash | String | Хэш транзакции |
| log_index | UInt32 | Индекс лога в транзакции |
| token_address | String | Адрес токена |
| from_address | String | Отправитель |
| to_address | String | Получатель |
| amount | String | Количество (в wei) |
| block_number | UInt64 | Номер блока |
| block_time | DateTime | Время блока |
| network | String | Сеть (zilliqa_mainnet, etc.) |
| token_symbol | Nullable(String) | Символ токена |
| token_decimals | Nullable(UInt8) | Decimals токена |

### Примеры запросов

```sql
-- Последние 20 трансферов
SELECT 
    block_number,
    block_time,
    token_address,
    from_address,
    to_address,
    amount
FROM indexer.token_transfers 
ORDER BY block_number DESC 
LIMIT 20;

-- Топ-10 токенов по количеству трансферов
SELECT 
    token_address,
    count() AS transfers,
    uniqExact(from_address) AS unique_senders,
    uniqExact(to_address) AS unique_receivers
FROM indexer.token_transfers 
GROUP BY token_address 
ORDER BY transfers DESC 
LIMIT 10;

-- Общая статистика
SELECT 
    count() AS total_transfers,
    min(block_number) AS from_block,
    max(block_number) AS to_block,
    uniqExact(token_address) AS unique_tokens,
    uniqExact(from_address) AS unique_senders,
    uniqExact(to_address) AS unique_receivers
FROM indexer.token_transfers;

-- Трансферы конкретного токена
SELECT 
    block_time,
    from_address,
    to_address,
    amount
FROM indexer.token_transfers 
WHERE token_address = '0xYOUR_TOKEN_ADDRESS'
ORDER BY block_number DESC 
LIMIT 50;

-- Трансферы конкретного адреса (отправленные или полученные)
SELECT 
    block_time,
    token_address,
    CASE 
        WHEN from_address = '0xYOUR_ADDRESS' THEN 'OUT'
        ELSE 'IN'
    END AS direction,
    from_address,
    to_address,
    amount
FROM indexer.token_transfers 
WHERE from_address = '0xYOUR_ADDRESS' OR to_address = '0xYOUR_ADDRESS'
ORDER BY block_number DESC 
LIMIT 50;

-- Объём трансферов по дням
SELECT 
    toDate(block_time) AS date,
    count() AS transfers,
    uniqExact(token_address) AS unique_tokens
FROM indexer.token_transfers 
GROUP BY date 
ORDER BY date DESC 
LIMIT 30;

-- Mint'ы (трансферы от нулевого адреса)
SELECT 
    block_time,
    token_address,
    to_address,
    amount
FROM indexer.token_transfers 
WHERE from_address = '0x0000000000000000000000000000000000000000'
ORDER BY block_number DESC 
LIMIT 20;

-- Burn'ы (трансферы на нулевой адрес)
SELECT 
    block_time,
    token_address,
    from_address,
    amount
FROM indexer.token_transfers 
WHERE to_address = '0x0000000000000000000000000000000000000000'
ORDER BY block_number DESC 
LIMIT 20;
```

---

## 1.1. ERC-20 Token Metadata

Таблица: `indexer.erc20_tokens`

Кеш metadata токенов (name, symbol, decimals). Заполняется автоматически при API запросах.

### Структура таблицы

| Поле | Тип | Описание |
|------|-----|----------|
| address | String | Адрес токена |
| name | String | Название токена |
| symbol | String | Символ токена |
| decimals | UInt8 | Decimals |
| network | String | Сеть |
| first_seen_block | UInt64 | Блок первого появления |
| created_at | DateTime | Время добавления в кеш |

### Примеры запросов

```sql
-- Все закешированные токены
SELECT address, name, symbol, decimals
FROM indexer.erc20_tokens
ORDER BY created_at DESC;

-- Поиск токена по символу
SELECT address, name, symbol, decimals
FROM indexer.erc20_tokens
WHERE symbol ILIKE '%USDT%';
```

---

## 1.2. Wallet-Token Interactions

Таблица: `indexer.wallet_tokens`

Хранит пары кошелёк-токен с датой последнего взаимодействия. 
Позволяет быстро получить список токенов пользователя без агрегации по token_transfers.

### Структура таблицы

| Поле | Тип | Описание |
|------|-----|----------|
| wallet_address | String | Адрес кошелька |
| token_address | String | Адрес токена |
| last_interaction | DateTime | Дата последнего взаимодействия |
| network | String | Сеть |

**Engine:** `ReplacingMergeTree(last_interaction)` — хранит только последнюю запись для каждой пары.

### Примеры запросов

```sql
-- Токены кошелька (используйте FINAL для актуальных данных)
SELECT token_address, last_interaction
FROM indexer.wallet_tokens FINAL
WHERE wallet_address = '0xYOUR_ADDRESS'
ORDER BY last_interaction DESC;

-- Количество уникальных кошельков на токен
SELECT 
    token_address,
    count() AS wallets
FROM indexer.wallet_tokens FINAL
GROUP BY token_address
ORDER BY wallets DESC
LIMIT 20;

-- Активные пользователи за последние 7 дней
SELECT 
    wallet_address,
    count() AS tokens_used
FROM indexer.wallet_tokens FINAL
WHERE last_interaction >= now() - INTERVAL 7 DAY
GROUP BY wallet_address
ORDER BY tokens_used DESC
LIMIT 50;
```

---

## 2. ERC-721 NFT Transfers

Таблица: `indexer.nft_transfers`

### Структура таблицы

| Поле | Тип | Описание |
|------|-----|----------|
| id | String | Уникальный ID |
| transaction_hash | String | Хэш транзакции |
| log_index | UInt32 | Индекс лога |
| contract_address | String | Адрес NFT контракта |
| from_address | String | Отправитель |
| to_address | String | Получатель |
| token_id | String | ID токена |
| block_number | UInt64 | Номер блока |
| block_time | DateTime | Время блока |
| network | String | Сеть |
| collection_name | Nullable(String) | Название коллекции |
| token_uri | Nullable(String) | URI метаданных |

### Примеры запросов

```sql
-- Последние 20 NFT трансферов
SELECT 
    block_time,
    contract_address,
    token_id,
    from_address,
    to_address
FROM indexer.nft_transfers 
ORDER BY block_number DESC 
LIMIT 20;

-- Топ NFT коллекций по активности
SELECT 
    contract_address,
    count() AS transfers,
    uniqExact(token_id) AS unique_tokens,
    uniqExact(to_address) AS unique_holders
FROM indexer.nft_transfers 
GROUP BY contract_address 
ORDER BY transfers DESC 
LIMIT 10;

-- NFT mint'ы (новые токены)
SELECT 
    block_time,
    contract_address,
    token_id,
    to_address AS minter
FROM indexer.nft_transfers 
WHERE from_address = '0x0000000000000000000000000000000000000000'
ORDER BY block_number DESC 
LIMIT 20;

-- История конкретного NFT
SELECT 
    block_time,
    from_address,
    to_address,
    transaction_hash
FROM indexer.nft_transfers 
WHERE contract_address = '0xNFT_CONTRACT' 
  AND token_id = '123'
ORDER BY block_number ASC;

-- NFT активность адреса
SELECT 
    block_time,
    contract_address,
    token_id,
    CASE 
        WHEN from_address = '0xYOUR_ADDRESS' THEN 'SOLD/TRANSFERRED'
        ELSE 'BOUGHT/RECEIVED'
    END AS action
FROM indexer.nft_transfers 
WHERE from_address = '0xYOUR_ADDRESS' OR to_address = '0xYOUR_ADDRESS'
ORDER BY block_number DESC 
LIMIT 50;

-- Уникальные владельцы NFT коллекции (текущие)
-- Примечание: это приближённый запрос, для точных данных нужна отдельная таблица состояния
SELECT 
    to_address AS holder,
    count() AS nfts_received
FROM indexer.nft_transfers 
WHERE contract_address = '0xNFT_CONTRACT'
GROUP BY to_address
ORDER BY nfts_received DESC
LIMIT 20;
```

---

## 3. RexSwap DEX

### 3.1 Swaps (Обмены)

Таблица: `indexer.swaps`

### Структура таблицы

| Поле | Тип | Описание |
|------|-----|----------|
| id | String | Уникальный ID |
| transaction_hash | String | Хэш транзакции |
| call_index | UInt32 | Индекс вызова |
| user_address | String | Адрес пользователя |
| pool_id | String | ID пула |
| is_buy | UInt8 | 1 = покупка base, 0 = продажа |
| is_vault | UInt8 | Использовал vault |
| in_base_qty | UInt8 | Input в base токене |
| qty | String | Количество input |
| limit_price | Nullable(String) | Лимитная цена |
| min_out | Nullable(String) | Минимальный output |
| base_flow | String | Изменение base баланса |
| quote_flow | String | Изменение quote баланса |
| price | Nullable(String) | Цена исполнения |
| call_source | String | Источник вызова |
| dex | String | DEX идентификатор |
| block_number | UInt64 | Номер блока |
| block_time | DateTime | Время блока |
| network | String | Сеть |

### Примеры запросов

```sql
-- Последние 20 свопов
SELECT 
    block_time,
    user_address,
    pool_id,
    CASE WHEN is_buy = 1 THEN 'BUY' ELSE 'SELL' END AS direction,
    qty,
    base_flow,
    quote_flow,
    price
FROM indexer.swaps 
ORDER BY block_number DESC 
LIMIT 20;

-- Объём свопов по пулам
SELECT 
    pool_id,
    count() AS swap_count,
    uniqExact(user_address) AS unique_traders
FROM indexer.swaps 
GROUP BY pool_id 
ORDER BY swap_count DESC 
LIMIT 10;

-- Топ трейдеры
SELECT 
    user_address,
    count() AS trades,
    uniqExact(pool_id) AS pools_traded
FROM indexer.swaps 
GROUP BY user_address 
ORDER BY trades DESC 
LIMIT 20;

-- Свопы конкретного пользователя
SELECT 
    block_time,
    pool_id,
    CASE WHEN is_buy = 1 THEN 'BUY' ELSE 'SELL' END AS direction,
    qty,
    base_flow,
    quote_flow
FROM indexer.swaps 
WHERE user_address = '0xYOUR_ADDRESS'
ORDER BY block_number DESC 
LIMIT 50;

-- Активность по часам (за последние 7 дней)
SELECT 
    toStartOfHour(block_time) AS hour,
    count() AS swaps
FROM indexer.swaps 
WHERE block_time > now() - INTERVAL 7 DAY
GROUP BY hour 
ORDER BY hour DESC;

-- Покупки vs Продажи
SELECT 
    pool_id,
    countIf(is_buy = 1) AS buys,
    countIf(is_buy = 0) AS sells
FROM indexer.swaps 
GROUP BY pool_id 
ORDER BY buys + sells DESC 
LIMIT 10;
```

### 3.2 Pools (Пулы)

Таблица: `indexer.pools`

```sql
-- Все пулы
SELECT 
    id AS pool_id,
    base,
    quote,
    pool_idx,
    template_id,
    time_create,
    block_create
FROM indexer.pools 
ORDER BY block_create DESC;

-- Поиск пула по токенам
SELECT * FROM indexer.pools 
WHERE base = '0xTOKEN_A' OR quote = '0xTOKEN_A';

-- Статистика по шаблонам пулов
SELECT 
    template_id,
    count() AS pool_count
FROM indexer.pools 
GROUP BY template_id 
ORDER BY pool_count DESC;
```

### 3.3 Liquidity Changes (Изменения ликвидности)

Таблица: `indexer.liquidity_changes`

```sql
-- Последние изменения ликвидности
SELECT 
    block_time,
    pool_id,
    user_address,
    change_type,
    position_type,
    base_flow,
    quote_flow
FROM indexer.liquidity_changes 
ORDER BY block_number DESC 
LIMIT 20;

-- Добавления vs Удаления ликвидности
SELECT 
    pool_id,
    countIf(change_type = 'mint') AS mints,
    countIf(change_type = 'burn') AS burns
FROM indexer.liquidity_changes 
GROUP BY pool_id 
ORDER BY mints + burns DESC 
LIMIT 10;

-- LP активность пользователя
SELECT 
    block_time,
    pool_id,
    change_type,
    position_type,
    base_flow,
    quote_flow
FROM indexer.liquidity_changes 
WHERE user_address = '0xYOUR_ADDRESS'
ORDER BY block_number DESC;
```

---

## 4. RexPump Launchpad

### 4.1 RexPump Pools (Мемкоины)

Таблица: `indexer.rexpump_pools`

```sql
-- Все созданные мемкоины
SELECT 
    block_time,
    pool_id,
    memecoin_address,
    creator_address,
    token_id
FROM indexer.rexpump_pools 
ORDER BY block_number DESC 
LIMIT 20;

-- Топ создатели мемкоинов
SELECT 
    creator_address,
    count() AS memecoins_created
FROM indexer.rexpump_pools 
GROUP BY creator_address 
ORDER BY memecoins_created DESC 
LIMIT 10;
```

### 4.2 RexPump Swaps (Свопы мемкоинов)

Таблица: `indexer.rexpump_swaps`

```sql
-- Последние свопы мемкоинов
SELECT 
    block_time,
    pool_id,
    sender,
    amount0,
    amount1,
    fee0,
    fee1
FROM indexer.rexpump_swaps 
ORDER BY block_number DESC 
LIMIT 20;

-- Топ мемкоины по объёму торгов
SELECT 
    pool_id,
    count() AS swap_count,
    uniqExact(sender) AS unique_traders
FROM indexer.rexpump_swaps 
GROUP BY pool_id 
ORDER BY swap_count DESC 
LIMIT 10;

-- Активность трейдера в RexPump
SELECT 
    block_time,
    pool_id,
    amount0,
    amount1
FROM indexer.rexpump_swaps 
WHERE sender = '0xYOUR_ADDRESS'
ORDER BY block_number DESC;
```

### 4.3 RexPump Pool States (Состояние пулов)

Таблица: `indexer.rexpump_pool_states`

```sql
-- Последнее состояние пулов
SELECT 
    pool_id,
    sqrt_price_x96,
    tick,
    liquidity,
    block_time
FROM indexer.rexpump_pool_states 
ORDER BY block_number DESC 
LIMIT 20;

-- История цены конкретного мемкоина
SELECT 
    block_time,
    sqrt_price_x96,
    tick,
    liquidity
FROM indexer.rexpump_pool_states 
WHERE pool_id = '0xPOOL_ID'
ORDER BY block_number ASC;
```

### 4.4 Fee Distributions (Распределение комиссий)

Таблица: `indexer.rexpump_fee_distributions`

```sql
-- Распределение комиссий
SELECT 
    block_time,
    pool_id,
    donate_amount,
    creator_amount,
    bidwall_amount,
    governance_amount,
    protocol_amount
FROM indexer.rexpump_fee_distributions 
ORDER BY block_number DESC 
LIMIT 20;

-- Общие комиссии по пулам
SELECT 
    pool_id,
    count() AS distributions,
    sum(toUInt256OrZero(creator_amount)) AS total_creator_fees
FROM indexer.rexpump_fee_distributions 
GROUP BY pool_id 
ORDER BY distributions DESC 
LIMIT 10;
```

### 4.5 BidWall Events

Таблица: `indexer.rexpump_bidwall_events`

```sql
-- BidWall события
SELECT 
    block_time,
    pool_id,
    event_type,
    eth_amount,
    tick_lower,
    tick_upper
FROM indexer.rexpump_bidwall_events 
ORDER BY block_number DESC 
LIMIT 20;

-- Типы BidWall событий
SELECT 
    event_type,
    count() AS event_count
FROM indexer.rexpump_bidwall_events 
GROUP BY event_type;
```

### 4.6 FairLaunch Events

Таблица: `indexer.rexpump_fairlaunch_events`

```sql
-- FairLaunch события
SELECT 
    block_time,
    pool_id,
    event_type,
    tokens,
    starts_at,
    ends_at,
    revenue
FROM indexer.rexpump_fairlaunch_events 
ORDER BY block_number DESC 
LIMIT 20;

-- Активные FairLaunch
SELECT 
    pool_id,
    event_type,
    tokens,
    starts_at,
    ends_at
FROM indexer.rexpump_fairlaunch_events 
WHERE event_type = 'created'
ORDER BY block_number DESC 
LIMIT 10;
```

### 4.7 Referrer Fees

Таблица: `indexer.rexpump_referrer_fees`

```sql
-- Реферальные выплаты
SELECT 
    block_time,
    pool_id,
    recipient,
    token_address,
    amount
FROM indexer.rexpump_referrer_fees 
ORDER BY block_number DESC 
LIMIT 20;

-- Топ рефереры
SELECT 
    recipient,
    count() AS payments,
    uniqExact(pool_id) AS pools_referred
FROM indexer.rexpump_referrer_fees 
GROUP BY recipient 
ORDER BY payments DESC 
LIMIT 10;
```

---

## 5. Indexer State

Таблица: `indexer.indexer_state`

```sql
-- Текущее состояние индексера
SELECT 
    network,
    last_synced_block,
    updated_at
FROM indexer.indexer_state FINAL;

-- История синхронизации
SELECT 
    network,
    last_synced_block,
    updated_at
FROM indexer.indexer_state 
ORDER BY updated_at DESC 
LIMIT 100;
```

---

## 6. Полезные аналитические запросы

### Общая активность сети

```sql
-- Дневная активность (все типы событий)
SELECT 
    toDate(block_time) AS date,
    'ERC20' AS type,
    count() AS events
FROM indexer.token_transfers 
GROUP BY date

UNION ALL

SELECT 
    toDate(block_time) AS date,
    'NFT' AS type,
    count() AS events
FROM indexer.nft_transfers 
GROUP BY date

UNION ALL

SELECT 
    toDate(block_time) AS date,
    'Swap' AS type,
    count() AS events
FROM indexer.swaps 
GROUP BY date

ORDER BY date DESC, type
LIMIT 100;
```

### Поиск по транзакции

```sql
-- Найти все события в транзакции
SELECT 'ERC20' AS type, * FROM indexer.token_transfers WHERE transaction_hash = '0xTX_HASH'
UNION ALL
SELECT 'NFT' AS type, * FROM indexer.nft_transfers WHERE transaction_hash = '0xTX_HASH'
UNION ALL
SELECT 'Swap' AS type, * FROM indexer.swaps WHERE transaction_hash = '0xTX_HASH';
```

### Экспорт в CSV

```bash
clickhouse-client --user indexer --password 'YOUR_PASSWORD' \
  --query "SELECT * FROM indexer.token_transfers FORMAT CSV" > transfers.csv
```

### Экспорт в JSON

```bash
clickhouse-client --user indexer --password 'YOUR_PASSWORD' \
  --query "SELECT * FROM indexer.token_transfers LIMIT 100 FORMAT JSONEachRow" > transfers.json
```

---

## 7. Оптимизация запросов

### Используйте фильтры по времени

```sql
-- Хорошо: фильтр по block_time
SELECT * FROM indexer.token_transfers 
WHERE block_time > now() - INTERVAL 1 DAY;

-- Хорошо: фильтр по block_number
SELECT * FROM indexer.token_transfers 
WHERE block_number > 15600000;
```

### Используйте LIMIT

```sql
-- Всегда добавляйте LIMIT для интерактивных запросов
SELECT * FROM indexer.token_transfers 
ORDER BY block_number DESC 
LIMIT 100;
```

### Используйте FINAL для таблиц с ReplacingMergeTree

```sql
-- Для получения актуальных данных из indexer_state
SELECT * FROM indexer.indexer_state FINAL;
```

---

## 8. Список всех таблиц

```sql
-- Показать все таблицы в базе indexer
SHOW TABLES FROM indexer;

-- Структура конкретной таблицы
DESCRIBE TABLE indexer.token_transfers;
DESCRIBE TABLE indexer.nft_transfers;
DESCRIBE TABLE indexer.swaps;
DESCRIBE TABLE indexer.pools;
DESCRIBE TABLE indexer.liquidity_changes;
DESCRIBE TABLE indexer.rexpump_pools;
DESCRIBE TABLE indexer.rexpump_swaps;
DESCRIBE TABLE indexer.rexpump_pool_states;
DESCRIBE TABLE indexer.rexpump_fee_distributions;
DESCRIBE TABLE indexer.rexpump_bidwall_events;
DESCRIBE TABLE indexer.rexpump_fairlaunch_events;
DESCRIBE TABLE indexer.rexpump_referrer_fees;
DESCRIBE TABLE indexer.indexer_state;

-- Размер таблиц
SELECT 
    table,
    formatReadableSize(sum(bytes_on_disk)) AS size,
    sum(rows) AS rows
FROM system.parts 
WHERE database = 'indexer' AND active
GROUP BY table
ORDER BY sum(bytes_on_disk) DESC;
```

---

## Полезные ссылки

- [ClickHouse SQL Reference](https://clickhouse.com/docs/en/sql-reference)
- [ClickHouse Functions](https://clickhouse.com/docs/en/sql-reference/functions)
- [ClickHouse Client](https://clickhouse.com/docs/en/interfaces/cli)
