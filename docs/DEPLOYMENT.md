# Развертывание RexSwap Indexer

## Требования к серверу

- **CPU**: 2+ ядер
- **RAM**: 4+ GB
- **Диск**: SSD 100+ GB (зависит от объема данных)
- **ОС**: Linux (Ubuntu 22.04 рекомендуется)

---

## 1. Установка ClickHouse

### Ubuntu/Debian

```bash
# Добавляем репозиторий
sudo apt-get install -y apt-transport-https ca-certificates curl gnupg
curl -fsSL 'https://packages.clickhouse.com/rpm/lts/repodata/repomd.xml.key' | sudo gpg --dearmor -o /usr/share/keyrings/clickhouse-keyring.gpg

echo "deb [signed-by=/usr/share/keyrings/clickhouse-keyring.gpg] https://packages.clickhouse.com/deb stable main" | sudo tee /etc/apt/sources.list.d/clickhouse.list

# Устанавливаем
sudo apt-get update
sudo apt-get install -y clickhouse-server clickhouse-client

# Запускаем
sudo systemctl start clickhouse-server
sudo systemctl enable clickhouse-server

# Проверяем
clickhouse-client -q "SELECT 1"
```

### Docker (альтернатива)

```bash
docker run -d \
  --name clickhouse \
  -p 8123:8123 \
  -p 9000:9000 \
  -v clickhouse_data:/var/lib/clickhouse \
  -v clickhouse_logs:/var/log/clickhouse-server \
  --restart unless-stopped \
  clickhouse/clickhouse-server
```

### Настройка базы данных

```bash
clickhouse-client
```

```sql
-- Создаем базу данных
CREATE DATABASE IF NOT EXISTS rexswap;

-- Создаем пользователя (опционально, но рекомендуется)
CREATE USER IF NOT EXISTS indexer IDENTIFIED BY 'your_secure_password';
GRANT ALL ON rexswap.* TO indexer;
```

---

## 2. Сборка индексатора

```bash
# Устанавливаем Rust (если не установлен)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Клонируем репозиторий
sudo mkdir -p /opt/indexer
sudo chown $USER:$USER /opt/indexer
git clone <repo> /opt/indexer
cd /opt/indexer

# Собираем release версию
cargo build --release

# Проверяем
./target/release/indexer --help
```

---

## 3. Конфигурация

### Создаём конфиг

```bash
cp config.yaml /opt/indexer/config.yaml
nano /opt/indexer/config.yaml
```

### Пример config.yaml для продакшена

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
  user: "indexer"
  password: ""  # Лучше через env переменную
  database: "rexswap"

contracts:
  rexswap_dex: "0x8630616b0198e7e449d0aad3603b73f665188025"
  
  rexpump:
    position_manager: "0x789d72235bff6215b37ecf0f8deaa8f43a92f0dc"
    bidwall: "0x3d5052fc64dcec858fb178afec0011ee41f47f00"
    fairlaunch: "0x2a0849e2773164031d877614bb8bd8c7e6a6cd9f"
    fee_escrow: "0x58abe9779d3a3b399d9003b9d2675e65e76899fc"
  
  erc20_tokens: []

indexer:
  batch_size: 2000
  poll_interval_ms: 1000
  confirmations: 12
  live_indexing: true
  track_erc20: false
  track_all_erc20: false
  track_rexpump: true
```

### Создаём env файл (рекомендуется для секретов)

```bash
sudo nano /opt/indexer/.env
```

```bash
# ClickHouse
CLICKHOUSE_HOST=localhost
CLICKHOUSE_PORT=8123
CLICKHOUSE_USER=indexer
CLICKHOUSE_PASSWORD=your_secure_password
CLICKHOUSE_DB=rexswap

# RPC
ZILLIQA_TESTNET_RPC_URL=https://dev-api.zilliqa.com

# Контракты (можно переопределить конфиг)
REXSWAP_DEX_ADDRESS=0x8630616b0198e7e449d0aad3603b73f665188025
REXPUMP_POSITION_MANAGER=0x789d72235bff6215b37ecf0f8deaa8f43a92f0dc
REXPUMP_BIDWALL=0x3d5052fc64dcec858fb178afec0011ee41f47f00
REXPUMP_FAIRLAUNCH=0x2a0849e2773164031d877614bb8bd8c7e6a6cd9f
REXPUMP_FEE_ESCROW=0x58abe9779d3a3b399d9003b9d2675e65e76899fc

# Логирование
RUST_LOG=info
```

```bash
# Защищаем файл
sudo chmod 600 /opt/indexer/.env
```

---

## 4. Создание systemd сервиса

### Создаём пользователя для сервиса

```bash
sudo useradd -r -s /bin/false -d /opt/indexer rexswap
sudo chown -R rexswap:rexswap /opt/indexer
```

### Создаём service файл

```bash
sudo nano /etc/systemd/system/indexer.service
```

```ini
[Unit]
Description=RexSwap Blockchain Indexer
Documentation=https://github.com/your-repo/indexer
After=network-online.target clickhouse-server.service
Wants=network-online.target
Requires=clickhouse-server.service

[Service]
Type=simple
User=rexswap
Group=rexswap
WorkingDirectory=/opt/indexer

# Загрузка переменных из .env файла
EnvironmentFile=/opt/indexer/.env

# Команда запуска
ExecStart=/opt/indexer/target/release/indexer \
    --config /opt/indexer/config.yaml \
    --network zilliqa_testnet \
    start

# Перезапуск при падении
Restart=always
RestartSec=10

# Graceful shutdown
TimeoutStopSec=30
KillMode=mixed
KillSignal=SIGTERM

# Лимиты
LimitNOFILE=65535

# Логирование
StandardOutput=journal
StandardError=journal
SyslogIdentifier=indexer

[Install]
WantedBy=multi-user.target
```

### Активация и запуск

```bash
# Перечитываем конфигурацию systemd
sudo systemctl daemon-reload

# Включаем автозапуск при старте системы
sudo systemctl enable indexer

# Инициализируем БД (от имени сервисного пользователя)
sudo -u rexswap /opt/indexer/target/release/indexer \
    --config /opt/indexer/config.yaml \
    --network zilliqa_testnet \
    init-db

# Запускаем сервис
sudo systemctl start indexer

# Проверяем статус
sudo systemctl status indexer
```

---

## 5. Управление сервисом

### Основные команды

```bash
# Статус
sudo systemctl status indexer

# Запуск
sudo systemctl start indexer

# Остановка
sudo systemctl stop indexer

# Перезапуск
sudo systemctl restart indexer

# Перезагрузка конфига (без перезапуска, если поддерживается)
sudo systemctl reload indexer
```

### Просмотр логов

```bash
# Последние логи (follow mode)
sudo journalctl -u indexer -f

# Логи за последний час
sudo journalctl -u indexer --since "1 hour ago"

# Логи с определённой даты
sudo journalctl -u indexer --since "2024-01-01 00:00:00"

# Только ошибки
sudo journalctl -u indexer -p err

# Логи в JSON формате (для парсинга)
sudo journalctl -u indexer -o json
```

### Проверка статуса индексера

```bash
# Через CLI утилиту
sudo -u rexswap /opt/indexer/target/release/indexer \
    --config /opt/indexer/config.yaml \
    --network zilliqa_testnet \
    status
```

---

## 6. Несколько сетей одновременно

Можно запустить несколько сервисов для разных сетей:

### Создаём сервис для mainnet

```bash
sudo cp /etc/systemd/system/indexer.service \
        /etc/systemd/system/indexer-mainnet.service

sudo nano /etc/systemd/system/indexer-mainnet.service
```

Изменяем строку `ExecStart`:
```ini
ExecStart=/opt/indexer/target/release/indexer \
    --config /opt/indexer/config.yaml \
    --network mainnet \
    start
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable indexer-mainnet
sudo systemctl start indexer-mainnet
```

---

## 7. Мониторинг

### Health check скрипт

Создайте `/opt/indexer/healthcheck.sh`:

```bash
#!/bin/bash

# Проверяем что сервис запущен
if ! systemctl is-active --quiet indexer; then
    echo "ERROR: Service is not running"
    exit 1
fi

# Проверяем подключение к ClickHouse
if ! clickhouse-client -q "SELECT 1" > /dev/null 2>&1; then
    echo "ERROR: Cannot connect to ClickHouse"
    exit 1
fi

# Проверяем что индексация идёт (блок обновлялся за последние 5 минут)
LAST_UPDATE=$(clickhouse-client -q "SELECT updated_at FROM rexswap.indexer_state FINAL LIMIT 1")
if [ -z "$LAST_UPDATE" ]; then
    echo "WARNING: No indexer state found"
    exit 0
fi

LAST_UPDATE_TS=$(date -d "$LAST_UPDATE" +%s)
NOW_TS=$(date +%s)
DIFF=$((NOW_TS - LAST_UPDATE_TS))

if [ $DIFF -gt 300 ]; then
    echo "WARNING: Last update was $DIFF seconds ago"
    exit 1
fi

echo "OK: Indexer is healthy"
exit 0
```

```bash
chmod +x /opt/indexer/healthcheck.sh
```

### Cron для алертов

```bash
crontab -e
```

```cron
# Проверка каждые 5 минут
*/5 * * * * /opt/indexer/healthcheck.sh || echo "Indexer unhealthy!" | mail -s "Alert" admin@example.com
```

### Мониторинг через ClickHouse

```sql
-- Статус индексера
SELECT 
    network,
    last_synced_block,
    updated_at,
    dateDiff('minute', updated_at, now()) as minutes_since_update
FROM rexswap.indexer_state FINAL;

-- Размер таблиц
SELECT 
    table,
    formatReadableSize(sum(bytes)) as size,
    sum(rows) as rows
FROM system.parts 
WHERE database = 'rexswap' AND active
GROUP BY table
ORDER BY sum(bytes) DESC;

-- Скорость индексации (последние 100 insert'ов)
SELECT 
    toStartOfMinute(event_time) as minute,
    sum(written_rows) as rows_written
FROM system.query_log 
WHERE type = 'QueryFinish' 
  AND query LIKE 'INSERT INTO rexswap.%'
  AND event_time > now() - INTERVAL 1 HOUR
GROUP BY minute
ORDER BY minute DESC
LIMIT 10;
```

---

## 8. Обновление

```bash
# 1. Останавливаем сервис
sudo systemctl stop indexer

# 2. Обновляем код
cd /opt/indexer
git pull

# 3. Пересобираем
cargo build --release

# 4. Обновляем схему БД (если есть новые таблицы)
sudo -u rexswap ./target/release/indexer \
    --config config.yaml \
    --network zilliqa_testnet \
    init-db

# 5. Запускаем
sudo systemctl start indexer

# 6. Проверяем логи
sudo journalctl -u indexer -f
```

---

## 9. Резервное копирование

### ClickHouse backup

```bash
# Установка clickhouse-backup
wget https://github.com/Altinity/clickhouse-backup/releases/download/v2.4.0/clickhouse-backup-linux-amd64.tar.gz
tar -xzf clickhouse-backup-linux-amd64.tar.gz
sudo mv clickhouse-backup /usr/local/bin/

# Создание бэкапа
sudo clickhouse-backup create rexswap_$(date +%Y%m%d)

# Список бэкапов
sudo clickhouse-backup list

# Восстановление
sudo clickhouse-backup restore rexswap_20240101
```

### Простой экспорт таблиц

```bash
# Экспорт
clickhouse-client --query "SELECT * FROM rexswap.swaps FORMAT Native" > swaps_backup.native

# Импорт
clickhouse-client --query "INSERT INTO rexswap.swaps FORMAT Native" < swaps_backup.native
```

---

## 10. Troubleshooting

### Сервис не стартует

```bash
# 1. Проверяем логи
sudo journalctl -u indexer -n 50

# 2. Проверяем права
ls -la /opt/indexer/
ls -la /opt/indexer/.env

# 3. Пробуем запустить вручную
sudo -u rexswap /opt/indexer/target/release/indexer \
    --config /opt/indexer/config.yaml \
    --network zilliqa_testnet \
    start
```

### Нет подключения к ClickHouse

```bash
# Проверяем что ClickHouse запущен
sudo systemctl status clickhouse-server

# Проверяем подключение
clickhouse-client -h localhost -u indexer --password your_password -q "SELECT 1"

# Проверяем порт
netstat -tlnp | grep 8123
```

### Нет подключения к RPC

```bash
# Проверяем RPC
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
  https://dev-api.zilliqa.com

# Если rate limit, используйте другой RPC или свою ноду
```

### Медленная синхронизация

1. Увеличьте `batch_size` в config.yaml (например, до 5000)
2. Используйте RPC с хорошей пропускной способностью
3. Убедитесь что ClickHouse имеет достаточно RAM
4. Проверьте что диск не перегружен: `iostat -x 1`

### Пропущенные блоки

```bash
# Перезапуск с конкретного блока
sudo systemctl stop indexer

sudo -u rexswap /opt/indexer/target/release/indexer \
    --config /opt/indexer/config.yaml \
    --network zilliqa_testnet \
    start --from-block 17820000

# Или просто перезапустите сервис - он продолжит с последнего блока
sudo systemctl start indexer
```

---

## Чеклист развертывания

- [ ] ClickHouse установлен и запущен
- [ ] База данных `rexswap` создана
- [ ] Rust установлен
- [ ] Индексер собран (`cargo build --release`)
- [ ] `config.yaml` настроен
- [ ] `.env` файл создан с секретами
- [ ] Пользователь `rexswap` создан
- [ ] Права на файлы настроены
- [ ] Service файл создан
- [ ] `init-db` выполнен
- [ ] Сервис запущен и работает
- [ ] Логи пишутся
- [ ] Мониторинг настроен (опционально)
