# Аудит відповідності torgashka-prro документації ДПС «Опис API Електронного кабінету»

**Дата:** 2026-08-09
**Аудитор:** QA_Agent (NIKO 4.0.0)
**Об'єкт:** `frontend/src-tauri/crates/torgashka-prro/` (етап 7 міграції Kasa → Rust)
**Еталон:** офіційна документація ДПС (gRPC ChkIncomeService, proto3, package `com.programika.rro.ws.chk`) + Python-еталон `backend/app/infrastructure/services/prro/` (1:1 parity)
**Метод:** статичний аналіз коду (Rust + Python-еталон + prro.proto), звірка з документом.

---

## Висновок: **ЧАСТКОВО ВІДПОВІДАЄ**

Крейт реалізує 6 з 8 обов'язкових пунктів чек-листа повністю (proto, gRPC, sendChkV2, local_number, службові типи, ping/відповіді). Виявлено **1 критичну невідповідність** (формат `date_time` у 3 шляхах відправки) та **1 відсутній механізм** (контроль послідовності — хеш попереднього Check), обидві успадковані при портуванні з Python-еталона, але в Rust додатково спотворені (epoch замість `yyyyMMddHHmmss` — у Python цього бага нема).

---

## Таблиця відповідності

| # | Пункт чек-листа | Статус | Деталі |
|---|---|---|---|
| 1 | gRPC endpoints + TLS | **OK** | `DEFAULT_PRRO_TEST_URL="cabinet.tax.gov.ua:9443"`, `DEFAULT_PRRO_PROD_URL="prro.tax.gov.ua:443"` (`prro/settings.rs:30-31`). TLS: native roots + кастомний CA (`grpc.rs:80-93`), для тестового 9443 також TLS. `PRRO_GRPC_INSECURE=1` — тільки для мок-сервера (`grpc.rs:63-67`). Крейт: `tonic 0.12` + `tonic-build` + `prost` (`Cargo.toml`). Резервний `prro2.tax.gov.ua:443` — відсутній |
| 2 | proto-визначення | **OK** | `backend/app/infrastructure/services/prro/prro.proto`: package `com.programika.rro.ws.chk`, message Check/CheckRequest/CheckRequestId/CheckResponse/StatusResponse/RroInfoResponse/Operator — усі поля та номери ідентичні доку; enum CheckResponse.Status — всі 18 значень (UNKNOWN+OK+16 помилок) з ПРАВИЛЬНИМИ негативними кодами (-1…-16); service ChkIncomeService — всі 8 RPC. Генерація: `build.rs` → `tonic_build::compile_protos`; експорт: `proto.rs` |
| 3 | sendChkV2 як основний | **OK** | `grpc.rs:180-190` `send_chk_v2` — єдиний метод відправки; `chk_sender.rs:41-44` `ChkSender::send_chk → self.send_chk_v2`. V1 `sendChk` не реалізований (не потрібен з 01.10.2021) |
| 4 | local_number | **OK** | `0` для відкриття зміни: `shift.rs:123`, `grpc.rs:237`; `0x7FFFFFFF` для ping: `grpc.rs:143`, `PING_LOCAL_NUMBER=0x7FFF_FFFF` (`proto.rs:31`), юніт-тест `grpc.rs:316` |
| 5 | Службові типи Т | **OK (частково)** | V2-типи правильні: `SERVICE_OPEN_SHIFT=108`, `SERVICE_OFFLINE=109`, `SERVICE_ONLINE=110`, `SERVICE_PING=111`, `SERVICE_RESERVE=112` (`xml.rs:30-34`). Реалізовано: T=108 (`shift.rs:113`), T=111 (`settings.rs:497`). **T=109/110/112 — визначені, але не використовуються** (немає use case переходу offline/online та запиту діапазону резервних номерів) — аналогічно Python-еталону. V1 (8-12) не потрібні — відправка тільки V2 |
| 6 | Обробка відповідей | **OK** | `status_message()` покриває OK + всі 16 негативних статусів (`settings.rs:452-482`); `error_message` обробляється (`fiscalize.rs:334-348`); `ERROR_SAVE`/`ERROR_BAD_HASH_PREV` → дедуплікація через `lastChk` (`fiscalize.rs:826-847`); statusRro/infoRro → типи StatusResponse/RroInfoResponse (`grpc.rs:150-195`) |
| 7 | Контроль послідовності (хеш попереднього) | **ВІДСУТНЄ** | Жоден шлях формування Check не додає хеш XML попереднього чеку (див. Невідповідність №2) |
| 8 | ping CT=111 без MAC | **OK** | `build_ping_check_sign`: `build_service_check_xml("111")` + `build_message(..., include_mac=false)` (`settings.rs:497-500`); check_type=SERVICECHK, local_number=0x7FFFFFFF (`grpc.rs:143-146`); smoke-тест `examples/smoke_ping.rs` |

---

## Невідповідності

### №1. date_time надсилається як Unix epoch замість `yyyyMMddHHmmss` — **КРИТИЧНА**

**Файл:рядок:** `prro/fiscalize.rs:947`, `prro/shift.rs:469`, `prro/sync.rs:84`

**Що зараз:**
```rust
// fiscalize.rs:947 (make_check) та shift.rs:469
date_time: now.timestamp(),          // Unix epoch, секунди (10 цифр)
// sync.rs:84
date_time: chrono::Utc::now().timestamp(),
```

**Що має бути згідно доки/еталона:**
- Офіційний семпл ДПС `Sender.java` та Python-еталон `_check_date_time()` (`backend/.../grpc_client.py:59-67`): формат **`yyyyMMddHHmmss` (14 цифр, локальний час)**.
- Python use cases передають `build_check(...)` без `date_time` → `_make_check` підставляє `_check_date_time()` (yyyyMMddHHmmss). Rust-порт замість цього жорстко передає `Some(now.timestamp())`.

**Чому критично:** у XML всередині `check_sign` елемент `<TS>` формується як `yyyyMMddHHmmss` (локальний час), а `date_time` у gRPC-заголовку — epoch. Розбіжність форматів може дати `ERROR_XML_DATE` (-8) від фіскального сервера або неправильну фіскальну дату. Примітка: `grpc.rs:126` (`unwrap_or_else(check_date_time)`) робить правильно, але цей шлях використовується лише у мертвому `PrroGrpcClient::open_shift` (ніким не викликається) та `ping`.

**Серйозність:** критична (бойова відправка чеків/Z-звітів/офлайн-синку).

---

### №2. Відсутній контроль послідовності — хеш XML попереднього Check — **СЕРЕДНЯ**

**Файл:рядок:** відсутність механізму в `prro/fiscalize.rs` (build/відправка), `prro/sync.rs`, `prro/shift.rs`, `src/xml.rs` (уся структура `<DAT>/<C>`) — ніде немає поля/елемента з хешем попереднього чеку.

**Що зараз:** жоден Check не містить хеша попереднього; `local_number` зростає послідовно (`fiscalize.rs:258` `last_local_number + 1`), DI/MAC-нумерація йде через `XmlBuilder` (`xml.rs:562-573`), але це не хеш попереднього документа.

**Що має бути згідно доки:** «Контроль послідовності — хеш XML попереднього Check у кожному чеку (крім перевірки зв'язку)». Сервер очікує хеш; при розриві послідовності повертає `ERROR_BAD_HASH_PREV` (-12).

**Додатково:** невідповідність успадкована від Python-еталона (`grpc_client.py`/`xml_builder.py` хеш попереднього не формують). Rust-код вміє лише **обробляти** -12 (`fiscalize.rs:826`, `settings.rs:470`), але не **запобігає** йому. Для тестового середовища (чеки нефіскальні) може не проявлятися, для бойового — ризик відхилення при офлайн-відправці.

**Серйозність:** середня (бойовий сценарій офлайн-черги; онлайн-потік зазвичай не розриває послідовність).

---

### №3. Резервний endpoint `prro2.tax.gov.ua:443` не налаштований — **НИЗЬКА**

**Файл:рядок:** `prro/settings.rs:30-31` (тільки `DEFAULT_PRRO_TEST_URL` / `DEFAULT_PRRO_PROD_URL`)

**Що зараз:** один прод-сервер; при відмові `prro.tax.gov.ua` — немає failover.

**Що має бути згідно доки:** «додаткова https://prro2.tax.gov.ua:443».

**Серйозність:** низька (резервний канал; не впливає на коректність протоколу).

---

### №4. Службові типи T=109/110/112 не задіяні в use cases — **НИЗЬКА**

**Файл:рядок:** `xml.rs:30-34` (константи), використання лише 108 (`shift.rs:113`) і 111 (`settings.rs:497`).

**Що зараз:** переходи offline/online (T=109/110) і запит діапазону резервних номерів (T=112) не реалізовані як операції — ПРРО не може явно повідомити сервер про перехід в офлайн/онлайн та запросити резервні номери.

**Що має бути згідно доки:** «Т=9/109 перехід в офлайн; Т=10/110 перехід в онлайн; Т=12/112 запит діапазону резервних номерів» (через sendChkV2).

**Додатково:** ідентично Python-еталону (там цих use case також немає) — тому не регресія, але неповнота функціоналу офлайн-режиму (офлайн-черга працює без явних переходів; сервер може не знати про перехід).

**Серйозність:** низька (обмеження функціоналу, не помилка протоколу; компенсується офлайн-чергою та sync).

---

### №5. Коментар поля `date_time` у proto суперечить реалізації — **НИЗЬКА**

**Файл:рядок:** `backend/app/infrastructure/services/prro/prro.proto` (рядок `int64 date_time = 2;` — коментар «Unix epoch, секунди»).

**Що зараз:** коментар у proto каже epoch, а фактичний wire-формат (Python-еталон, офіційний Sender.java) — `yyyyMMddHHmmss`.

**Що має бути:** коментар має описувати фактичний формат `yyyyMMddHHmmss` (або «локальний час, 14 цифр»), щоб не вводити в оману.

**Серйозність:** низька (не впливає на wire-формат — лише документація).

---

## Рекомендації

| # | Дія | Пріоритет |
|---|---|---|
| 1 | **Виправити `date_time` у 3 місцях** (`fiscalize.rs:947`, `shift.rs:469`, `sync.rs:84`): замість `now.timestamp()` використати `crate::grpc::check_date_time()` (yyyyMMddHHmmss, локальний час) — 1:1 Python `_check_date_time`. Після зміни — golden-тест формату (`check_date_time_format_is_14_digits` уже є) | Критичний |
| 2 | **Реалізувати контроль послідовності**: зберігати хеш/ідентифікатор попереднього успішного Check (у `PrroOfflineQueue` або `XmlBuilder`) і включати в наступний Check крім ping. Мінімально: задокументувати відхилення від доки ДПС і зафіксувати обробку -12 як компенсацію | Середній |
| 3 | **Додати failover endpoint** `prro2.tax.gov.ua:443` у `settings.rs` (env `PRRO_PROD_URL_2`), вибір при `Channel`-помилці | Низький |
| 4 | **Реалізувати T=109/110** (явні переходи offline/online) та **T=112** (запит діапазону резервних номерів) у `prro/sync.rs` або окремому use case — закриє функціонал офлайн-режиму згідно доки | Низький |
| 5 | **Видалити мертвий `PrroGrpcClient::open_shift`** (правильний формат, але ніким не викликається) або перевести `shift.rs` на нього — усуне дублювання логіки | Низький |
| 6 | Виправити коментар `date_time` у `prro.proto` | Низький |

---

## Обґрунтування (ключові докази з коду)

- **proto = еталон:** `backend/app/infrastructure/services/prro/prro.proto` — package, поля, enum-коди (-1…-16), 8 RPC ідентичні доку (перевірено звіркою message-by-message).
- **sendChkV2:** `grpc.rs:180-190` `stub.send_chk_v2(req)`, `chk_sender.rs:41-44`; Python `grpc_client.py:191-216` викликає `self._stub.sendChkV2`.
- **ping:** `grpc.rs:137-146` — local_number=0x7FFFFFFF, check_type=3; XML `settings.rs:497-500` — T=111, `include_mac=false`; smoke-тест `examples/smoke_ping.rs` очікує TLS-канал + статус -1 на тестовому сервері.
- **local_number=0:** `shift.rs:123` (SERVICECHK, 0), `shift.rs:213` (ZREPORT, 0); тест `fiscalize.rs:1156` (`shift.last_local_number == 1`).
- **Відповіді:** `settings.rs:452-482` — повний маппінг 17 статусів; `fiscalize.rs:334-348` — error_message у DTO; `fiscalize.rs:826-847` — дедуплікація через lastChk при -3/-12.
- **date_time-баг:** `fiscalize.rs:947` `date_time: now.timestamp()` vs Python `context.py:188-208` `build_check(...)` без date_time → `_check_date_time()` (`grpc_client.py:59-67`) = `%Y%m%d%H%M%S`.
- **Відсутність хешу попереднього:** grep по `src/` (fiscalize/sync/shift/xml) — немає жодного механізму prev-hash; Python-еталон — аналогічно (grep по `grpc_client.py`, `xml_builder.py`, use cases — нуль збігів крім обробки помилки -12).
