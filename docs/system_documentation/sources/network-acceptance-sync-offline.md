# Наскрізний приймальний сценарій «Мережа магазинів» (гілка sync-offline)

> Дата виконання: 2026-09-06
> Режим: реальний код + реальні тести (cargo test, npm typecheck/lint/build).
> Середовище: локальний PostgreSQL 16.15 (127.0.0.1:5432, суперкористувач postgres),
> тестова БД `pos_system_fresh_test`, ізольований `db_sources.toml` через
> `TORGASHKA_DB_SOURCES`. Фізичний VPS/VPN у середовищі відсутній — Етап 0
> підготовлений як пакет конфігів/скриптів (deploy/network/), готових до
> застосування на реальному сервері.

---

## Крок 1 — Мережева доступність сервера (Етап 0)

**Виконано в цьому середовищі:**
- `bin/facade.rs` — пріоритет адреси: `env TORGASHKA_FACADE_ADDR` →
  `config.toml [server].addr` → дефолт `127.0.0.1:8000`
  (unit-тести `cargo test -p torgashka-api --bin facade` → 4/4 ok).
- Пакет `deploy/network/`: `postgresql.conf.d/torgashka-network.conf`
  (listen_addresses = конкретна VPN-IP, не `*`), `pg_hba.conf.torgashka-network`
  (hostssl + scram-sha-256, лише VPN-підмережа), `firewall.sh`
  (8000/5432 лише з VPN), `vpn-setup.sh` (Tailscale/ZeroTier),
  `torgashka-facade.service` (systemd, `TORGASHKA_FACADE_ADDR=<vpn-ip>:8000`),
  `README.md` з кроками. `bash -n` обох скриптів — OK.

**Лишилось на реальне середовище (позначено в deploy/network/README.md):**
фізичний VPS, реальний підйом VPN-тунелю, `systemctl enable` PostgreSQL-конфігів
і фасаду, запуск firewall на реальному хості, перевірка
`curl http://<vpn-ip>:8000/api/v1/health` → 200 з точки / timeout ззовні.

---

## Крок 2 — Provisioning централізованої БД (Етап 1)

**Endpoint:** `POST /api/v1/admin/db-sources/provision` (owner-only;
admin/store_manager → 403).

**Що робить:** CREATE DATABASE `<name>` TEMPLATE template0 → накатує повну
схему (SCHEMA_SQL — єдине джерело, без дублів) → створює/оновлює роль
`torgashka_app` (NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE) з
автогенерованим паролем + GRANT на нову БД → зберігає джерело у
`db_sources.toml` (пароль лише AES-256-GCM) зі статусом
`provisioned_pending_activation`. Активація — окремим підтвердженням власника
(існуючий `POST /:id/activate`, stability_first: застосування після рестарту).

**Тест:** `cargo test -p torgashka-api --test provision_e2e` → 2/2 ok
(повний lifecycle owner + невалідне ім'я БД → 400). У тесті перевірено: нова БД
з повною схемою на цільовому сервері, порожній `stores`, `POST /:id/test`
проходить під роллю `torgashka_app`, повторний provision → 409, cashier → 403,
суперкредити не збережені/не залоговані.

---

## Крок 3 — Джерела даних: UI «Створити нову БД» (Етап 2)

**UI:** `frontend/src/pages/settings/DataSourcePage.tsx` — два таби
«Підключити наявну» (старий сценарій без змін) / «Створити нову БД» (provision).
Рядки зі статусом `provisioned_pending_activation` отримують бейдж
«створено, не активовано» + нагадування про перезапуск сервісу при активації.

**Видалення точки:** `DELETE /api/v1/admin/stores/:id` (owner-only) — лише
порожня точка (перевірка 23 таблиць; 409 з іменем першої таблиці з даними);
кнопка «Видалити» у StoresPage для role=owner.
Тест: `store_delete_e2e` → ok (204 чиста / 409 зі stock / 403 admin·cashier / 404 / 400).

**Фронт-верифікація:** `npm run typecheck`, `npm run lint`, `npm run build:admin` — чисті.

---

## Крок 4 — Майстер «Додати магазин у мережу» + конфіг-файл (Етап 3, обов'язкова вимога)

**Backend:**
- `POST /api/v1/admin/network-config/export` (owner) → `{ filename, content }`,
  content = JSON schema v1: `network_id`, `server_url`, `store {id,name,activation_code}`,
  `db {host,port,database,user,password_encrypted?}` (пароль лише за
  `include_db_password=true`, завжди AES-256-GCM, ніколи plaintext).
- `POST /api/v1/admin/network-config/import` (owner) → валідація та прийом.
- Тест: `network_config_e2e` → ok (export/import lifecycle, відсутність/наявність
  password_encrypted, підроблений код → 4xx, cashier/admin → 403).

**Frontend:**
- StoresPage: «Експорт конфігурації» (owner) → модалка «Адреса сервера для точок
  (VPN-IP:8000)» → download JSON-файлу (придатний для USB/файлообмінника).
- DeviceSyncPage («Мережева каса»): при неактивному пристрої — вибір
  «Код активації» (fallback, без змін) або «Файл конфігурації мережі»:
  локальний імпорт файлу → автозаповнення server_url + code + назва точки →
  існуюча серверна активація (`/devices/activate`) → persistSyncDevice →
  Rust-клієнт запускає фонові синки (авто-pull локальної SQLite, since_version=0).

---

## Крок 5 — Регресійна ізоляція per-store (Етап 4, QA)

**Новий тест:** `cargo test -p torgashka-api --test per_store_isolation_e2e` → 3/3 ok:
1. `pull_isolates_store_scoped_and_global_rows` — точка A на pull не отримує
   stock/ціни точки B (і навпаки);
2. `soft_delete_reaches_only_owner_store` — is_deleted доходить лише до точки-власника;
3. `five_stores_concurrent_push_no_conflicts_isolated` — 5 точок, паралельні push,
   без конфліктів запису, дані не перехрещуються.

**Регресія суміжних серій:** `network_device_sync_e2e` → ok;
`sym_4stores_outage_e2e` → ok. Сценарії 1–14 §7.6
(`docs/design/multi-store-and-cash-operations.md`) покриті наявними серіями
(onboarding/network/device-sync/pull/push), які залишаються зеленими.

---

## Крок 6 — Безпека (Етап 5)

- provision/export/import network-config — лише `role=owner`
  (`auth_routes::require_owner`, перевірено тестами: admin/cashier → 403).
- Суперкористувацькі кредити provision — одноразові: не зберігаються,
  не логуються, не повертаються; пароль ролі — лише `password_encrypted`
  (AES-256-GCM, ключ `TORGASHKA_DBKEY`/`.dbkey`, 0600).
- Для не-localhost host у провіжинінгу URL містить `sslmode=require`.
- Конфіг-файл мережі не містить plaintext-паролів; передача поза мережею
  (USB/файлообмінник) безпечна лише за умови довіреного каналу доставки —
  це зазначено в UI та документації.
- `deploy/network/` — TLS/VPN-розгортання (hostssl + scram-sha-256, firewall
  лише з VPN; HTTPS reverse-proxy для 8000 описано в README як опцію без VPN).
- `db_sources.toml` — права 0600 (перевірено в db_sources e2e).

---

## Підсумок за критерієм прийняття

| Критерій | Статус |
|---|---|
| cargo test torgashka-api (нові серії: provision_e2e, admin_db_sources_e2e, network_config_e2e, store_delete_e2e, per_store_isolation_e2e) | ✅ зелені |
| cargo test torgashka-infrastructure (lib) | ✅ 160/160 (2 ignored) |
| Фронтові перевірки (typecheck/lint/build:admin) | ✅ чисті |
| Ендпоінти provision та network-config існують і покриті тестами | ✅ |
| Наскрізний сценарій задокументовано результатом | ✅ (цей файл) |

> ⚠️ Відоме обмеження середовища (НЕ регресія, підтверджено на чистому HEAD):
> інтеграційні тести infra, що залежать від фіксованих фікстур даних
> (`cash_operations` на `pos_system_fresh_test`, `pos_crud` на
> `pos_system_ci_test`), падають через відсутність відповідного сиду
> («ФОП Мельничук», товари/залишки) у цих БД. На БД з правильними фікстурами
> (напр. `pos_system_ci_test` для `cash_operations`) — зелені.
