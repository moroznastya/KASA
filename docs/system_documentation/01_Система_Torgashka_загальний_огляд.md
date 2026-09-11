# 01. Torgashka — Загальний огляд системи

> Верифіковано за кодом репозиторію станом на 2026-09-06.

## 1. Що це

**Torgashka POS** — касово-облікова система (каса / POS / ПРРО) для роздрібної
торгівлі України. Ключові властивості:

- **Фіскальна касовий апарат**: реєстрація продажів, ПРРО (програмний РРО),
  фіскалізація через ДПС (gRPC-протокол), офлайн-режим до **168 годин** за законом.
- **Товарооблік**: товари, штрих-коди, категорії, постачальники, залишки,
  документи (прибуткові накладні, переміщення, списання, повернення
  постачальнику, замовлення, інвентаризації).
- **Продажі**: чеки (готівка/картка/змішано, борг, повернення, термінальні
  реквізити), робочі зміни, боржники (книга боргу).
- **Взаєморозрахунки**: журнал постачальників (supplier ledger).
- **Мережа магазинів**: один власник — багато торговельних точок, ролі на
  рівні точки, міжточкова наявність, звітність мережі, касові операції
  (інкасація/внесення), централізоване адміністрування кас.
- **Offline-first**: кожна каса може працювати без інтернету; дані
  синхронізуються з сервером (pull довідників + push транзакцій).
- **Простий запуск**: некваліфікований користувач ставить `.deb`/`.exe`,
  БД піднімається автоматично (embedded PostgreSQL) або каса підключається
  до сервера мережі.

## 2. Технологічний стек (продакшн)

| Шар | Технологія |
|-----|-----------|
| Desktop-оболонка | **Tauri v2** (Rust) — бінарник `torgashka-pos`/`kasa-pos` |
| Frontend | **React 18 + TypeScript + Vite + TailwindCSS v4**, Zustand, TanStack Query, axios |
| Backend | **100% Rust**: axum HTTP-фасад, вбудований у Tauri-бінарник, `127.0.0.1:8000` |
| БД (сервер/власник) | **PostgreSQL 15–17** (схема: `frontend/src-tauri/crates/torgashka-infrastructure/src/schema.sql`) |
| БД (офлайн каса) | **SQLite** (`offline.db`, ~/.local/share/torgashka/offline.db) |
| БД (single-store desktop) | **Embedded PostgreSQL** (`embedded_pg.rs`, initdb-кластер у data_dir застосунку) |
| Фіскалізація | `torgashka-prro` — gRPC з TLS (JKS keystore), XML-чеки, MAC |
| OCR накладних | `torgashka-ocr` — Gemini API для розпізнавання прибуткових накладних |
| Мови | Rust (prod), Python/FastAPI (legacy-еталон, дезактивований) |

## 3. Архітектура (шари)

```
┌───────────────────────────────────────────────────────────────┐
│ PRESENTATION: React SPA (src/)  ──  Tauri webview/браузер     │
│   axios → http://127.0.0.1:8000/api/v1 (або /api/v1 через dev-проксі) │
├───────────────────────────────────────────────────────────────┤
│ RUST-ФАСАД (презентаційний шар API):                          │
│   crates/torgashka-api/  — axum-роутер /api/v1, /api/v2       │
│     auth_middleware (JWT), store_middleware (X-Store-Id→RLS), │
│     fallback 410 Gone для legacy-шляхів                       │
├───────────────────────────────────────────────────────────────┤
│ APPLICATION: crates/torgashka-application/ (use cases, DTO)   │
├───────────────────────────────────────────────────────────────┤
│ DOMAIN: crates/torgashka-domain/ (entities, VO, rules,        │
│   auth.rs, pos.rs, products_v2.rs, invoices.rs, ledger.rs…)   │
├───────────────────────────────────────────────────────────────┤
│ INFRASTRUCTURE: crates/torgashka-infrastructure/              │
│   db.rs, db_sources.rs, embedded_pg.rs, provision.rs,         │
│   store_ctx.rs (StorePool/StoreCtx), repositories/,           │
│   offline/ (SQLite + sync push/pull + snapshots + stock),     │
│   devices/, cash_drawer/, terminal/, print/, prro/, ocr.rs    │
├───────────────────────────────────────────────────────────────┤
│ LEGACY (НЕ runtime): backend/ — Python FastAPI, еталон для    │
│   differential-тестів, історія Alembic-міграцій               │
└───────────────────────────────────────────────────────────────┘
```

**Продакшн-шлях даних:** React → axios → Rust-фасад axum (:8000) →
`StorePool` (з RLS-контекстом `app.store_id`/`app.user_id`) → PostgreSQL.
В offline: React → Tauri invoke → SQLite (outbox) → фоновий sync → сервер.

## 4. Склад Rust-крейтів

| Крейт | Роль | Ключові файли |
|-------|------|---------------|
| `torgashka-api` | HTTP-фасад (axum), авторизація, всі роути v1/v2 | `router_v1.rs`, `auth*.rs`, `admin*.rs`, `network.rs`, `stores.rs`, `store_context.rs`, `sync.rs`, `pos.rs`, `crud.rs`, `invoices.rs`, `documents.rs`, `debtors.rs`, `ledger.rs`, `prro.rs`, `ocr.rs`, `proxy.rs` |
| `torgashka-application` | Use cases, DTO | `services/`, `lib.rs` |
| `torgashka-domain` | Доменні правила, DTO, сервісні трейти | `auth.rs`, `pos.rs`, `invoices.rs`, `ledger.rs`, `debtors.rs`, `purchase_orders.rs`, `documents.rs`, `print.rs`, `repos.rs`, `settings.rs`, `dto.rs` |
| `torgashka-infrastructure` | PostgreSQL, SQLite-offline, provisioning, embedded-PG, репозиторії | `schema.sql`, `provision.rs`, `db_sources.rs`, `embedded_pg.rs`, `store_ctx.rs`, `repositories/*`, `offline/*` |
| `torgashka-ocr` | Розпізнавання накладних (Gemini) | `gemini.rs`, `ocr_service.rs` |
| `torgashka-prro` | Фіскалізація ПРРО (gRPC→ДПС, XML, MAC, keystore) | `grpc.rs`, `crypto/`, `xml.rs`, `prro/` |

## 5. Ключові папки проєкту

```
Torgashka/
├── backend/            # LEGACY Python (FastAPI, Alembic-історія, differential-тести)
├── frontend/
│   ├── src/            # React (pages/, components/, services/, store/, hooks/, types/)
│   ├── dist/, dist-admin/   # збірки: desktop-застосунок + веб-адмінка
│   └── src-tauri/      # Tauri + Rust-крейти torgashka-*; migrations/ (порожня — міграції в крейтах)
├── docs/               # ADR, дизайни, аудити, ПРРО, архітектура
├── deploy/, docker/, flatpak/, scripts/, tools/, tests/, certs/
├── Categories/         # CSV-імпорти категорій (УКТЗЕД/наповнення)
├── docker-compose.yml  # PostgreSQL 16 + (профіль legacy backend)
├── STRUCTURE.md, SYSTEM_STATE.md   # ⚠️ історичні (kasa-*, 17 таблиць)
└── README.md, ROADMAP.md
```

## 6. Модель розгортання (3 сценарії)

1. **Desktop single-store (малий магазин, 1 каса):**
   Tauri-бінарник + embedded PostgreSQL (локальний initdb-кластер у data dir);
   онбординг 4 кроки; даних вистачає для однієї точки. Власник = admin = касир
   в одній особі.

2. **Серверний (мережа точок одного власника):**
   центральний сервер із PostgreSQL (`db_sources.toml` / UI «Джерела даних»),
   provisioning створює БД власника і роль `torgashka_app`; кожна каса
   активується кодом точки (`devices/activate`), тримає локальний SQLite
   offline.db і синхронізується (`/api/v1/sync/master` — pull довідників,
   `/api/v1/sync/push` — push транзакцій).

3. **SaaS/кілька власників (потенціал):**
   database-per-owner: мета-БД `public.owners_db` зіставляє `owner_id → db_name`;
   на старті проєктується «пул пулів» з маршрутизацією до БД власника
   (деталі — в `sources/database-architecture-decision.md`, розділи B3/§9).

## 7. Поточний стан (звірено з кодом)

| Компонент | Стан |
|-----------|------|
| Rust-фасад axum :8000 | ✅ працює, ~190+ роутів v1/v2 (router_v1.rs, 998 рядків) |
| React frontend | ✅ desktop + адмінка |
| PostgreSQL (сервер/embedded) | ✅ |
| Offline SQLite + sync | ✅ реалізовано (outbox, sync_log, майстер-дельти) |
| Мультиточковість (мережа) | ✅ реалізовано: stores/user_stores/RLS/devices/activation/admin |
| RLS реальне форсування (`FORCE ROW LEVEL SECURITY` + роль `torgashka_app`) | 🟠 за планом етапу 7 (див. `sources/database-architecture-implementation-plan.md`) |
| Маршрутизація «пул пулів» до БД власника | 🟠 план етапу 9 |
| Python-бекенд | ❌ дезактивований (еталон) |
| Differential-тести Rust vs Python | ⏳ частково |

*Суперечності зі `SYSTEM_STATE.md` пояснені в `README.md` цієї папки.*

## 8. Хто відповідає за що (delegation-карта)

| Домен | Агент |
|-------|-------|
| Rust-фасад/крейти | Rust_Agent |
| PostgreSQL/схеми/міграції | DB_Admin_Agent |
| React/UI | React_UI_UX_Agent |
| Tauri desktop | Tauri_Agent |
| ПРРО/OCR | apiarm_agent / Rust_Agent |
| Тести/аудит | QA_Agent, Test Helper Agent |
