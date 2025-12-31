# API Reference

REST API для чтения данных из индексера RexSwap/RexPump.

## Запуск

```bash
# Только API сервер
indexer serve --port 8882

# Индексер + API вместе
indexer start --with-api --port 8882
```

## Конфигурация

**config.yaml:**
```yaml
api:
  enabled: false
  host: "0.0.0.0"
  port: 8882
  cors_origins:
    - "http://localhost:3000"
```

**ENV переменные:**
```bash
API_ENABLED=true
API_HOST=0.0.0.0
API_PORT=8882
API_CORS_ORIGINS=http://localhost:3000,https://app.com
```

**Приоритет:** CLI `--port` > ENV > config.yaml

---

## Общие параметры

### Пагинация

Все списковые эндпоинты поддерживают пагинацию:

| Параметр | Тип | По умолчанию | Описание |
|----------|-----|--------------|----------|
| `limit` | u32 | 50 | Макс. записей на страницу (max: 1000) |
| `offset` | u32 | 0 | Пропустить записей |

**Ответ:**
```json
{
  "items": [...],
  "pagination": {
    "total": 1234,
    "limit": 50,
    "offset": 0,
    "has_more": true
  }
}
```

---

## Health & Status

### GET /api/health

Проверка работоспособности сервера.

**Response:**
```json
{
  "status": "ok",
  "version": "0.1.0",
  "network": "zilliqa_testnet"
}
```

### GET /api/status

Статус индексера и количество записей.

**Response:**
```json
{
  "network": "zilliqa_testnet",
  "chain_id": 33101,
  "last_synced_block": 18500000,
  "counts": {
    "swaps": 12345,
    "pools": 89,
    "liquidity_changes": 567,
    "token_transfers": 99999,
    "nft_transfers": 1234,
    "rexpump_swaps": 5678
  }
}
```

---

## ERC-20 Token Transfers

### GET /api/erc20/transfers

Получить трансферы ERC-20 токенов.

**Query параметры:**
| Параметр | Тип | Описание |
|----------|-----|----------|
| `wallet` | string | Фильтр по адресу кошелька (from или to) |
| `token` | string | Фильтр по адресу токена |

**Примеры:**
```bash
# Все трансферы кошелька
GET /api/erc20/transfers?wallet=0x123...

# Трансферы конкретного токена для кошелька
GET /api/erc20/transfers?wallet=0x123...&token=0xabc...

# Все трансферы токена
GET /api/erc20/transfers?token=0xabc...
```

**Response item:**
```json
{
  "id": "0x..._42",
  "transaction_hash": "0x...",
  "log_index": 42,
  "token_address": "0x...",
  "from_address": "0x...",
  "to_address": "0x...",
  "amount": "1000000000000000000",
  "block_number": 18500000,
  "block_time": "2024-01-15 12:30:00",
  "token_symbol": "USDT",
  "token_decimals": 18
}
```

### GET /api/erc20/tokens/{token}/transfers

Все трансферы конкретного токена.

### GET /api/erc20/wallet/{address}/tokens

Список токенов, с которыми взаимодействовал кошелёк (с metadata).

Данные берутся из таблицы `wallet_tokens` (хранит пары wallet+token с датой последнего взаимодействия).
Metadata токенов автоматически запрашивается из RPC если отсутствует в кеше.

**Response:**
```json
[
  {
    "address": "0xtoken1...",
    "name": "My Token",
    "symbol": "MTK",
    "decimals": 18
  },
  {
    "address": "0xtoken2...",
    "name": "Another Token",
    "symbol": "ATK",
    "decimals": 6
  }
]
```

### GET /api/erc20/token/{address}

Получить metadata токена по адресу.

Если metadata нет в кеше — запрашивается из RPC через `eth_call` (name, symbol, decimals) и сохраняется.

**Response:**
```json
{
  "address": "0x...",
  "name": "My Token",
  "symbol": "MTK",
  "decimals": 18
}
```

---

## NFT (ERC-721) Transfers

### GET /api/nft/transfers

Получить трансферы NFT.

**Query параметры:**
| Параметр | Тип | Описание |
|----------|-----|----------|
| `wallet` | string | Фильтр по адресу кошелька |
| `contract` | string | Фильтр по адресу NFT контракта |

### GET /api/nft/collections/{contract}/transfers

Все трансферы коллекции.

### GET /api/nft/collections/{contract}/{token_id}/history

История владения конкретным NFT (от mint до текущего).

**Response:**
```json
[
  {
    "from_address": "0x0000...",
    "to_address": "0xminter...",
    "block_time": "2024-01-01 10:00:00"
  },
  {
    "from_address": "0xminter...",
    "to_address": "0xbuyer...",
    "block_time": "2024-01-15 12:00:00"
  }
]
```

### GET /api/nft/wallet/{address}/collections

Список NFT коллекций, с которыми взаимодействовал кошелёк.

---

## RexSwap DEX

### Pools

#### GET /api/rexswap/pools

Список всех пулов.

**Response item:**
```json
{
  "id": "0x...",
  "base": "0xtoken1...",
  "quote": "0xtoken2...",
  "pool_idx": "420",
  "template_id": "default",
  "hooks_address": "0x...",
  "block_create": 18000000,
  "time_create": "2024-01-01 00:00:00"
}
```

#### GET /api/rexswap/pools/search?token={address}

Поиск пулов по токену (в base или quote).

#### GET /api/rexswap/pools/{pool_id}

Детали конкретного пула.

#### GET /api/rexswap/pools/{pool_id}/swaps

Свопы в пуле.

#### GET /api/rexswap/pools/{pool_id}/liquidity

Изменения ликвидности в пуле.

### Swaps

#### GET /api/rexswap/swaps

Все свопы (с пагинацией).

**Response item:**
```json
{
  "id": "0x..._0",
  "transaction_hash": "0x...",
  "call_index": 0,
  "user_address": "0x...",
  "pool_id": "0x...",
  "is_buy": true,
  "in_base_qty": true,
  "qty": "1000000000000000000",
  "base_flow": "-1000000000000000000",
  "quote_flow": "2500000000000000000",
  "price": 2.5,
  "call_source": "hot_proxy",
  "block_number": 18500000,
  "block_time": "2024-01-15 12:30:00"
}
```

### User

#### GET /api/rexswap/user/{address}/swaps

Свопы пользователя.

#### GET /api/rexswap/user/{address}/positions

LP позиции пользователя (агрегировано по пулам).

**Response:**
```json
[
  {
    "pool_id": "0x...",
    "position_type": "ambient",
    "mint_count": 5,
    "burn_count": 2,
    "last_activity_block": 18500000
  }
]
```

---

## RexPump (Memecoin Launchpad)

### Tokens

#### GET /api/rexpump/tokens

Список всех мемкоинов.

**Response item:**
```json
{
  "pool_id": "0x...",
  "memecoin_address": "0x...",
  "memecoin_treasury": "0x...",
  "token_id": "123",
  "currency_flipped": false,
  "creator_address": "0x...",
  "creator_fee_allocation": 500,
  "block_number": 18000000,
  "created_at": "2024-01-01 00:00:00",
  "transaction_hash": "0x..."
}
```

#### GET /api/rexpump/tokens/trending?limit=20

Трендовые токены (по активности за 24h).

**Response:**
```json
[
  {
    "pool_id": "0x...",
    "memecoin_address": "0x...",
    "creator_address": "0x...",
    "created_at": "2024-01-01 00:00:00",
    "swap_count_24h": 150,
    "unique_traders_24h": 45
  }
]
```

#### GET /api/rexpump/tokens/{pool_id}

Детали токена со статистикой.

**Response:**
```json
{
  "pool_id": "0x...",
  "memecoin_address": "0x...",
  "creator_address": "0x...",
  "creator_fee_allocation": 500,
  "created_at": "2024-01-01 00:00:00",
  "stats": {
    "total_swaps": 1234,
    "unique_traders": 89,
    "first_swap": "2024-01-01 00:01:00",
    "last_swap": "2024-01-15 12:30:00"
  }
}
```

#### GET /api/rexpump/tokens/{pool_id}/swaps

Свопы токена.

### Charts

#### GET /api/rexpump/tokens/{pool_id}/chart?limit=100

История цен для графика (raw data points).

**Response:**
```json
[
  {
    "sqrt_price_x96": "79228162514264337593543950336",
    "tick": 0,
    "liquidity": "1000000000000000000",
    "block_number": 18500000,
    "timestamp": "2024-01-15 12:30:00"
  }
]
```

#### GET /api/rexpump/tokens/{pool_id}/candles

OHLCV свечи для графика.

**Query параметры:**
| Параметр | Тип | По умолчанию | Описание |
|----------|-----|--------------|----------|
| `interval` | u32 | 60 | Интервал в минутах (1, 5, 15, 30, 60, 240, 1440) |
| `limit` | u32 | 100 | Количество свечей (max: 500) |

**Response:**
```json
[
  {
    "timestamp": "2024-01-15 12:00:00",
    "open": 0.001,
    "high": 0.0015,
    "low": 0.0009,
    "close": 0.0012,
    "volume": "1000000000000000000",
    "trades": 42
  }
]
```

### User

#### GET /api/rexpump/user/{address}/created

Токены, созданные пользователем.

#### GET /api/rexpump/user/{address}/swaps

Свопы пользователя (по всем токенам).

---

## Ошибки

Все ошибки возвращаются в формате:

```json
{
  "error": {
    "code": "NOT_FOUND",
    "message": "Pool not found: 0x..."
  }
}
```

**Коды ошибок:**
| Код | HTTP Status | Описание |
|-----|-------------|----------|
| `NOT_FOUND` | 404 | Ресурс не найден |
| `BAD_REQUEST` | 400 | Неверные параметры запроса |
| `DATABASE_ERROR` | 500 | Ошибка БД |
| `INTERNAL_ERROR` | 500 | Внутренняя ошибка сервера |

---

## Примеры использования

### curl

```bash
# Health check
curl http://localhost:8882/api/health

# Трансферы кошелька
curl "http://localhost:8882/api/erc20/transfers?wallet=0x123...&limit=10"

# Trending токены
curl "http://localhost:8882/api/rexpump/tokens/trending?limit=10"

# Свечи для графика (1 час)
curl "http://localhost:8882/api/rexpump/tokens/0xpool.../candles?interval=60&limit=24"
```

### JavaScript/TypeScript

```typescript
const API_BASE = 'http://localhost:8882/api';

// Получить трансферы кошелька
async function getWalletTransfers(wallet: string, token?: string) {
  const params = new URLSearchParams({ wallet, limit: '50' });
  if (token) params.set('token', token);
  
  const res = await fetch(`${API_BASE}/erc20/transfers?${params}`);
  return res.json();
}

// Получить trending токены
async function getTrendingTokens(limit = 20) {
  const res = await fetch(`${API_BASE}/rexpump/tokens/trending?limit=${limit}`);
  return res.json();
}

// Получить свечи для графика
async function getCandles(poolId: string, interval = 60, limit = 100) {
  const res = await fetch(
    `${API_BASE}/rexpump/tokens/${poolId}/candles?interval=${interval}&limit=${limit}`
  );
  return res.json();
}
```
