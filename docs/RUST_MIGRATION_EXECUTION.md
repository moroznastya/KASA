# Виконавчий план міграції Kasa POS → Rust

> Джерело стратегії: `docs/RUST_MIGRATION_PLAN.md` (v1.0, затверджений)
> Виконавчий контроль: NIKO (координація, моніторинг)
> Створено: 2026-08-07 | Оновлено: 2026-08-07 (ЕТАП 7 ЗАВЕРШЕНО)

## 0. Статус етапів

| Етап | Назва | Ведучий | Статус | DoD |
|---|---|---|---|---|
| 0 | Фундамент (workspace, axum-фасад, sidecar, CI) | Rust_Agent + Tauri_Agent | ✅ ЗАВЕРШЕНО (0.1–0.5) | workspace збирається; :8000+проксі :8001; фронт без змін; cargo test зелений |
| 1 | Довідники read (products/categories/suppliers GET) | Rust_Agent | ✅ ЗАВЕРШЕНО | Rust==Python 20/20, 50/50, 50/50; feature-flag KASA_RUST_READDIRS; відкат працює |
| 2 | Довідники CRUD + inventory | Rust_Agent | ✅ ЗАВЕРШЕНО | E2E 16/16; конкурентність 2 confirm → stock 104.000; валідація 1:1 |
| 3 | POS: чеки v2, робочі сесії, списання, переміщення, зміни ПРРО (X/Z) | Rust_Agent | ✅ ЗАВЕРШЕНО | E2E POS 43/43; конкурентність 2 sale → stock 86.000; транзакційність (помилка → rollback); X/Z без ПРРО 1:1 |
| 3b | Документи (invoices, purchase_orders, return_invoices) | Python_Backend_Agent + Rust_Agent | ⏳ | статуси 1:1; ledger ідентичний; офлайн-синхронізація |
| 4 | Ledger (журнал взаєморозрахунків) v1+v2 | Rust_Agent | ✅ ЗАВЕРШЕНО | differential 10 100 записів 1:1; 101 сторінка GET 1:1; валідації 404/400/422/500 1:1; конкурентність/транзакційність |
| 5 | Receipts + друк (open→pay→close, офлайн-черга) | Tauri_Agent + Rust_Agent | ✅ ЗАВЕРШЕНО | ESC/POS друк у мок-пристрій (19227 байт, ESC @ + GS v 0 + GS V); офлайн-черга SQLite на диск + персистентність + синхронізація; Python print-роути 410 |
| 6 | Auth / users / settings / RBAC | Rust_Agent | ✅ ЗАВЕРШЕНО | E2E AUTH DIFF 59/59; JWT крос-валідний (Rust↔Python, той самий секрет); RBAC admin/cashier 1:1; feature-flag KASA_RUST_AUTH (відкат перевірено); валідації 401/400/403/404/409/422 1:1 |
| 7 | ПРРО (gRPC/tonic, crypto, xml, offline_queue, shift) | Rust_Agent + apiarm_agent | ✅ | **7.1 фундамент ✅**; **7.2 крипто ✅** (XAdES golden 5/5 байт-ідентично; CAdES ДСТУ 4145 FFI); **7.3 ✅** (offline_queue 1:1, shift open/close 1:1, sync replay, facade KASA_RUST_PRRO) |: ADR-014 (крипто-стратегія FFI→IIT SDK); gRPC-клієнт tonic+prost (TLS READY, ping status -1 1:1 Python); XML СЗЗД golden parity 12/12; JKS читається (ДСТУ 4145). 7.2: XAdES/CAdES підпис; 7.3: offline_queue+shift |
| 8 | Дезактивація Python | Tauri_Agent + Git Admin Agent | ⏳ **БЛОКОВАНО** (залежність: план-етап 3 «Документи» = журнал 3b ⏳ не мігрований) | Rust НЕ покриває 6 груп роутів, які активно використовує фронтенд: documents/invoices/return-invoices/purchase-orders, debtors, print (цінники/етикетки), print-templates, products images/barcodes, prro v2 (test-connection/fiscalize). Повна дезактивація = втрата функціоналу. Див. Аномалія 5 |

Паралельно з етапом 1: QA_Agent — тестовий контур (differential/golden/proptest).
Паралельно з етапом 5: ПРРО-дослідження (JKS→PKCS12/PEM, FFI vs gRPC).

## 1. Критичний шлях

```
Етап 0 ✅ → 1 ✅ → 2 ✅ → 3 ✅ → 4 ✅ → 5 ✅ → 6 ✅ → 7 ✅ → 8
```

## 2. Журнал делегувань (етап 0)

| # | Контракт | Агент | Коміт | Статус | Верифікація NIKO |
|---|---|---|---|---|---|
| 0.1 | Workspace + рефакторинг 3 916→455 LOC у крейти | Rust_Agent | 1a63dae | ✅ | build/test/clippy чисті; 28 тестів збережено |
| 0.2 | axum-фасад :8000 + проксі :8001 + JWT + diff CLI | Rust_Agent | 87cc1e1, b4778cd | ✅ | health 200; 401 без JWT; 503 при Python down; 200 при Python up |
| 0.3 | Python sidecar → :8001 + CORS | Tauri_Agent | 002e725 | ✅ | /health 200, /docs 200 на :8001 |
| 0.4 | CI rust-core.yml + push гілки | Git Admin Agent | 81e5b12 | ⚠️ ЧАСТКОВО | workflow валідний; гілка в origin; **PR не відкрито — gh не автентифікований** |
| 0.5 | Differential CLI | Rust_Agent | (в 0.2) | ✅ | echo-op працює: {"op":"echo","ok":true} |
| 1.1 | Репозиторії read довідників + роути GET + snapshot-тести | Rust_Agent | c71f97c, f0d6db8, 21d9467 | ✅ | Rust==Python 20/20, 50/50, 50/50; flag KASA_RUST_READDIRS=1; без флага проксі ідентичний; cargo test/clippy/fmt чисті |
| 4.1 | Ledger v1+v2: порти, SQL-репозиторії, роути під flag, E2E differential 10 100 записів | Rust_Agent | 08d0c34, 20bc65d, 24ec542, 0ed4d70, ee365c8, 9ef3931, c35a03e | ✅ | E2E ledger: 10 100 записів (10k Rust + 100 Python), v2 GET entries 101 сторінка 1:1, v1/v2 balance, balances, валідації 404/400/422/500 1:1; конкурентність 2 паралельні POST → 201/201 без втрат; транзакційність: 400 не створив запис; cargo test 58/58, clippy/fmt чисті; тестові дані E4-/E4T- видалені (перевірено psql)
| 5.1 | Друк чеків open→pay→close (ESC/POS → мок-пристрій) + офлайн-черга (SQLite на диск, персистентність, синхронізація) | Tauri_Agent | f009579 | ✅ | e2e_stage5_tauri.sh: друк 19227 байт ESC/POS (ESC @ / GS v 0 / 2×подача / GS V); офлайн: save→count=1→перезапуск процесу count=1 (персистентність)→sync POST sale 201→count=0→чек знайдено в backend; фінальний health 200; тестові дані видалені; fmt виправлено NIKO (494766a) |
| 6.1 | Auth/users/settings/RBAC: порти kasa-domain (AuthService, DTO, валідатор settings), SQL-репозиторій SqlxAuth (login/login-pin/refresh/logout, users CRUD, permissions, hourly-rate, settings), фасад kasa-application, роути kasa-api під KASA_RUST_AUTH, JWT create (HS256, той самий секрет), E2E differential | Rust_Agent | 000d238 | ✅ | E2E AUTH DIFF 59/59 зелений; JWT parity: токени Rust↔Python крос-валідні (verify/refresh обидва напрямки, claims 1:1: access {sub,role,permissions,type,iat,exp}, refresh без permissions); RBAC 401/403 1:1; валідації 401/400/403/404/409/422 1:1; feature-flag: KASA_RUST_AUTH=1 → Rust, =0 → проксі Python (перевірено); cargo test 69/69, clippy/fmt чисті; БД почищена |
| 7.1 | ПРРО-фундамент: ADR-014 (крипто-стратегія FFI→IIT SDK EUSignCP через libloading, ДСТУ 4145; чистий Rust для RSA/ECDSA); crate kasa-prro (tonic+prost build.rs з prro.proto, TLS native roots+кастомний CA, дедлайни+ретраї 3×1s→2s); gRPC-клієнт ChkIncomeService (sendChkV2/ping/statusRro/infoRro/lastChk/delLastChk/delLastChkId/open_shift); XML СЗЗД 2.1.7 (C14N, MAC SHA-256→Base64, чек/Z/службові 108-112) — golden parity з Python; key_store JKS (власний парсер + JavaSoft XOR/SHA1-keystream) / PKCS#12 (openssl) / PEM; crypto::iit FFI (10 extern C-сигнатур) | Rust_Agent | a13fd35 | ✅ | E2E: TLS READY до cabinet.tax.gov.ua:9443, ping (0x7FFFFFFF, SERVICECHK) → status -1 ERROR_VEREFY, error_message ідентичний Python; golden XML 12/12 байт-ідентично (v1-v7 + MAC + message + canonical); JKS pb_3791505547: приватний ключ 802B PKCS#8 + OID ДСТУ 4145 + ланцюг 4 сертифікати, підписант serial=5E984D52... (1:1 Python); cargo test 33/33, clippy 0, fmt чистий |
| 7.2 | XAdES/CAdES крипто-шар: crypto::xades (чистий Rust) — XAdES-BES enveloped (C14N 1.1 inclusive 1:1 libxml2, RSA PKCS#1 v1.5 детермінований, ECDSA P-256/P-384); crypto::iit FFI завершено — load_jks_key (EUGetJKSPrivateKeyFile→EUSaveCertificate×N→EUReadPrivateKeyBinary), CAdES-BES ContentInfo/signedData (EUSignDataInternal/EUVerifyDataInternal), get_signer_serial/name (X.509); фабрика signer_from_key_material (OID ДСТУ 4145 → IitSigner, RSA/EC → XadesSigner) | Rust_Agent | 03373be | ✅ | GOLDEN XAdES 5/5: Rust sign == Python signxml БАЙТ-В-БАЙТ (чек/Z/службові, digest+signature value збігаються); CAdES: Rust verify Python sig ✅, Python verify Rust sig ✅, структура ContentInfo OID 1.2.840.113549.1.7.2; get_serial_number 1:1 (RSA 7AED62...; ДСТУ 5E984D52...), get_signer_name 1:1 (Тестовий Підписант / МОРОЗ АНАСТАСІЯ-РОКСОЛАНА ВАСИЛІВНА); cargo test 43/43, clippy 0, fmt чистий |
| 7.3 | Фінальний етап 7: offline_queue + shift/Z-звіт + інтеграція facade — kasa-prro::prro (PrroOfflineQueue 1:1 Python: add_document/get_pending(100)/count_pending/list_by_shift/mark_sent/mark_failed/is_expired 168 год/get_expired; PrroShiftUseCase open_shift T=108 SERVICECHK local_number=0 + close_shift ZREPORT + валідації SHIFT_ALREADY_OPEN/NO_OPEN_SHIFT; SyncOfflineQueueUseCase replay pending→sent/failed, expired блокується сервером ERROR_OFFLINE_168=-11; trait PrroRepository + InMemory (unit) + SqlxPrroRepository PostgreSQL (ensure_prro_schema, DDL 1:1 Alembic 578fd283a156); parse_receipt_xml_totals 1:1; facade kasa-api: KASA_RUST_PRRO=1|shadow, роути /api/v2/prro/fiscal/* (open/close/shifts/sync/queue/status), shadow — Rust готує чек і логує parity, Python виконує | Rust_Agent | 7f23102 | ✅ | cargo test 148/148 (workspace), clippy 0 (наш код), fmt чистий; sqlx-інтеграція 6/6 + facade 5/5 на живій PostgreSQL; unit: queue 8, shift 9, sync 8 (відкат: gRPC-помилка → pending/failed + error, нуль втрат фіскального стану); shadow-лог open_shift: dat_len/signed_len/DI |
| 7.3.1 | SIGSEGV-фікс FFI EUSignCP (cades_iit стабільно падав після cargo clean + debug=line-tables-only) — КОРІНЬ: два баги euscp.so/cspb.so, НЕ наш UB: (1) EUReadPrivateKeyBinary використовує callee-saved %rbx без ініціалізації (очікує %rbx=0 від калера; Python-ctypes випадково лишає 0, C/Rust — адресу функції → `movl $0,(%rbx)` → запис у .text → SIGSEGV); (2) SDK читає %rcx від калера (Python/C: rcx=0 → rc=0; Rust: rcx=heap-ptr → rc=24 «невірний пароль»). gdb-докази: Python rbx=0/rcx=0 vs Rust rbx=адреса fn/rcx=heap; C-відтворювач із rbx=0+rcx=0 → rc=0. Фікс: C-обгортка ffi/euscp_wrappers.c (cc у build.rs) — rbx=0 + rcx=0 перед викликом, rbx зберігається в r11 (НЕ на стек — зсув rsp на 8 ламає movdqa у cspb.so). Одночасно виправлено хибну гіпотезу 7.2: буфери EUGetJKSPrivateKeyFile ТРЕБА free-шити (1:1 Python) — «dangling pointer від EUFreeMemory» був насправді rbx/rcx-багом; прибрано окремий Library::new у load_jks_key (1:1 Python: один CDLL) | Rust_Agent | (коміт SIGSEGV) | ✅ | cades_iit 5/5 прогонів (паралельно + threads=1), workspace 148/148, clippy 0 (наш код), fmt чистий; профілі Cargo.toml не змінені |
| 3.1 | POS: чеки v2 (sale/return/list/detail/items/stats/search/by-product/returnable), робочі сесії, списання, переміщення, зміни ПРРО (X/Z) | Rust_Agent | 72b4e21, fcaeffa, ba695ec, 6e97a5c, 9b0bf39, 435ea36 | ✅ | E2E POS 43/43: чеки (sale/return/список/деталі/статистика/пошук/returnable), робочі сесії, списання (авто-confirm), переміщення (draft→confirm/cancel), ПРРО X/Z; транзакційність: 400 у середині → чек не створено, stock не змінено; конкурентність 2 паралельні sale → stock 86.000, нуль втрат; cargo test 9/9, clippy/fmt чисті |
| 2.1 | Write-порти CRUD+інвентаризації, SQL-репозиторії write, CRUD-роути під flag, E2E differential-скрипт | Rust_Agent | 319d849, c66450c, 04e6edb, adfa79a | ✅ | E2E 16/16: 201/200/204, 404, 409, 400, 422 ідентичні Python; конкурентність 2 паралельні confirm → stock 104.000; БД почищена від тестових даних |

**DoD підетапу 7.3 (offline_queue + shift + sync + facade):**
- [x] PrroOfflineQueue 1:1 Python `offline_queue.py`: add_document (валідації
      local_number<0, xml_body порожній), get_pending(limit=100, pending→failed
      порядок), count_pending (лише pending), list_by_shift (local_number asc),
      mark_sent (sent_at=now, error=NULL), mark_failed (failed + error),
      is_expired (>168 год, НЕ >= — 1:1 Python), get_expired (фільтр pending)
- [x] PrroShiftUseCase 1:1 Python `shift_use_case.py`: open_shift (T=108,
      SERVICECHK, local_number=0 → PrroShift open, last_local_number=0, запис у
      чергу sent, last_shift_number+1, persist counters DI/NT);
      close_shift (ZREPORT → closed, zreport_number=response.id, closed_by);
      валідації SHIFT_ALREADY_OPEN (до gRPC!), NO_OPEN_SHIFT;
      _build_zreport_data 1:1 (з фактично переданих чеків queue sent CHK:
      sales/returns count, payments SMI/SMO, taxes TXI/TXO/SMI, fallback
      receipt_count); auto_reminder_check (>24 год); list_shifts (пагінація)
- [x] parse_receipt_xml_totals 1:1 Python (C T, M T/SM, P TX/SM, E SM/TX/TXPR/
      TXSM + вкладені <TX>) — Rust-парсер тегів без зовнішнього XML-крейта
- [x] SyncOfflineQueueUseCase 1:1 Python `sync_offline_queue_use_case.py`:
      replay pending/failed по порядку → повторне RQ+MAC обгортання, підпис,
      send_chk; status=1 → mark_sent; status≠1 (зокрема ERROR_OFFLINE_168=-11
      для expired) → mark_failed з текстом; gRPC-виключення → mark_failed —
      ВІДКАТ: документ не втрачається (failed + error у черзі)
- [x] trait PrroRepository (ізольований від sqlx: PrroRepoError) + InMemory
      (unit-тести без БД) + SqlxPrroRepository (PostgreSQL): create/get/get_open/
      list_shifts/close_shift/increment_shift_counters + queue CRUD + settings
      upsert; ensure_prro_schema — ідемпотентний DDL 1:1 Alembic 578fd283a156
      (enum prro_shift_status/prro_queue_status, індекси, FK receipts/prro_shifts)
- [x] Facade kasa-api: KASA_RUST_PRRO=1 (Rust виконує) | shadow (Rust готує
      чек+підпис, логує parity: dat_len/signed_len/DI, Python виконує проксі);
      роути /api/v2/prro/fiscal/{shift/open, shift/close, shifts, sync, queue,
      status}; close_shift — require_admin; gRPC-клієнт PrroGrpcClient
      переведено на &self (ChkSender trait, Send future для axum)
- [x] PrroSigner: + Send + Sync bound (axum Handler вимагає Send future)
- [x] Тести: unit queue 8, shift 9, sync 8 (kasa-prro); sqlx-інтеграція 6/6 +
      facade 5/5 (kasa-infrastructure/kasa-api, жива PostgreSQL); workspace
      148/148; clippy 0 (наш код), fmt чистий
- [x] Відкат (критерій 5): gRPC недоступний/сервер відхилив → open_shift/close_
      shift НЕ змінюють стан (зміна не створюється / лишається open), sync →
      mark_failed + error; фіскальний стан зберігається, нуль втрат
- [x] Обмеження 7.3: пароль КЕП для Rust-фасаду — env PRRO_KEY_FILE +
      PRRO_KEY_PASSWORD (plaintext); Python key_store (Fernet + PRRO_MASTER_KEY)
      не підтримується (TODO наступних етапів); real gRPC open_shift до
      sandbox ДПС потребує налаштованого ключа/ФН (як Python)

**DoD підетапу 7.2 (XAdES/CAdES крипто-шар):**
- [x] crypto::xades (чистий Rust, ADR-014): XAdES-BES enveloped — C14N 1.1 inclusive
      1:1 libxml2/lxml (namespace nodes тільки не виведені на предку, §2.3; порожні
      елементи — пари тегів); RSA PKCS#1 v1.5 (rsa crate) детермінований; ECDSA
      P-256/P-384 (p256/p384 crates); вставка ds:Signature останнім елементом;
      verify (digest + signature + публічний ключ з KeyInfo/X509Certificate)
- [x] GOLDEN XAdES 5/5: Rust sign == Python signxml БАЙТ-В-БАЙТ (чек sale/return,
      Z-звіт, службові 108/111) з ключем certs/prro-test/test-rsa.pem; digest +
      signature value зафіксовані у tests/fixtures/xades_golden.json
- [x] crypto::iit FFI завершено: load_jks_key (EUGetJKSPrivateKeyFile →
      EUSaveCertificate×N → EUReadPrivateKeyBinary) з одним локальним Library
      (quirk EUSignCP: виклики через self.lib дають rc=24, через свіжий
      Library::new — rc=0; задокументовано в коді); CAdES-BES sign/verify
      (EUSignDataInternal/EUVerifyDataInternal); get_signer_serial/get_signer_name
      (X.509 parse, 1:1 Python)
- [x] CAdES golden: ДСТУ 4145 недетермінований (випадковий k) → golden = взаємна
      verify-сумісність: Rust verify Python-підпису ✅, Python verify Rust ✅
      (cross-check виконано); Python sig зафіксовано у fixtures/cades_python_sig.bin,
      Rust verify його — тест
- [x] Структура CAdES: ContentInfo/signedData (OID 1.2.840.113549.1.7.2) — як
      Java ee.SignInternal(true, data) офіційного семпла programika/prro_sample
- [x] get_serial_number/get_signer_name 1:1 Python: RSA 7AED6274... / Тестовий
      Підписант Kasa; ДСТУ 5E984D52... / МОРОЗ АНАСТАСІЯ-РОКСОЛАНА ВАСИЛІВНА
- [x] Фабрика signer_from_key_material: OID ДСТУ 4145 → IitSigner (CAdES),
      RSA/ECDSA → XadesSigner (XAdES) — шлях XML → підпис → gRPC sendChkV2 готовий
- [x] cargo test 43/43 (kasa-prro), clippy 0, fmt чистий; workspace 112 passed

**DoD підетапу 7.1 (ПРРО-фундамент):**
- [x] ADR-014 прийнято: крипто-стратегія (a) FFI до IIT SDK EUSignCP (euscp.so) через
      libloading з ручними extern "C" сигнатурами (без bindgen — не тягне libclang у CI);
      RSA/ECDSA — чистий Rust (XAdES). Відхилено: (b) Python sidecar, (c) gRPC-криптосервіс,
      (d) чистий Rust ДСТУ 4145 (заборонено планом)
- [x] tonic + prost: build.rs генерує типи з prro.proto (package com.programika.rro.ws.chk);
      protoc — vendored (protoc-bin-vendored), без системного protobuf-compiler
- [x] gRPC-клієнт ChkIncomeService: sendChkV2 / ping / statusRro / infoRro / lastChk /
      delLastChk / delLastChkId / open_shift; TLS (native roots + кастомний CA);
      дедлайни (Request::set_timeout) + ретраї 3 спроби, бек-оф 1s→2s; date_time
      yyyyMMddHHmmss (14 цифр) — 1:1 Python
- [x] SMOKE: TLS READY до cabinet.tax.gov.ua:9443; ping (local_number=0x7FFFFFFF,
      check_type=SERVICECHK, check_sign=b"") → status -1 (ERROR_VEREFY),
      error_message="Виникла помилка при розборі чи формуванні даних..." — ІДЕНТИЧНО Python
- [x] key_store (Rust): JKS (власний парсер: EncryptedPrivateKeyInfo DER → OCTET STRING →
      JavaSoft XOR/SHA1-keystream, 1:1 pyjks), PKCS#12 (openssl parse2), PEM (pem crate);
      авто-визначення формату (розширення + магія); витяг приватного ключа (DER PKCS#8),
      сертифікатів (DER), OID алгоритму (власний DER-парсер — ДСТУ-сумісний)
- [x] JKS-ключ certs/prtro-test/pb_3791505547 (2).jks (пароль test2003) читається:
      PKCS#8 802B, OID ДСТУ 4145, ланцюг 4 сертифікати, підписант
      serial=5E984D526F82F38F040000006E5EE80123BED307 (1:1 Python)
- [x] xml_builder (Rust): СЗЗД 2.1.7 — чек <C T="0|1">, Z-звіт <Z>, службові 108-112,
      канонізація (атрибути за алфавітом, закриті теги, без пробілів), MAC
      (SHA-256 → Base64), build_message <RQ><DAT><MAC></RQ>
- [x] GOLDEN PARITY: 12 векторів Rust==Python байт-ідентично (v1-v7 + mac + message +
      canonical + to_cents/to_thousandths), згенеровано з Python-еталона
- [x] cargo test --workspace зелений (33 нових тестів kasa-prro), clippy 0, fmt чистий
- [x] euscp.so не встановлено (vendor поза git) — FFI smoke-тест тихо skip;
      при встановленому SDK перевіряє 10 сигнатур (див. iit_sdk_api_available_if_installed)

**DoD етапу 6 (Auth/users/settings/RBAC):**
- [x] auth: POST /login (пароль), /login-pin (PIN, 401 без PIN-коду), /refresh (400 без
      токена, 401 невалідний, 401 неіснуючий/деактивований), /logout (закриття сесії,
      duration_hours), GET /verify (публічний, optional), GET /users-list (публічний)
- [x] users: list (page/size + 422 int/ge/le), get (404), create (201, авто-логін
      транслітерацією, 409 дублікат), update (exclude_unset, хешування пароля/PIN),
      permissions (400 невідоме право), hourly-rate (422 float/gt), delete (204/404/409),
      permissions/list (групи+іконки 1:1)
- [x] settings: GET всі (модулі), GET /{module} (404), PUT /{key} (upsert: module/value_type/
      label авто; валідації 422: int-діапазони, bool true/false/1/0, whitelist barcode_type),
      PUT batch (тільки існуючі ключі, ігнор невідомих; нормалізація значень), 403 для cashier
- [x] RBAC: 401 без токена ("Відсутній заголовок авторизації"/"Невірний формат токена..."),
      403 cashier (users/settings), деактивація → 403; ролі admin/cashier (v1 — manager немає)
- [x] JWT parity: той самий секрет (KASA_JWT_SECRET / backend/.env SECRET_KEY); claims 1:1
      (access: sub/role/permissions/type/iat/exp; refresh: без permissions); крос-валідація
      Rust↔Python: verify обидва напрямки valid=true, refresh обидва напрямки 200
- [x] feature-flag KASA_RUST_AUTH: =1 → Rust-гілка; =0 → проксі Python (відкат перевірено)
- [x] E2E differential-скрипт scripts/e2e_auth_diff.sh — 59/59 зелений
- [x] cargo test --workspace 69 passed, clippy 0 warnings, fmt чистий
- [x] БД почищена: тестові юзери видалені, settings відновлені (print_copies=1,
      auto_cut_paper=false, barcode_type=code128, upsert-ключі видалені)

**DoD етапу 4 (Ledger):**
- [x] ledger v1+v2 (7 ендпойнтів): POST /ledger, GET /{supplier_id}, GET /balance/{id} (v1);
      GET/POST /entries, GET /balance/{id}, GET /balances (v2) — 1:1 Python
- [x] differential 10 100 записів (10 000 через Rust v2 POST + 100 через Python v1 POST):
      GET v2 entries 101 сторінка × 100 — Rust==Python 1:1; v1 history, v1/v2 balance, v2 balances
- [x] валідації 1:1: 404 (v1×3, v2 balance), 400 (тип/supplier), 422 (decimal_max_places
      з ctx, missing з input=body, enum з ctx.expected), 500 ValueError (v2 entries)
- [x] конкурентність: 2 паралельні POST → 201/201, записів 2 (жоден не втрачено)
- [x] транзакційність: 400 (невалідний тип) не створює запис (count до/після рівні)
- [x] E2E differential-скрипт scripts/e2e_ledger_diff.sh — повністю зелений (25/25)
- [x] cargo test --workspace зелений (63 passed, 0 failed), clippy 0, fmt чистий
- [x] БД почищена: тестові E4-/LEDGER- дані count=0; реальні дані не чіпались

**DoD етапу 3 (POS):**
- [x] чеки v2 (sale/return/list/detail/items/stats/search/by-product/returnable) — 1:1 Python
- [x] робочі сесії (/my, /report, /user/{id}) — 1:1, 'Z'-формат часу
- [x] списання (CRUD+confirm, авто-confirm при create) — 1:1, вхідна scale create / scale БД GET
- [x] переміщення (CRUD+confirm/cancel, тільки чернетки редагуються) — 1:1
- [x] зміни ПРРО: list з БД; open/close без ПРРО → 400 з текстом Python
- [x] конкурентність: 2 паралельні sale → stock 86.000, нуль втрат (FOR UPDATE)
- [x] транзакційність: помилка на 2-й позиції sale → rollback (stock/чек/номер не спалено)
- [x] E2E differential-скрипт scripts/e2e_pos_diff.sh — повністю зелений
- [x] cargo test/clippy/fmt чисті; БД почищена від тестових даних (реальні 24.07 не чіпались)

**DoD етапу 2:**
- [x] write-репозиторії + транзакції для products/categories/suppliers/inventory
- [x] CRUD-роути POST/PUT/PATCH/DELETE під KASA_RUST_READDIRS (без флага — проксі)
- [x] валідація/статуси/помилки 1:1 з Python (201/200/204, 404, 409, 400, 422)
- [x] конкурентність: 2 паралельні confirm → stock 104.000, нуль втрат
- [x] E2E differential-скрипт scripts/e2e_crud_diff.sh — 16/16 пройдено
- [x] БД почищена від тестових даних, цілісність збережена

**DoD етапу 1:**
- [x] репозиторії sqlx read-only + сервіси + DTO для products/categories/suppliers
- [x] роути GET під feature-flag KASA_RUST_READDIRS (без флага — проксі на Python, відкат)
- [x] snapshot-тести: Rust-відповідь == Python-еталон (нормалізований JSON)
- [x] cargo build/clippy/test/fmt чисті

**DoD етапу 0:**
- [x] cargo workspace збирається; Tauri-команди працюють через крейти
- [x] фасад :8000 приймає запити, проксі на Python :8001 працює
- [x] фронтенд працює БЕЗ змін (axios → :8000, тепер це Rust-фасад)
- [x] `cargo test` зелений; differential-міст (Rust CLI) зібраний

## 2.1 Етап 8 — група 1/9: Боржники (debtors) ✅ ЗАВЕРШЕНО

**Коміт:** `992f0f2` — feat(rust): група 1/9 — боржники (debtors) 1:1 Python, KASA_RUST_DEBTORS=1

**Що зроблено:**
- `kasa-domain::debtors` — DebtorService trait, DTO (DebtorDto, DebtorPaymentDto,
  DebtorReceiptDto v1, DebtorListDto), DebtorError, blanket impl Arc
- `kasa-infrastructure::repositories::debtors::SqlxDebtors` — search (ilike),
  list (пагінація, sort total_debt DESC), CRUD, pay (транзакція FOR UPDATE:
  400/404/422 1:1, повне погашення → DELETE з каскадом payment — як SQLAlchemy
  cascade), receipts (v1 JOIN items+products), payments
- `kasa-application::DebtorServiceFacade`
- `kasa-api::debtors` — 8 роутів /api/v1/debtors* під `KASA_RUST_DEBTORS=1`,
  Pydantic Decimal-валідація (decimal_parsing/decimal_max_places/greater_than)
- `scripts/e2e_debtors_diff.sh` — differential Rust==Python: **29 перевірок PASS**
  (create 201/parity, search, list total+items, get, update, pay часткове 70.00,
  pay повне 0.00+видалення, receipts реального боржника, payments,
  валідації 404/400/422), тестові дані видаляються

**Верифікація:** cargo test 148/148, clippy 0 (наш код), fmt чистий,
фасад :8002 (KASA_RUST_DEBTORS=1) — Python :8001 вимкнено для групи.

---

## 2.2 Етап 8 — аналіз решти 8 груп (карта робіт)

| Група | Файли Python | Обсяг | Залежності | Оцінка |
|---|---|---|---|---|
| 2. documents | v1/documents.py | 1793 | **залежить від invoices/return_invoices/purchase_orders** (copy→Response-схеми, batch-confirm→confirm-логіка) | 5-8 год; Excel export (openpyxl→rust_xlsxwriter), print (HTML), copy |
| 3. invoices | v1(748)+v2(362) | 1110 | stock/ledger сервіси (Rust має) | 4-6 год; confirm→stock+SupplierLedger, payment-info, print-items |
| 4. return_invoices | v1/return_invoices.py | 428 | stock/ledger | 2-3 год |
| 5. purchase_orders | v1/purchase_orders.py | 416 | invoice (confirm→створює invoice) | 2-3 год |
| 6. print+print_templates | v1/print.py(719)+print_templates.py(315) | 1034 | minijinja рендер, ESC/POS, цінники/етикетки | 4-6 год; Jinja2→minijinja сумісність |
| 7. products v2 | v2/products.py | 360 | multipart upload, static serve, barcode-генерація | 2-3 год |
| 8. prro v2 | v2/prro.py | 229 | kasa-prro (gRPC+крипто готові в 7.3) | 2-4 год; test-connection, fiscalize |
| 9. ocr | ocr.py(108)+invoice_ocr.py(125)+services(690) | 923 | **зовнішній Gemini API** (genai SDK) | 3-5 год; клієнт REST Gemini + зіставлення з БД, мок-тест |

**⚠️ Технічне виправлення порядку ТЗ:** documents (група 2) глибоко залежить від
invoices/return_invoices/purchase_orders (copy повертає їхні Response-схеми;
batch-confirm викликає confirm_invoice/confirm_return_invoice). Тому фактичний
порядок міграції: **debtors(✅) → invoices → return_invoices → purchase_orders →
documents → print/print_templates → products v2 → prro v2 → ocr**.

**Рекомендація:** наступна сесія — invoices (найбільша залежність documents).

## 3. Аномалії та відкриті питання

1. **PR не відкрито**: `gh` CLI не автентифікований на цій машині. Гілка
   `feat/rust-migration` запушена в origin (moroznastya/KASA, 6 комітів).
   → Потрібен ручний PR через GitHub web UI або `gh auth login`.
3. **Python-баг v2 POST /ledger/entries → 500 UnmappedInstanceError** (етап 4):
   Rust v2 POST створює запис (201), Python падає з 500. Rust==Python на GET-стороні
   (101 сторінка 1:1); аномалія зафіксована скриптом e2e_ledger_diff.sh як «аномалія Python».
   → Виправити у Python-бекенді або задокументувати як відомий баг при дезактивації (етап 8).
3. **Python-баг delete_user → 500 IntegrityError** (етап 6):
   Python `DELETE /users/{id}` падає з 500 IntegrityError на користувачах, які мають
   робочі сесії (login): SQLAlchemy relationship `user.work_sessions` без cascade +
   БД FK ON DELETE CASCADE + nullable=False → ORM намагається встановити user_id=NULL
   → порушення NOT NULL. Юзер без сесій видаляється (204). Rust `SqlxAuth::delete_user`
   робить правильно (204, CASCADE) — це відхилення від Python у КРАЩИЙ бік, зафіксовано.
   → Виправити у Python-бекенді при дезактивації (етап 8): додати cascade у relationship.
5. **БЛОКУВАННЯ ЕТАПУ 8 — Python sidecar НЕ можна дезактивувати без втрати функціоналу** (2026-08-07, Rust_Agent):
   Rust НЕ покриває 6 груп роутів, які фронтенд активно використовує (сторінки
   documents/*, debtors/, printing/*, settings/PrintTemplatesPage; сервіси
   documentService, debtorService, printService, printTemplateService,
   productService images/barcodes, prroService):
   - **Документи** (план-етап 3, журнал 3b ⏳): /documents (list/get/create/delete/batch-confirm),
     /invoices (CRUD+confirm+payment-info+print-items+price-changes),
     /return-invoices, /purchase-orders — Python v1/invoices.py, v1/documents.py.
   - **Боржники**: /debtors (CRUD+search+pay+receipts+payments) — v1/debtors.py.
   - **Друк цінників/етикеток**: /print/price-tags/render, /print/labels/render,
     /print/printers, /print/test — v1/print.py (друк ЧЕКІВ — Rust, етап 5 ✅;
     цінники/етикетки — Python).
   - **Шаблони друку**: /print-templates (CRUD+render+set-default+default) — v1/print_templates.py.
   - **Продукти**: /products/{id}/images, /products/{id}/barcodes — v2/products.py
     (Rust crud НЕ має).
   - **ПРРО v2 залишки**: /prro/test-connection, /prro/receipts/{id}/fiscalize —
     v2/prro.py (Rust має лише /fiscal/*).
   Додатково: sqlx-міграцій НЕМАЄ (frontend/src-tauri/migrations/ порожня;
   у БД alembic_version — схема належить Alembic). tauri-updater: конфіг на місці
   (pubkey+endpoint), але endpoint https://github.com/kasa-pos/kasa-pos/.../latest.json
   → 404 (релізу ще немає) — не блокер, стане валідним при першому релізі.
   → ПРОПОЗИЦІЯ (етапність): 8a — sqlx-міграції з поточної схеми + 410 для
   покритих Rust-роутів при вимкненому флазі + умовний spawn sidecar;
   8b — міграція 6 груп у Rust (≈5-10 днів, починаючи з документів — план-етап 3);
   8c — повне видалення Python. Рішення потрібне від PM_Agent/NIKO.

4. Тестові процеси (facade :8000, Python :8001) зупиняються після верифікації.
   Наступний запуск: `cargo run -p kasa-api --bin facade` + Python sidecar
   (Tauri сам підніме sidecar при старті).

## 4. Наступні кроки (етап 7 — ПРРО)

1. Rust_Agent: ПРРО gRPC/tonic (crypto, xml, offline_queue, shift) — sandbox-сертифікація,
   golden parity з Python, shadow-mode, відкат.
2. QA_Agent: розширити differential-контур на auth/users/settings (e2e_auth_diff.sh).
3. Git Admin Agent: PR за етапами 1–6 (накопичений зміст).

## 7.3.1 Баг: SIGSEGV у cades_iit (FFI EUSignCP) — діагностика та фікс

**Симптом:** після `cargo clean` + `[profile.dev] debug="line-tables-only"` тест
`cades_iit` стабільно падає SIGSEGV (signal 11) — спершу на 2-му тесті, згодом
і на 1-му. Повний debuginfo маскував проблему розкладкою malloc — це тригер,
не причина. Профіль збірки змінювати заборонено.

**Діагноз (gdb + мінімальний C-відтворювач):** корінь — ДВА баги в SDK
`euscp.so`/`cspb.so` (ІІТ), які Python-ctypes випадково обходить:

1. **`%rbx` без ініціалізації (euscp.so).** `EUReadPrivateKeyBinary` у пролозі
   використовує callee-saved `%rbx` БЕЗ збереження/ініціалізації:
   `mov 0x200266(%rip),%eax; test; je; ...; movl $0x0,(%rbx)`.
   - Python (ctypes): на вході `%rbx == 0` (gdb-виміряно) → запис у NULL-подібну
     ділянку → ок (rc=0).
   - C/Rust: `%rbx == адреса функції` (dlsym-результат у callee-saved) →
     `movl $0,(%rbx)` пише в .text → SIGSEGV.
2. **`%rcx` від калера.** Той самий клас бага: SDK читає `%rcx` як додатковий
   параметр. Python/C: `rcx=0` → rc=0. Rust: `rcx=heap-ptr` → rc=24
   «невірний пароль» (навіть після фіксу №1).
   Побічно: `%rcx != 0` заводить SDK у гілку cspb.so з `movdqa` на адресу
   `rsp-0x38` (вирівнювання лише по 8) → #GP → SIGSEGV глибше в cspb.so.

**Фікс (ffi/euscp_wrappers.c, C-обгортка через cc у build.rs):**
перед викликом `EUReadPrivateKeyBinary` встановлює `%rbx=0` і `%rcx=0`,
оригінальний `%rbx` зберігає у `%r11` (caller-saved) — НЕ на стек:
`push %rbx` зсуває `%rsp` на 8 і ламає вирівнювання для movdqa у cspb.so.

**Виправлені хибні гіпотези 7.2 (задокументовані в iit.rs як помилкові):**
- «EUFreeMemory після EUGetJKSPrivateKeyFile → dangling pointer (rc=24)» —
  ХИБНО. Python free-шить буфери getJKS (iit_sdk.py) і працює; rc=24 у 7.2
  давав саме `%rcx`-баг. Тепер буфери free-шаться 1:1 Python.
- «окремий Library::new для load_jks_key потрібен (self.lib дає rc=24)» —
  ХИБНО. Прибрано: один дескриптор (self.lib) на весь SDK, як Python CDLL.

**Докази стабільності:** cades_iit 5/5 прогонів (паралельно) + 3/3
(`--test-threads=1`); `cargo test --workspace` 148/148; clippy 0 (наш код);
fmt чистий. Профілі Cargo.toml НЕ змінені.

**Висновок:** SDK EUSignCP має нестандартну ABI-угоду (читає callee-saved
регістри калера) — це quirk бібліотеки, а не UB нашого коду. Обгортка
відтворює Python-контекст (rbx=0, rcx=0) для зачепленої функції; sign/verify
мають нормальний пролог (зберігають rbx) — обгортка їм не потрібна
(перевірено дизасемблером).
