# Torgashka — Мережеве розгортання сервера мережі магазинів (ЕТАП 0)

> Гілка `sync-offline`. Пакет конфігів/скриптів для розгортання на фізичному
> VPS/VPN-сервері. **У цьому середовищі фізичного сервера НЕМАЄ** — пакет
> готовий до застосування; що лишилось на реальне середовище — у кінці файлу.

## Архітектура цільового розгортання

```
[Магазин N: каса Torgashka]  ──VPN──▶  [VPS-сервер]
                                        ├── torgashka-facade (Rust, :8000, слухає VPN-IP)
                                        ├── PostgreSQL 16 (:5432, слухає VPN-IP, hostssl)
                                        └── firewall: 8000+5432 ЛИШЕ з VPN-підмережі
```

Принципи безпеки:
- PostgreSQL і фасад **ніколи не слухають публічний інтерфейс** (`listen_addresses` = VPN-IP, не `*`).
- Віддалений доступ до БД — **тільки hostssl** (TLS) + `scram-sha-256`.
- Firewall пропускає порти **лише з VPN-підмережі**.
- Секрети — у `/etc/torgashka/facade.env` (chmod 600), не в unit-файлі.

## Вміст пакета

| Файл | Призначення |
|---|---|
| `postgresql.conf.d/torgashka-network.conf` | drop-in конфіг PG16 (listen VPN-IP, ssl=on) |
| `pg_hba.conf.torgashka-network` | зразок pg_hba: local trust(postgres) + hostssl(VPN) |
| `firewall.sh` | ufw/nft: 8000+5432 лише з VPN, решта deny (ідемпотентний) |
| `vpn-setup.sh` | інтерактивний підйом Tailscale або ZeroTier |
| `torgashka-facade.service` | systemd unit фасаду (VPN-IP:8000, hardening) |

---

## Покрокова інструкція реального розгортання

### Крок 1. Встановити PostgreSQL 16

```bash
# Debian/Ubuntu (офіційний репозиторій PG):
sudo apt install -y postgresql-common
sudo /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y
sudo apt install -y postgresql-16
systemctl status postgresql        # → active (running), слухає 127.0.0.1:5432
```

### Крок 2. Застосувати конфіги PostgreSQL

```bash
# 2.1. Drop-in конфіг (СПОЧАТКУ замініть VPN_IP_placeholder на реальний VPN-IP!)
sudo cp deploy/network/postgresql.conf.d/torgashka-network.conf \
    /etc/postgresql/16/main/conf.d/torgashka-network.conf
sudo chown postgres:postgres /etc/postgresql/16/main/conf.d/torgashka-network.conf

# 2.2. pg_hba (зразок → реальний файл)
sudo cp deploy/network/pg_hba.conf.torgashka-network \
    /etc/postgresql/16/main/pg_hba.conf
sudo chown postgres:postgres /etc/postgresql/16/main/pg_hba.conf
sudo chmod 640 /etc/postgresql/16/main/pg_hba.conf

# 2.3. Перевірка синтаксису + рестарт
sudo -u postgres /usr/lib/postgresql/16/bin/postgres \
    -D /var/lib/postgresql/16/main -C listen_addresses   # очікуємо VPN-IP
sudo systemctl restart postgresql
sudo -u postgres psql -c "SHOW listen_addresses; SHOW ssl; SHOW port;"
#   listen_addresses = <VPN-IP>   |   ssl = on   |   port = 5432
sudo -u postgres psql -c "SELECT type,address,auth_method FROM pg_hba_file_rules;"
```

> Створіть роль `torgashka_app` з паролем (для роботи фасаду):
> `sudo -u postgres psql -c "CREATE ROLE torgashka_app LOGIN PASSWORD '<надійний>';"`
> (або через `scripts/create_app_role.sql` — див. `config.toml.example`.)

### Крок 3. Firewall

```bash
# Спочатку підніміть VPN (крок 4) — firewall потребує знати VPN-підмережу.
# За замовчуванням у скрипті Tailscale CGNAT 100.64.0.0/10 — змініть за потреби.
sudo ./deploy/network/firewall.sh          # застосувати (ufw або nft)
sudo ./deploy/network/firewall.sh status   # перевірити стан
```

### Крок 4. VPN (Tailscale або ZeroTier)

```bash
sudo ./deploy/network/vpn-setup.sh
# Вибір 1 (Tailscale):  tailscale up → авторизація за URL (або --authkey)
# Вибір 2 (ZeroTier):   ввести Network ID → авторизувати сервер в my.zerotier.com
# Результат: VPN-IP сервера (записати!):
tailscale ip -4            # або: zerotier-cli info
```

Після отримання VPN-IP поверніться до кроку 2 і підставте його в
`torgashka-network.conf` (listen_addresses) та кроку 5 (unit фасаду).

### Крок 5. Systemd-сервіс фасаду

```bash
# 5.1. Секрети (файл читає unit через EnvironmentFile):
sudo mkdir -p /etc/torgashka
sudo tee /etc/torgashka/facade.env >/dev/null <<'EOF'
TORGASHKA_DBKEY=<base64 43 симв. або hex 64 симв.>
TORGASHKA_DB_SOURCES=/etc/torgashka/db_sources.toml
TORGASHKA_CONFIG=/etc/torgashka/config.toml
EOF
sudo chown root:root /etc/torgashka/facade.env && sudo chmod 600 /etc/torgashka/facade.env

# 5.2. Бінарник фасаду (приклад збірки + встановлення):
#   cd frontend/src-tauri && cargo build --release -p torgashka-api --bin facade
#   sudo install -m 755 target/release/facade /opt/torgashka/bin/facade

# 5.3. Unit: замініть <VPN-IP> у файлі, потім:
sudo cp deploy/network/torgashka-facade.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now torgashka-facade
systemctl status torgashka-facade          # → active (running)
journalctl -u torgashka-facade -n 50       # лог старту
```

### Крок 6. Перевірка критерію

**З точки всередині VPN (магазин / dev-машина):**

```bash
curl -sS -o /dev/null -w '%{http_code}\n' http://<vpn-ip>:8000/api/v1/health
# очікуємо: 200

# Перевірка TLS до PostgreSQL з точки:
psql "host=<vpn-ip> port=5432 dbname=pos_system user=torgashka_app sslmode=require" -c "SELECT 1;"
# очікуємо: 1
```

**Ззовні (без VPN, з публічного інтернету):**

```bash
curl -sS -m 5 http://<public-ip>:8000/api/v1/health
# очікуємо: timeout / connection refused (firewall блокує) — це УСПІХ
curl -sS -m 5 telnet://<public-ip>:5432    # аналогічно: timeout
```

Критерій прийняття Етапу 0: **з VPN → 200, ззовні → timeout**.

---

## ЛИШИЛОСЬ НА РЕАЛЬНЕ СЕРЕДОВИЩЕ (не можна виконати тут)

1. **Фізичний VPS/VPN-сервер** з публічним IP і доступом root.
2. **Реальний підйом VPN**: `tailscale up` (авторизація в акаунті) або ZeroTier
   (Network ID + авторизація в my.zerotier.com) — потрібен зовнішній акаунт.
3. **Підстановка реальних значень** у плейсхолдери:
   - `VPN_IP_placeholder` у `postgresql.conf.d/torgashka-network.conf`;
   - `<VPN-IP>` у `torgashka-facade.service`;
   - шлях бінарника `/opt/torgashka/bin/facade` (потрібен `cargo build --release`).
4. **`systemctl enable --now torgashka-facade` + postgresql** на реальному хості.
5. **Firewall на реальному хості** (`sudo ./firewall.sh` — потребує root/ufw/nft).
6. **Реальні сертифікати TLS** для PostgreSQL (замість snakeoil) і CA для клієнтів.
7. **Генерація секретів**: `TORGASHKA_DBKEY`, пароль `torgashka_app`, `db_sources.toml`.
8. **Перевірка критерію з реальної точки у VPN і ззовні** (крок 6).
