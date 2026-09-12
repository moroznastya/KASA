# MANIFEST — знімок актуальної БД хаба

**Знімок створено (ISO):** 2026-09-12T14:43:08+03:00
**Дата у назві дампа:** 20260912
**Джерело:** PostgreSQL кластер `/home/anastasia/torgashka_pg_primary`, `127.0.0.1:5544`, БД `pos_system_fresh`, користувач `postgres`
**Клієнт pg_dump:** `/usr/lib/postgresql/17/bin/pg_dump` (PostgreSQL 17.6) — використано саме 17.x: системний `/usr/bin/pg_dump` = 16.15 і не дампив би сервер 17.6 (newer server major).

## Версія сервера джерела

```
select version();
PostgreSQL 17.6 (Ubuntu 17.6-1.pgdg24.04+1) on x86_64-pc-linux-gnu, compiled by gcc (Ubuntu 13.3.0-6ubuntu2~24.04) 13.3.0, 64-bit

SHOW server_version;
17.6 (Ubuntu 17.6-1.pgdg24.04+1)
```

## Версія схеми (Alembic)

```
SELECT version_num FROM alembic_version;
0014
```

## Артефакт

| Поле | Значення |
|---|---|
| Файл | `artifacts/hub_snapshot/pos_system_fresh_20260912.dump` |
| Формат | `pg_dump -Fc` (custom, gzip), `--no-owner --no-privileges` |
| Розмір | 716167 байт (`ls -l`: `-rw-rw-r-- 1 anastasia anastasia 716167 вер 12 14:43`) |
| sha256 | `3d2d17c2f1592dccd39d824da047cbf5c07f9e4ac50a19a7fade75616fabb25a` |
| TOC entries | 434 |
| Dump Version | 1.16-0 |
| Таблиць у public | 46 |
| Розширення | `dblink`, `pg_trgm`, `plpgsql`, `uuid-ossp` |

## Row counts у ДЖЕРЕЛІ (фактичні, `select count(*)`)

| Таблиця | Count у джерелі | Очікувано контрактом | Збіг |
|---|---|---|---|
| users | 4 | 4 | ✅ |
| products | 4409 | 4409 | ✅ |
| receipts | 264 | ≥264 | ✅ |
| invoices | 3 | — | — |
| stores | 35 | 35 | ✅ |
| audit_log | 13 | — | — |

## Перевірка цілісності архіву

```
$ pg_restore -l pos_system_fresh_20260912.dump | head
;     Archive created at 2026-09-12 14:43:05 EEST
;     dbname: pos_system_fresh
;     TOC Entries: 434
;     Compression: gzip
;     Dump Version: 1.16-0
;     Format: CUSTOM
;     Integer: 4 bytes
;     Offset: 8 bytes
;     Dumped from database version: 17.6 (Ubuntu 17.6-1.pgdg24.04+1)
pg_restore -l exit=0
```

## Чого у знімку НЕМАЄ (перевірено в джерелі, не припущено)

- **Міграції 0015/0016 не застосовані як міграції** — `alembic_version = 0014`.
- `invoices.client_uuid` та індексу `uq_invoices_client_uuid` у знімку **НЕМАЄ**:
  ```
  SELECT count(*) FROM information_schema.columns
   WHERE table_name='invoices' AND column_name='client_uuid';  → 0
  ```
- Ідемпотентність push (0013) присутня — `client_uuid` + partial UNIQUE на 9 таблицях:
  `debtor_payments, inventories, purchase_orders, receipts, return_invoices, sync_log, transfers, work_sessions, write_offs`
  (`uq_*_client_uuid`, усі з `WHERE client_uuid IS NOT NULL`). Саме `invoices` — прогалина, яку закриває 0016.
- **Дрейф 0015 присутній і на хабі:** `stores.legal_name`, `stores.edrpou` уже існують при `alembic_version = 0014` → `alembic upgrade head` на відновленій з цього знімка БД впаде на 0015 (`DuplicateColumnError`) так само, як на 5432-інстансі.
- Володарів (owner) та привілеїв — зняті прапорцями `--no-owner --no-privileges` (навмисно).
