# Детальний план реалізації ПРРО для Kasa POS

**Дата:** 2026-08-01
**Автор:** Orchestrator (Auto) + PM_Agent
**Статус:** ЗАТВЕРДЖЕНО 2026-08-01 (рішення користувача: тестові КЕП → certs/prro-test; політика нестачі — часткова фіскалізація; підтримка будь-якого формату ключа)

---

## 0. Резюме

**Мета:** реалізувати в Kasa POS програмний реєстратор розрахункових операцій (ПРРО)
для фіскалізації чеків через API Фіскального серверу ДПС України.

**Підхід:** спочатку **ТЕСТОВЕ API** (`cabinet.tax.gov.ua:9443`, чеки не фіскальні)
для відшліфовування, потім — **БОЙОВЕ** (`prro.tax.gov.ua:443`).

**Вимоги користувача:**
1. ПРРО виконує всі необхідні операції фіскалізації чеків, що вимагає ДПС.
2. У Kasa: вікно роботи з ПРРО (операції) + налаштування ПРРО (електронний ключ + пароль).
3. Фіскалізація ЛИШЕ товару, який прийшов по **фіскальній накладній**.
4. **Політика при нестачі `fiscal_stock` — ЧАСТКОВА ФІСКАЛІЗАЦІЯ** (чек розділяється: фіскальна частина → ПРРО, нефіскальна → звичайний чек; зв'язок через `split_group_id`).
5. **Адаптація під будь-який ключ КЕП**: підтримка форматів ІІТ `.dat`, PKCS#12 `.pfx/.p12`, Java `.jks`, PEM-пари (`.pem`+`.crt`).
6. **Тестові ключі** → `certs/prro-test/` (інструкція в `README.md`), бойові → `certs/prro-prod/`.

**Джерела протоколів (`kasa/docs/scr/`):**
- `_site_text.txt` / `site.html` — «Опис API Електронного кабінету... для фіскалізації чеків та передачі Z-звітів» (gRPC, методи, повідомлення, коди помилок).
- `SZZD_RRO_Protokol_peredach_nformats_2_1_7.txt` — протокол СЗЗД 2.1.7: XML-формат чеків/Z-звітів, канонічний вигляд, MAC, коди помилок.

> ⚠️ **Важливо:** `docs/plan_prro.md` (попередня версія) згадує HTTP/JSON — це НЕВІРНО.
> За `docs/scr` транспорт — **gRPC (proto3)**, а XML СЗЗД передається всередині
> поля `check_sign` повідомлення `Check`. Цей план базується на фактах з `docs/scr`.

---

## 1. Конспект протоколу ДПС (з docs/scr — обов'язково до врахування)

### 1.1. Транспорт — gRPC (proto3), сервіс `ChkIncomeService`

| Адреса | Призначення |
|---|---|
| `cabinet.tax.gov.ua:9443` | **Тестове** API (чеки/Z-звіти НЕ фіскальні) |
| `prro.tax.gov.ua:443` | Бойове API |
| `prro2.tax.gov.ua:443` | Додаткова бойова адреса (застарілі ОС) |

```proto
syntax = "proto3";
package com.programika.rro.ws.chk;

message Check {
    string rro_fn = 1;        // фіскальний номер ПРРО
    int64  date_time = 2;     // epoch
    bytes  check_sign = 3;    // XML-документ СЗЗД, ПІДПИСАНИЙ КЕП (XAdES)
    int32  local_number = 4;  // 0 = відкриття зміни; 0x7FFFFFFF = перевірка зв'язку
    enum Type { UNKNOWN=0; CHK=1; ZREPORT=2; SERVICECHK=3; }
    Type check_type = 5;
    string id_offline = 6;    // ідентифікатор офлайн-ланцюжка
    string id_cancel = 7;
}

message CheckResponse {
    string id = 1;
    enum Status {
        UNKNOWN=0; OK=1;
        ERROR_VEREFY=-1; ERROR_CHECK=-2; ERROR_SAVE=-3; ERROR_UNKNOWN=-4;
        ERROR_TYPE=-5; ERROR_NOT_PREV_ZREPORT=-6; ERROR_XML=-7; ERROR_XML_DATE=-8;
        ERROR_XML_CHK=-9; ERROR_XML_ZREPORT=-10; ERROR_OFFLINE_168=-11;
        ERROR_BAD_HASH_PREV=-12; ERROR_NOT_REGISTERED_RRO=-13;
        ERROR_NOT_REGISTERED_SIGNER=-14; ERROR_NOT_OPEN_SHIFT=-15; ERROR_OFFLINE_ID=-16;
    }
    Status status = 2;
    bytes  id_sign = 3;
    bytes  data_sign = 4;     // останній чек (для lastChk)
    string error_message = 5;
}

service ChkIncomeService {
    rpc sendChk    (Check) returns (CheckResponse);   // ⚠️ діє до 01.10.2021
    rpc sendChkV2  (Check) returns (CheckResponse);   // ✅ з 01.10.2021 — ВИКОРИСТОВУВАТИ ЦЕЙ
    rpc lastChk    (CheckRequest) returns (CheckResponse);
    rpc ping       (Check) returns (CheckResponse);   // перевірка зв'язку, Т=111, MAC порожній
    rpc delLastChk (CheckRequest) returns (CheckResponse);    // 1 раз, тільки чек продажу (обрив зв'язку)
    rpc delLastChkId (CheckRequestId) returns (CheckResponse); // 1 раз, за ID
    rpc statusRro  (CheckRequest) returns (StatusResponse);    // open_shift, online, last_signer
    rpc infoRro    (CheckRequest) returns (RroInfoResponse);   // детальна інформація + оператори
}
```

### 1.2. Режими чеків та службові чеки

| Тип | Значення `local_number` | Призначення |
|---|---|---|
| Відкриття зміни | `0` | Службовий «нульовий чек», тип **Т=108** (sendChkV2) |
| Перевірка зв'язку | `0x7FFFFFFF` | Т=111, `ping`, MAC не заповнюється |
| Звичайний чек | 1..N | CHK (Т=0 продаж, Т=1 повернення) |
| Z-звіт | останній+1 | ZREPORT (закриття зміни) |

**Службові чеки (з 01.10.2021, тільки sendChkV2):**
Т=108 відкриття зміни · Т=109 перехід в офлайн · Т=110 перехід в онлайн ·
Т=111 перевірка зв'язку · Т=112 запит діапазону резервних номерів.

### 1.3. Правила роботи зі зміною (shift)

- При першому старті зміна **закрита**; відкривається **вручну** (кнопка на ПРРО).
- Відкриття = «нульовий чек» (local_number=0). Якщо цим КЕП відкрита зміна на іншому ПРРО — **відмова** з вказівкою фіскального номера.
- **Після відкриття всі чеки підписуються ОДНИМ підписом** (одним КЕП).
- Закриття = **Z-звіт** (вручну; в офлайні можливо, якщо ПРРО вміє рахувати).
- Авто-нагадування користувачу: зміна триває **> 24 год**; перед першим чеком нового дня.
- Зміну замість касира може закрити **старший касир**.

### 1.4. XML-формат чеків (СЗЗД 2.1.7)

**Конверт повідомлення:**
```xml
<?xml version="1.0" encoding="windows-1251"?>
<RQ V="1">
  <DAT FN="4538765845" TN="ПН 345612052809" ZN="АА57506761" DI="238" V="1">
    {зміст пакету: <C> чек або <Z> Z-звіт}
    <TS>20110801112601</TS>
  </DAT>
  <MAC DI="238" NT="34">{значення MAC Base64}</MAC>
</RQ>
```
- `FN` — фіскальний номер РРО; `TN` — податковий номер (ІПН); `ZN` — заводський номер;
  `DI` — унікальний ідентифікатор пакету в межах РРО; `NT` — номер MAC, зростає.
- **MAC** обчислюється для тегу `<DAT>` (хеш-ланцюжок: кожен Check містить хеш XML попереднього).
- Тег `<DAT>` завжди в **канонічному вигляді** (Додаток А): без пробілів/переносів між тегами,
  всі теги закриті `</tag>`, атрибути — **в алфавітному порядку**.

**Чек `<C T="...">`** (T: 0 продаж, 1 повернення, 2 службове внесення, 3 приймання пального, 4 переказ, 7 виплата готівки):

| Тег | Призначення | Ключові атрибути |
|---|---|---|
| `<P>` | Продаж | `N` (номер), `C` (код), `CD` (штрихкод), `NM` (назва), `SM` (сума), `Q` (кількість), `PRC` (ціна), `TX` (податок) |
| `<CP>` | Відмова від продажу | те саме |
| `<D>/<S>` | Знижка/націнка | `TR` (0 попередній продаж, 1 проміжний, 2 спеціальна), `TY` (0 сумова, 1 відсоткова), `PR`, `SM`, `NI` |
| `<M>` | Оплата | `T` (0 готівка, ≠0 безготівка), `NM`, `SM`, `RM` (решта), `RRN` (id транзакції еквайра), `PSNM` |
| `<I>/<O>` | Службове внесення/видача | `T`, `NM`, `SM` |
| `<L>` | Текстовий коментар | текст |
| `<VD>` | Відміна операції | `NI` (номер операції, що відміняється) |
| `<E>` | Закриття чеку | `NO` (номер фіскального чеку), `SM` (сума), `SE`, `FN`, `TS` (YYYYMMDDhhmmss), `TX/TXPR/TXSM`, `CS` (касир), `VD="1"` (відміна всього) |
| `<TX>` | Податок (всередині `<E>`) | `TX`, `TXPR` (ставка %), `TXSM` (сума), `DTPR/DTSM`, `TXTY` (0 включено, 1 не включено, 2 прибутковий), `TXAL` (алгоритм 0/1/2/3) |

**Правила сум/кількості:**
- Суми: **грн × 100** (12,34 грн → `1234`).
- Кількість: **× 1000** (1 шт → `1000`; 12,345 кг → `12345`).

**Z-звіт:** `<DAT><Z NO="номер">...</Z><TS>...</TS></DAT><MAC ...>` — підсумки зміни.

**Приклад чеку продажу (канонічний):**
```xml
<DAT DI="238" FN="4538765845" TN="ПН 345612052809" V="1" ZN="АА57506761">
  <C T="0">
    <P N="1" C="0760557812166" NM="T.sonic 615" SM="800" TX="1"/>
    <M N="2" T="0" NM="ГОТІВКА" SM="750" RM="250"/>
    <E N="3" NO="19" SM="750" SE="625" FN="4538765845" TS="20110801112601"
       TX="1" TXPR="20.00" TXSM="125" TXTY="0" TXAL="0" CS="1"/>
  </C>
  <TS>20110801112601</TS>
</DAT>
```

### 1.5. QR-перевірка фіскального чеку

```
https://cabinet.tax.gov.ua/cashregs/check?mac=<MAC>&date=yyyyMMdd&time=HHmm&id=<id>&sm=<сума.грн>&fn=<фіскальний номер>
```
Використовується для QR-коду на фіскальному чеку (друк).

### 1.6. Ключові коди помилок

| Код | Значення | Дія ПРРО |
|---|---|---|
| 1 | OK — чек фіскалізовано, надано номер | статус SENT |
| -1 | ERROR_VEREFY — помилка підпису | failed, перевірити КЕП |
| -3 | ERROR_SAVE — невірний хеш або **дубль чека** | failed, журнал |
| -9/-10 | ERROR_XML_CHK / ERROR_XML_ZREPORT — невірний XML | failed, валідація XML |
| -11 | ERROR_OFFLINE_168 — перевищено ліміт 168 год офлайну | блокування, попередження |
| -12 | ERROR_BAD_HASH_PREV — невірний хеш попереднього | resync ланцюжка |
| -13/-14 | РРО/підписант не зареєстровані | помилка налаштувань |
| -15 | ERROR_NOT_OPEN_SHIFT — зміна не відкрита | відкрити зміну |

---

## 2. Поточний стан Kasa (що вже є / чого немає)

### ✅ Вже реалізовано (незакомічені зміни у git — база для ПРРО)
| Компонент | Деталі |
|---|---|
| `Product.is_fiscal`, `fiscal_stock` | Фіскальний облік товарів (файли: `domain/entities/product.py`, `models/product.py`) |
| `Product.uktzed`, `tax_rate`, `tax_group`, `scan_excise` | Податкові атрибути товарів (вже в моделі) |
| `Receipt.is_fiscal`, `fiscal_status` (`none/pending/sent/failed`), `fiscal_number`, `fiscal_serial`, `fiscal_sent_at`, `fiscal_error`, `split_group_id` | Фіскальні поля чеків (`domain/entities/receipt.py`, `models/receipt.py`) |
| `ReceiptItem.fiscal_quantity` | Часткова фіскалізація позицій (міграція `f89706f0cc26`) |
| Міграції `f89706f0cc25`, `f89706f0cc26` | Alembic: фіскальні поля products/receipts/receipt_items |
| `Invoice.is_fiscal`, `ReturnInvoice.is_fiscal` | Вже існують — основа правила «товар з фіскальної накладної» |

### ❌ Чого НЕМАЄ (треба створити)
- gRPC-клієнт `ChkIncomeService` (Python: `grpcio`, `grpcio-tools`)
- Генератор XML СЗЗД (чек `<C>`, Z-звіт `<Z>`, службові Т=108–112, канонічний вигляд, MAC)
- Крипто-модуль: підпис XML КЕП (XAdES), зберігання ключа/пароля
- Сервіс зміни (відкриття/закриття, один підписант, авто-нагадування >24 год)
- Офлайн-черга (id_offline, резервні номери Т=112, ліміт 168 год)
- API ПРРО (`api/v2/prro.py`)
- Вікно ПРРО + налаштування у frontend
- Друк фіскального чеку з QR
- Тести ПРРО

---

## 3. Архітектура ПРРО-модуля (Clean Architecture Kasa)

```
backend/app/
├── contracts/
│   └── prro_contract.py               # Protocol: PrroGateway, PrroKeyStore, PrroXmlBuilder, PrroService
├── domain/
│   ├── entities/
│   │   └── prro.py                    # PrroShift (зміна), PrroQueueItem (офлайн-черга)
│   ├── value_objects/
│   │   └── prro.py                    # PrroStatus, CheckResult, FiscalReceiptData
│   └── services/
│       └── prro_domain_service.py     # правила: один підписант, ліміт 168 год, локальні номери
├── application/
│   ├── use_cases/prro/
│   │   ├── open_shift.py              # Т=108
│   │   ├── close_shift.py             # Z-звіт
│   │   ├── fiscalize_receipt.py       # CHK (sale/return), split на фіскальні/нефіскальні
│   │   ├── sync_offline_queue.py      # синхронізація офлайн-ланцюжка
│   │   ├── prro_status.py             # statusRro / infoRro
│   │   └── reserve_numbers.py         # Т=112
│   ├── dto/prro_dto.py
│   └── services/prro_service.py       # оркестрація + журнал + ретраї
├── infrastructure/
│   ├── services/prro/
│   │   ├── grpc_client.py             # ChkIncomeService (sendChkV2, ping, statusRro, infoRro, lastChk, delLastChk)
│   │   ├── xml_builder.py             # XML СЗЗД + канонізація + MAC-хеш-ланцюжок
│   │   ├── crypto_signer.py           # XAdES-підпис (signxml/cryptography), формати pfx/dat/jks
│   │   ├── key_store.py               # безпечне зберігання ключа/пароля (шифрування)
│   │   └── offline_queue.py           # черга непереданих чеків
│   └── persistence/
│       ├── models/prro_models.py      # PrroSetting, PrroShift, PrroQueueItem
│       └── repositories/prro_repository.py
└── api/
    └── v2/prro.py                     # роутери: settings, status, shift, fiscalize, journal, retry
```

**Модель даних (нові таблиці):**
| Таблиця | Поля |
|---|---|
| `prro_settings` | `id`, `key_name` (файл КЕП), `key_password` (шифровано), `fn` (фіскальний номер), `tn` (податковий номер), `zn` (заводський номер), `mode` (test/prod), `last_shift_number`, `updated_at` |
| `prro_shifts` | `id`, `shift_number`, `opened_at`, `closed_at`, `signer_serial` (КЕП), `signer_name`, `closed_by`, `zreport_number`, `status` (open/closed), `receipt_count`, `total_amount` |
| `prro_queue` | `id`, `receipt_id`, `local_number`, `xml_body`, `mac`, `created_at`, `status` (pending/sent/failed), `error`, `sent_at` |

**Налаштування:** додатково через `system_settings` module="prro" (сумісно з наявним `SettingsService`).

**Інтеграція з наявним:**
- `application/use_cases/receipt_use_cases.py` → після збереження чека → `FiscalizeReceiptUseCase`
- `api/v2/receipts.py` (`POST /sale`, `POST /return`) → статус фіскалізації у відповіді
- `api/v2/invoices.py` (фіскальна накладна) → оприбуткування `fiscal_stock` (`Invoice.is_fiscal=True`)
- Друк: `print` + Tauri → фіскальний чек з QR
- Frontend: `pages/prro/PrroPage.tsx`, `pages/settings/PrroSettings.tsx`, інтеграція в `PosPage.tsx`

---

## 4. Фази реалізації

### ФАЗА 0 — Підготовка та дослідження (0.5–1 день)
**Відповідальні:** Python_Backend_Agent, QA_Agent, System_Architect_Agent

| # | Задача | Результат |
|---|--------|-----------|
| 0.1 | Отримати/згенерувати **тестові КЕП** (ІІТ «Користувач ЦСК-1» / тестові ключі ДПС) та зареєструвати тестовий ПРРО (фіскальний номер) — **потрібен доступ користувача** | тестові ключі + fn |
| 0.2 | Перевірити доступність `cabinet.tax.gov.ua:9443` (TCP/TLS) з робочого середовища | звіт про доступність |
| 0.3 | Зафіксувати `prro.proto` у `backend/app/infrastructure/services/prro/` (з `docs/scr/_site_text.txt`) + згенерувати Python-стаби (`grpcio-tools`) | `prro_pb2.py`, `prro_pb2_grpc.py` |
| 0.4 | ADR «Архітектура ПРРО-модуля» (модель даних, офлайн-стратегія, безпека ключів) | ADR-документ |
| 0.5 | Зробити `ping` (Т=111) на тестове API — перевірити зв'язок | CheckResponse OK |

**Критерій Фази 0:** `ping` повертає `CheckResponse` з `status=OK`.

### ФАЗА 1 — Backend: ядро ПРРО (3–5 днів)
**Відповідальні:** Python_Backend_Agent, DB_Admin_Agent

| # | Задача | Файли | Відповідальний |
|---|--------|-------|----------------|
| 1.1 | **Моделі ПРРО** + міграція Alembic (prro_settings, prro_shifts, prro_queue) | `models/prro_models.py`, `repositories/prro_repository.py`, `alembic/versions/*` | DB_Admin_Agent |
| 1.2 | **gRPC-клієнт**: `sendChkV2`, `ping`, `statusRro`, `infoRro`, `lastChk`, `delLastChk`, `delLastChkId`; TLS, таймаути, ретраї | `infrastructure/services/prro/grpc_client.py` | Python_Backend_Agent |
| 1.3 | **Генератор XML СЗЗД**: чек `<C>` (T=0/1), Z-звіт `<Z>`, службові Т=108–112; канонічний вигляд (Додаток А), MAC-хеш-ланцюжок; суми ×100, кількість ×1000 | `infrastructure/services/prro/xml_builder.py` | Python_Backend_Agent |
| 1.4 | **Крипто-модуль**: читання КЕП **будь-якого формату** (`.dat` ІІТ ЦСК-1, `.pfx/.p12`, `.jks`, PEM — авто-визначення), XAdES-підпис XML (signxml/cryptography), перевірка підпису; безпечне зберігання пароля (шифрування, master-key) | `infrastructure/services/prro/crypto_signer.py`, `key_store.py` | Python_Backend_Agent |
| 1.5 | **Сервіс зміни**: відкриття (Т=108), закриття (Z-звіт), один підписант на зміну, авто-нагадування >24 год, перевірка "shift already open" | `application/use_cases/prro/open_shift.py`, `close_shift.py`, `prro_service.py` | Python_Backend_Agent |
| 1.6 | **Офлайн-черга**: id_offline, резервні номери (Т=112), ліміт 168 год, синхронізація при відновленні зв'язку | `infrastructure/services/prro/offline_queue.py`, `application/use_cases/prro/sync_offline_queue.py` | Python_Backend_Agent |
| 1.7 | **Domain-правила** (один підписант, статуси, валідація) | `domain/entities/prro.py`, `domain/services/prro_domain_service.py` | Python_Backend_Agent |
| 1.8 | **API ПРРО** `api/v2/prro.py`: settings CRUD, status, shift open/close, receipts/{id}/fiscalize, journal, retry + Pydantic-схеми | `api/v2/prro.py`, `dto/prro_dto.py` | Python_Backend_Agent |

**Критерій Фази 1:** unit-тести — XML відповідає СЗЗД (канонізація), підпис валідний, черга працює.

### ФАЗА 2 — Backend: фіскалізація чеків та правило «фіскальна накладна» (3–5 днів)
**Відповідальні:** Python_Backend_Agent, DB_Admin_Agent, QA_Agent

| # | Задача | Деталі |
|---|--------|--------|
| 2.1 | **Оприбуткування fiscal_stock** при фіскальній накладній (`Invoice.is_fiscal=True` → `Product.fiscal_stock += qty`; повернення постачальнику — `-=`) | `invoice_use_cases.py` + `product_use_cases.py` |
| 2.2 | **Сплит чека**: чек зі змішаними позиціями → фіскальний чек (тільки `fiscal_quantity > 0`) + нефіскальний чек (решта), зв'язок через `split_group_id` | `receipt_use_cases.py`, `fiscalize_receipt.py` |
| 2.3 | **Фіскалізація**: `POST /receipts/sale|return` → auto/ручна фіскалізація → статус `pending → sent/failed`, `fiscal_number`, `fiscal_sent_at` | `application/use_cases/prro/fiscalize_receipt.py`, `api/v2/receipts.py` |
| 2.4 | **Валідація та часткова фіскалізація**: якщо `fiscal_stock < qty` — позиція ділиться: `fiscal_quantity = min(qty, fiscal_stock)`; фіскальний чек отримує лише `fiscal_quantity`, решта йде в нефіскальний чек | `prro_domain_service.py` |
| 2.5 | **Повернення**: чек T=1 з `RT` (тип виплати), зменшення `fiscal_stock` | `fiscalize_receipt.py` |
| 2.6 | **Z-звіт**: розрахунок підсумків зміни з чеків (обороти, податки `<TX>`) | `close_shift.py` |
| 2.7 | **QR-код** для друку фіскального чеку (посилання перевірки, п.1.5) | `print` + `print_template_service.py` |

**Критерій Фази 2:** на тестовому API повний цикл: відкриття зміни → чек OK (fiscal_number) → Z-звіт OK; товар без фіскальної накладної не фіскалізується.

### ФАЗА 3 — Frontend: вікно ПРРО та налаштування (2–4 дні)
**Відповідальні:** React_UI_UX_Agent, Tauri_Agent

| # | Задача | Файли |
|---|--------|-------|
| 3.1 | **Налаштування ПРРО**: завантаження файлу КЕП + пароль, фіскальний номер, режим (тест/прод), перевірка з'єднання (ping) | `pages/settings/PrroSettings.tsx`, `components/prro/SettingsForm.tsx` |
| 3.2 | **Вікно ПРРО**: статус (online/offline, зміна, касир), кнопки: відкрити/закрити зміну, Z-звіт, синхронізація, журнал чеків/помилок, повторна відправка | `pages/prro/PrroPage.tsx`, `components/prro/StatusCard.tsx`, `ShiftPanel.tsx`, `FiscalJournal.tsx` |
| 3.3 | **Інтеграція з POS** (`PosPage.tsx`): індикатор фіскалізації чека (pending/sent/failed), кнопка «Фіскалізувати»; у ReceiptDetail — фіскальні реквізити + QR | `pages/pos/PosPage.tsx`, `pages/receipts/` |
| 3.4 | **API-клієнт та стан**: `services/prroService.ts`, `types/prro.ts`, store (стан зміни) | `frontend/src/services/`, `types/`, `store/` |
| 3.5 | **Друк фіскального чеку** (58мм): реквізити фіскалізації + QR ДПС | Tauri (print), `useReceiptPrinter.ts` |

**Критерій Фази 3:** UX-флоу: налаштував ключ → відкрив зміну → продав → чек `sent` → закрив зміну (Z) → журнал показує історію.

### ФАЗА 4 — Тестування на тестовому API (2–3 дні)
**Відповідальні:** QA_Agent, Python_Backend_Agent, React_UI_UX_Agent

| # | Задача | Деталі |
|---|--------|--------|
| 4.1 | **Unit-тести**: xml_builder (канонізація, суми ×100/×1000), crypto_signer (підпис/перевірка), grpc_client (mock), use_cases, domain-правила, офлайн-черга | ~30+ нових тестів |
| 4.2 | **Інтеграційні** проти тестового API: повний цикл зміни (відкриття → продаж → повернення → Z-звіт) з тестовими ключами | перевірка `fiscal_number` у CheckResponse |
| 4.3 | **Edge-cases**: офлайн (168 год), дублі (ERROR_SAVE), невірний хеш (ERROR_BAD_HASH_PREV), зміна не відкрита (ERROR_NOT_OPEN_SHIFT), два ПРРО одним ключем | журнал помилок |
| 4.4 | **Аудит безпеки**: ключ/пароль не в логах, шифрування at-rest, TLS, права доступу до API ПРРО (тільки старший касир для Z) | QA_Agent |
| 4.5 | **E2E**: POS → чек → фіскалізація → статус → Z-звіт (фронтенд) | QA_Agent + React_UI_UX_Agent |
| 4.6 | Повний `pytest` (не зламати 260 наявних тестів) + `tsc --noEmit` + `cargo check` | всі зелені |

**Критерій Фази 4:** 100% тестів зелених; повний цикл зміни на тестовому API — OK.

### ФАЗА 5 — Перехід на бойове API (1–2 дні, після реєстрації ПРРО в ДПС)
**Відповідальні:** PM_Agent, Python_Backend_Agent, Git Admin Agent

| # | Задача | Деталі |
|---|--------|--------|
| 5.1 | Отримати бойові КЕП (АЦСК), зареєструвати ПРРО (фіскальний номер, договір) | **потрібен користувач** |
| 5.2 | Перемикання адреси: `prro.tax.gov.ua:443` (+ prro2), режим prod у налаштуваннях | `grpc_client.py` (конфіг) |
| 5.3 | Тест-прогін на бойовому API, моніторинг, журнал | QA_Agent |
| 5.4 | Документація: інструкція адміністратора та оператора (налаштування ключа, робота зі змінами) | System_Architect_Agent |
| 5.5 | Коміти по фазах + тег релізу | Git Admin Agent |

**Критерій Фази 5:** успішна фіскалізація реального чека; QR-перевірка на cabinet.tax.gov.ua.

---

## 5. Залежності та ризики

### 🔴 Потребують рішення/дій користувача
1. ✅ **Тестові КЕП** — користувач розмістить у `certs/prro-test/` (README містить чек-лист). Крипто-модуль має підтримувати будь-який формат ключа.
2. ✅ **Політика при нестачі `fiscal_stock` — ЧАСТКОВА ФІСКАЛІЗАЦІЯ**: позиція ділиться на фіскальну (`fiscal_quantity`) та нефіскальну; чек розділяється на два (`split_group_id`).
3. ✅ **Формат КЕП — будь-який**: `.dat` (ІІТ ЦСК-1), `.pfx/.p12`, `.jks`, PEM. Визначення формату автоматичне (за сигнатурою/розширенням).

### ✅ Вирішені питання (затверджено користувачем 2026-08-01)
1. **Тестові КЕП** → `certs/prro-test/` (README з чек-листом створено).
2. **Політика при нестачі** → **часткова фіскалізація** (`fiscal_quantity`, `split_group_id`).
3. **Формат ключа** → підтримка **будь-якого** (dat/pfx/jks/pem), авто-визначення.

### 🟠 Технічні ризики
| Ризик | Мінімізація |
|---|---|
| gRPC/TLS недоступний з середовища (офлайн Tauri) | Фаза 0.2 — перевірка; офлайн-черга з лімітом 168 год |
| Сувора валідація XML ДПС (ERROR_XML_*) | Ітеративне налагодження на тестовому API; unit-тести канонізації |
| Хеш-ланцюжок MAC (порядок чеків) | Зберігати MAC попереднього чека в `prro_shifts`/`prro_queue` |
| Безпека пароля ключа | Шифрування at-rest, master-key, не в логах |
| `datetime.utcnow` deprecated (Python 3.12) | Використовувати `datetime.now(timezone.utc)` у новому коді |

### 🟡 Обмеження
- Один КЕП (підписант) на зміну; зміну закриває старший касир.
- `delLastChk` — лише 1 раз, лише чек продажу (обрив зв'язку).
- Старі чеки (до впровадження ПРРО) не фіскалізуємо (`is_fiscal=false`).
- Передача даних — не рідше 1 разу на 72 год (регламент), офлайн-ліміт 168 год.

---

## 6. Порядок дій (найближчі кроки)

1. ✅ План **затверджено**. Рішення: тестові ключі → `certs/prro-test/`; політика нестачі — часткова фіскалізація; ключ — будь-якого формату. **Фазу 0 розпочато.**
2. **Фаза 0** (1 день): Python_Backend_Agent (proto + ping) + QA_Agent (валідація специфікації) + System_Architect_Agent (ADR).
3. **Фаза 1 → 2** (backend): DB_Admin_Agent (моделі/міграції) паралельно з Python_Backend_Agent (gRPC/XML/крипто/сервіси).
4. **Фаза 3** (frontend): React_UI_UX_Agent + Tauri_Agent.
5. **Фаза 4** (тести) → **Фаза 5** (бойовий, після реєстрації ПРРО).

**Загальна оцінка трудомісткості: 12–20 людино-днів** (залежно від доступності тестових ключів).

---

## 7. Відповідальні агенти

| Агент | Зона відповідальності |
|---|---|
| Python_Backend_Agent | gRPC, XML СЗЗД, крипто, use cases, API, інтеграція з чеками |
| DB_Admin_Agent | Моделі Prro*, міграції Alembic, індекси |
| React_UI_UX_Agent | Вікно ПРРО, налаштування, інтеграція в POS |
| Tauri_Agent | Друк фіскального чеку (QR), офлайн-клієнт |
| QA_Agent | Аудит безпеки/логіки, edge-cases, інтеграційні тести |
| System_Architect_Agent | ADR, документація, рев'ю архітектури |
| Git Admin Agent | Коміти по фазах, теги релізів |

---

## 8. Стан реалізації (оновлено 2026-08-01)

| Фаза | Статус | Результат |
|---|---|---|
| 0. Підготовка | ✅ **ЗАВЕРШЕНО** | proto + стаби gRPC, smoke-тест TLS до тестового API (канал READY), ADR-013, валідація специфікації (13/13 OK, 0 протиріч) |
| 1. Backend-ядро | ✅ **ЗАВЕРШЕНО** | gRPC-клієнт, XML СЗЗД (канонізація, ×100/×1000), крипто (dat/pfx/jks/pem + XAdES), key_store (Fernet), offline_queue, моделі + міграція 578fd283a156, use cases, API api/v2/prro.py (9 endpoints) |
| 2. Фіскалізація | ✅ **ЗАВЕРШЕНО** | fiscal_stock в накладних, split чеків (split_group_id), повернення T=1, валідація, Z-звіт, QR URL |
| 3. Frontend | ✅ **ЗАВЕРШЕНО** | PrroPage, PrroSettings (ключ+пароль, будь-який формат), prroService/prroStore, інтеграція в PosPage, маршрут /prro, QR у друці 58мм (Tauri) |
| 4. Тестування | 🟡 **ЧАСТКОВО** | 424 pytest зелених + 3 skip; tsc 0 помилок; аудит безпеки (1 CRITICAL + 5 виправлено); **інтеграційні тести на тестовому API ДПС — ОЧІКУЮТЬ тестових КЕП** (certs/prro-test/) |
| 5. Бойовий режим | ⏳ **НЕ ПОЧАТО** | після реєстрації ПРРО в ДПС + бойових ключів (certs/prro-prod/) |

### Відкриті питання для тестового API (перевірити з реальними ключами)
1. **Хеш-ланцюжок**: який саме тег/формат хешу попереднього Check очікує ДПС (звіт QA #3) — уточнити на тестовому API.
2. **MAC**: чи приймає ДПС SHA-256(DAT) без ключа для ПРРО (QA #11).
3. **Z-звіт**: значення local_number для ZREPORT (0 чи останній+1) (QA #10).
4. **Авто-фіскалізація**: винести у фон (BackgroundTasks/Celery), щоб не блокувати продаж (QA #8).

### Стан запуску сервісів (2026-08-01, вечір)

**Сервери запущено (правильні проєкти!):**
- Backend: `http://localhost:8000` (uvicorn, backend/venv, з ПРРО API) — лог `/tmp/kasa_backend.log`
- Frontend: `http://localhost:5173` (vite dev, frontend/) — меню «ПРРО» доступне

**Виправлені критичні баги (коміт `23035f9`):**
1. `AuthMiddleware` відповідав 401 на lifespan-scope → DI-контейнер не ініціалізувався → весь API давав 500
2. `LocalEventBus.publish` не знаходив хендлери за MRO (підписка на BaseDomainEvent)
3. `service_registry`: subscribe() викликався з 1 аргументом замість (event_type, handler)
4. DI-репозиторії створювались без session → `deps.py` перероблено на per-request session (як ПРРО)
5. `AuthUseCases` повертав `mock_token_...` замість JWT → тепер справжній JWT
6. ORM `User`: додано `last_login_at` (міграція `a1b2c3d4e5f6`) + domain-методи
7. `UserMapper`/`UserResponse`: обробка role (str/enum), email/phone, datetime

**venv-інфраструктура:** виправлено shebang у всіх скриптах `backend/venv/bin/` (venv був скопійований з іншого шляху — `Andriy/Bot/aegis_v3`).

**Тести:** 440 passed (було 424) — додано тести gRPC-клієнта, settings-connection.

**ПРРО test-connection (помилка -1) — діагностовано:**
- Фіскальний сервер ДПС вимагає у `Check.check_sign` **підписаний XML** (CT=111); без валідного підпису зареєстрованого підписанта — завжди `-1 ERROR_VEREFY`.
- `Key-6_test3.dat` — контейнер **ІІТ «ЦСК-1»** (ДСТУ 4145-2002, закрите крипто-ядро SDK EUSign), **недоступний для Python**.
- 🔑 **Потрібно від користувача:** конвертувати ключ у **PKCS#12 (.pfx/.p12)** або **PEM** (KeyConverter / «Користувач ЦСК-1») та завантажити його в Налаштуваннях ПРРО.
- Деталі: `docs/prro_test_connection_notes.md`.

