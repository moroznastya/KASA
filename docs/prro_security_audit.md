# Аудит безпеки та логіки ПРРО-модуля — Фаза 4

**Дата:** 2026-08-01
**Виконавець:** QA_Agent
**Область:** backend/app (services/prro, use_cases/prro, api/v2/prro.py, models/prro.py, prro_repository.py, інтеграція receipt/invoice)

---

## 1. Проблеми → Серйозність → Місце → Рекомендація

| # | Проблема | Серйозність | Місце | Рекомендація | Статус |
|---|---|---|---|---|---|
| 1 | **Витік JWT-токена в логах**: `print(f"...TokenFirstChars: {token[:20]}...")` — перші 20 символів токена потрапляють у stdout/логи кожного запиту | **CRITICAL** | `backend/app/middleware/auth_middleware.py:108` | Видалити діагностичний print | ✅ **ВИПРАВЛЕНО** |
| 2 | **Відсутність RBAC на ПРРО-ендпоінтах**: будь-який автентифікований касир міг змінювати налаштування ПРРО (ключ, пароль), закривати зміну (Z-звіт), синхронізувати чергу; `scope["user_role"]` встановлювався, але не перевірявся | **HIGH** | `backend/app/api/v2/prro.py` (PUT /settings, POST /test-connection, POST /shift/close, POST /sync) | Додано `Depends(require_admin_role)` — 403 для cashier; касиру доступні status/queue/shifts/fiscalize/open_shift | ✅ **ВИПРАВЛЕНО** |
| 3 | **Хеш-ланцюжок не реалізований**: `PrroShift.last_mac` зберігається, але НЕ підставляється в XML наступного чека (xml_builder не має параметра prev_mac/prev_hash); ризик `ERROR_BAD_HASH_PREV (-12)` на бойовому API | **HIGH** | `xml_builder.py` + `fiscalize_receipt_use_case.py` (build_receipt_xml без prev_mac) | Потрібне уточнення формату хешу (який тег XML несе хеш попереднього Check — не документовано в СЗЗД 2.1.7); після цього — передавати `last_mac` зміни в build_receipt_xml | ⚠️ НЕ виправлено (архітектурне, див. §4) |
| 4 | **float для грошових сум**: `amount=float(total)` при оновленні лічильників зміни | **MEDIUM** | `fiscalize_receipt_use_case.py:624` | Передавати `Decimal` (репозиторій вже додає через Decimal) | ✅ **ВИПРАВЛЕНО** |
| 5 | **Немає валідації prro_fn/tn/zn** (довжина/формат) при збереженні налаштувань | **MEDIUM** | `prro_settings_use_case.py` (save_settings), DTO без pattern | Додано: prro_fn — 5–15 цифр; prro_tn — 5–20 символів; prro_zn — 3–30 символів | ✅ **ВИПРАВЛЕНО** |
| 6 | **Секрети ПРРО не в .gitignore**: `.prro_master.key`, `.prro_keystore.json` (Fernet master-ключ і шифрований пароль) могли потрапити в git | **MEDIUM** | `.gitignore` | Додано `.prro_master.key`, `.prro_keystore.json`, `*.key` | ✅ **ВИПРАВЛЕНО** |
| 7 | **Невідповідність типу моделі**: `total_amount: Mapped[float]` з колонкою `Numeric(12,2)` — ризик приведення до float | **MEDIUM** | `models/prro.py` (PrroShift.total_amount) | `Mapped[Decimal]` | ✅ **ВИПРАВЛЕНО** |
| 8 | **Авто-фіскалізація блокує HTTP-відповідь**: `await _auto_fiscalize()` в тому ж запиті; gRPC 30с × 3 ретраї → продаж «зависає» до 90с (хоча помилка не впаде — try/except) | **MEDIUM** | `receipt_use_cases.py` (create_sale_receipt / create_return_receipt) | Винести фіскалізацію у фон (FastAPI BackgroundTasks / Celery / окрема черга) | ⚠️ НЕ виправлено (рекомендація) |
| 9 | **Журнал черги неповний**: `get_queue` використовує `list_pending` (лише pending/failed) — sent-чеки не видно в журналі | **MEDIUM** | `prro_status_use_case.py` (get_queue) | Окремий запит з усіма статусами + пагінація в SQL | ⚠️ НЕ виправлено (рекомендація) |
| 10 | **Z-звіт передається з `local_number=0`** — у _site_text.txt не вказано; план припускав «останній+1» | **MEDIUM** | `shift_use_case.py` (close_shift) | Перевірити на тестовому API; зафіксувати значення | ⚠️ НЕ виправлено (відкрите питання) |
| 11 | **MAC обчислюється як SHA-256(DAT) без ключа** — припущення для ПРРО (у СЗЗД для модемів MAC з ключем, розділ 5.4) | **MEDIUM** | `xml_builder.py` (compute_mac) | Перевірити на тестовому API (ping/чек) до бойового запуску | ⚠️ НЕ виправлено (потребує тесту) |
| 12 | **XSS у журналі черги**: `error` повідомлення з API ДПС зберігається в БД і повертається в JSON; якщо фронтенд рендерить через `dangerouslySetInnerHTML` — XSS | **LOW** | `prro_queue_items.error` → `get_queue` | React екранує за замовчуванням; забороняти `dangerouslySetInnerHTML` для error | ⚠️ НЕ виправлено (рекомендація) |
| 13 | **Фейковий MAC у QR**: `_fallback_mac` = SHA-1(fiscal_number) — QR не пройде перевірку ДПС без реального `id_sign/data_sign` | **LOW** | `qr_url.py` | Не друкувати QR без реального MAC; показувати попередження | ⚠️ НЕ виправлено (рекомендація) |
| 14 | **Мертвий код**: `PrroGrpcClient.open_shift` використовує `check_type=CHK` (не SERVICECHK) — метод ніде не викликається (shift_use_case будує Check сам) | **LOW** | `grpc_client.py` (open_shift) | Видалити або виправити на SERVICECHK | ⚠️ НЕ виправлено (рекомендація) |
| 15 | **Немає ліміту розміру завантажуваного файлу ключа** (`UploadFile.read()` без обмеження) — потенційна DoS | **LOW** | `api/v2/prro.py` (save_settings) | Обмежити розмір (напр., 10 МБ) | ⚠️ НЕ виправлено (рекомендація) |

---

## 2. Підтверджені сильні сторони

### 2.1. Безпека ключів/паролів
- ✅ Пароль ключа шифрується **Fernet** (`PrroKeyStore`), master-ключ з `env PRRO_MASTER_KEY` або файлу `.prro_master.key` з правами **0600**.
- ✅ Пароль **не логується** (тільки `PRRO_KEYSTORE | пароль зашифровано`) і **не повертається в API** — тільки маска `"••••"` (`PrroSettingsDTO.key_password_masked`).
- ✅ Файли ключів копіюються у `certs/prro-{mode}/`; `.gitignore` покриває `certs/`, `*.pem`, `*.pfx`, `*.p12`, `*.jks`, `*.key`, `Key-6.dat`, `Cert-6.dat` (після виправлення — і `.prro_master.key`/`.prro_keystore.json`).
- ✅ Захист від path traversal у `_save_uploaded_key` (`Path(filename).name`).

### 2.2. Логіка фіскалізації
- ✅ **Статуси чеків**: захист від повторної фіскалізації (`fiscal_status == "sent"` → `PRRO_ALREADY_FISCALIZED`); переходи `pending → sent/failed` коректні.
- ✅ **Повернення T=1**: вимагає фіскалізований оригінал (`original_receipt_id` зі статусом `sent`); `fiscal_stock += qty`.
- ✅ **Продаж T=0**: `fiscal_stock -= qty` (з `max(0, ...)`).
- ✅ **Часткова фіскалізація (split)**: `fiscal_quantity = min(plan, fiscal_stock)`, перерахунок сум пропорційно (`price × qty`, ROUND_HALF_UP), нефіскальний дублікат з `split_group_id`, `is_fiscal=False`.
- ✅ **Зміна**: відкриття Т=108 з `local_number=0`; один підписант (фіксується `signer_serial/name`); закриття Z-звітом; авто-нагадування **> 24 год** (`auto_reminder_check`).
- ✅ **Офлайн**: черга `prro_queue_items`, ліміт **168 год** (`PRRO_OFFLINE_LIMIT_HOURS`, `is_expired`), sync по порядку (pending → failed за `created_at`).
- ✅ **Дедуплікація**: при `ERROR_SAVE (-3)` / `ERROR_BAD_HASH_PREV (-12)` — перевірка `lastChk` і переведення чека в `sent` (обробка «відповідь загубилась, але чек збережено»).

### 2.3. Валідація входів та XML
- ✅ **XML-екранування** атрибутів і тексту (`_esc_attr`/`_esc_text`) — захист від XML-ін'єкцій.
- ✅ **Канонізація** за Додатком А (алфавітний порядок атрибутів, закриті теги, без зайвих пробілів).
- ✅ **Суми ×100, кількість ×1000** через `Decimal` + `ROUND_HALF_UP` (не float).
- ✅ **Правило `<E>` + підтеги `<TX>`** при кількох податкових групах реалізовано (взаємовиключення з атрибутами на `<E>`).
- ✅ **RT** (тип виплати) — тільки для T=1.
- ✅ Після виправлення: валідація `prro_fn/tn/zn`, mode — лише `test/prod`.

### 2.4. Обробка помилок
- ✅ gRPC-коди -1…-16 мапуються у `error_message` відповіді (зрозумілі повідомлення).
- ✅ Таймаути (30с) + ретраї (3, експоненційний бек-оф) — `_call_with_retry`.
- ✅ Авто-фіскалізація в `try/except` — **продаж не блокується** помилкою ПРРО.
- ✅ `PrroStatusUseCase.get_status` — best-effort (сервер недоступний → локальний стан).

### 2.5. Інші
- ✅ **SQL-ін'єкцій немає** — всі запити через SQLAlchemy ORM (параметризовані); f-string у запитах не використовуються.
- ✅ **TLS** для gRPC (`grpc.ssl_channel_credentials`).
- ✅ `invoice_use_cases` коректно оновлює `fiscal_stock` при `Invoice.is_fiscal` (оприбуткування `+`, повернення постачальнику `-`).
- ✅ QR-URL формується через `urlencode` + `Decimal` сума `0.00`.

---

## 3. Виправлення, внесені під час аудиту

| # | Файл | Зміна |
|---|---|---|
| 1 | `backend/app/middleware/auth_middleware.py` | Видалено `print` з першими 20 символами JWT-токена (витік секрету) |
| 2 | `backend/app/api/v2/prro.py` | Додано `require_admin_role` (Depends) на PUT /settings, POST /test-connection, POST /shift/close, POST /sync → 403 для cashier |
| 3 | `backend/app/application/use_cases/prro/fiscalize_receipt_use_case.py` | `amount=float(total)` → `amount=total` (Decimal) |
| 4 | `backend/app/application/use_cases/prro/prro_settings_use_case.py` | Валідація prro_fn (5–15 цифр), prro_tn (5–20), prro_zn (3–30) |
| 5 | `.gitignore` | Додано `.prro_master.key`, `.prro_keystore.json`, `*.key` |
| 6 | `backend/app/infrastructure/persistence/models/prro.py` | `total_amount: Mapped[float]` → `Mapped[Decimal]` |
| 7 | `tests/unit/api/test_prro_api.py` | Додано 3 RBAC-тести (cashier → 403 на settings/close, 200 на status) |

---

## 4. Відкриті питання (потребують рішення до бойового запуску)

1. **Формат хеш-ланцюжка** (HIGH, п.3): який XML-тег/атрибут несе «хеш XML попереднього Check»? У СЗЗД 2.1.7 не описано; потрібен уточнений формат від ДПС або тест на API.
2. **MAC без ключа** (MEDIUM, п.11): чи приймає ФСКО `SHA-256(DAT)` Base64 без додаткового ключа для ПРРО?
3. **`local_number` Z-звіту** (MEDIUM, п.10): 0 чи «останній+1»?
4. **`check_type` службових чеків** (LOW): для Т=108–112 — `SERVICECHK=3` чи `CHK=1`? (у коді — SERVICECHK через context; мертвий `grpc_client.open_shift` — CHK).
5. **`date_time`**: epoch у секундах чи мілісекундах? (код — `int(time.time())`, секунди).

---

## 5. Загальний висновок

Модуль ПРРО реалізовано **на високому рівні**: Fernet-шифрування пароля ключа, маскування в API, Decimal-математика, канонічний XML з екрануванням, коректні статуси/спліт/повернення, офлайн-черга з лімітом 168 год, дедуплікація через lastChk, TLS.

**Виправлено 2 CRITICAL/HIGH** (витік JWT у логах, відсутність RBAC) та **4 MEDIUM** (float, валідація реквізитів, .gitignore, тип моделі). Головний **незакритий HIGH-ризик** — незавершений хеш-ланцюжок попередніх чеків (`last_mac` не підставляється в наступний чек), що потребує уточнення формату від ДПС/тестового API перед бойовою фіскалізацією.

**Рекомендація:** до інтеграційного тестування на тестовому API (Фаза 4.2) обов'язково закрити питання §4 (хеш-ланцюжок, MAC, local_number Z-звіту) — без цього бойова фіскалізація ризикує `ERROR_BAD_HASH_PREV/-12` та `ERROR_XML_ZREPORT/-10`.
