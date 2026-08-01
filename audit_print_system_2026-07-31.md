# 📋 ЗВІТ АУДИТУ: СИСТЕМА ДРУКУ KASA POS (15 файлів)

**Дата:** 2026-07-31
**Область:** kasa/backend, kasa/frontend
**Метод:** читання всіх 15 файлів + tsc + cargo check + import + grep по проєкту

---

## ✅ БАЗОВІ ПЕРЕВІРКИ

| Перевірка | Результат |
|-----------|-----------|
| `npx tsc --noEmit` | ✅ 0 помилок |
| `cargo check` | ✅ 0 помилок, 0 warnings |
| `python3 -c "import app.main"` | ✅ OK |
| Міграційний ланцюг `f89706f0cc14 → 56b4c1696966` | ✅ OK |

---

## 🔴 ЗНАЙДЕНІ ПРОБЛЕМИ

### 🔴 CRITICAL — Stored XSS через дані товару в шаблонах друку

**Місце:** `backend/app/infrastructure/services/price_tag_print_service.py` → `_render_single()` (рядки ~230-270), `_generate_barcode_svg()` (~90-100)

**Проблема:** Поля товару вставляються в HTML **БЕЗ екранування**:
```python
replacements: dict[str, str] = {
    "title": product.get("title", ""),      # ← RAW, не екрановано
    "barcode": barcode_val,                  # ← RAW
    "article": product.get("article", ""),   # ← RAW
    "category": product.get("category", ""), # ← RAW
}
```

**Ланцюг експлуатації:**
1. Товар із назвою `<img src=x onerror=alert(document.cookie)>`
2. `POST /print/price-tags/render` → HTML містить payload без екранування
3. Frontend вставляє HTML через `dangerouslySetInnerHTML` **у головний DOM**:
   - `PrintLabelsPriceTagsPage.tsx:539` (hidden div для html2canvas)
   - `PrintFromInvoiceModal.tsx:564` (hidden div)
4. `<img onerror>` спрацьовує в контексті головної сторінки → **виконання JS**

**Також:** `_generate_barcode_svg` вставляє `barcode_text` у `<span>` без екранування.

**Рекомендація:** Екранувати всі поля товару (title, barcode, article, category, created_date) перед підстановкою в `_render_single`, аналогічно `escapeHtml()` у `useReceiptPrinter.ts`. АБО екранувати на фронтенді перед `dangerouslySetInnerHTML`.

---

### 🟠 HIGH — PUT /settings/{key}: відсутня валідація значень на backend

**Місце:** `backend/app/api/v1/settings.py` → `update_setting()` (upsert)

**Проблема:**
- Приймає **будь-яке значення** для будь-якого ключа: `price_tag_width = -500`, `print_copies = 999999`, `barcode_type = "<script>..."`
- `_determine_value_type()` лише визначає тип, але не перевіряє діапазони/дозволені значення
- `barcode_type` зберігається як завгодно (наприклад, "ean13" з seed-опцій), але API рендеру приймає тільки `Literal["code128","qr"]` — розбіжність

**Захист:** Фронтенд валідує (`if (savedBarcodeType === 'qr' || savedBarcodeType === 'code128')`), але backend — точка примусу.

**Рекомендація:** Додати Pydantic-валідацію або сервісний шар перевірки:
- `price_tag_width/height/label_width/height` → 10..200
- `*_gap` → 0..20, `*_margin` → 0..50
- `barcode_type` → whitelist `["code128","qr"]` (або узгодити з seed-опціями)
- `print_copies` → 1..100

---

### 🟠 MEDIUM — Rust: немає верхньої межі для copies (DoS/OOM)

**Місце:** `frontend/src-tauri/src/commands/print.rs` (рядки ~57, ~100) та `print.rs` → `build_multi_copy_escpos()`

```rust
let copies = copies.unwrap_or(1).max(1);  // тільки мінімум!
```

**Проблема:** `copies = 4_000_000_000` → `data.extend_from_slice(raster_block)` повторюється 4 млрд разів → вичерпання пам'яті. Для локального застосунку ризик низький, але коду достатньо одного рядка.

**Рекомендація:**
```rust
let copies = copies.unwrap_or(1).clamp(1, 100);
```
(аналогічно в `build_multi_copy_escpos`).

---

### 🟠 MEDIUM — show_logo не працює: жоден шаблон не використовує {{show_logo}}

**Місце:** `frontend/src/hooks/useReceiptPrinter.ts` (рядки 270-275)

```ts
const buildExtraRenderData = useCallback((): Record<string, string> => {
    return { show_logo: showLogo ? 'true' : 'false' };
}, [showLogo]);
```

**Проблема:** `grep "show_logo"` по шаблонах/сидах/БД → **0 результатів**. Змінна передається в рендер, але жоден шаблон її не використовує → налаштування «Показувати логотип» не має жодного ефекту.

**Рекомендація:** Або додати `{{show_logo}}` в дефолтні шаблони чеків, або прибрати налаштування/задокументувати як «для майбутнього використання».

---

### 🟠 MEDIUM — PrintLabelsPriceTagsPage: налаштування не перезавантажуються при зміні типу

**Місце:** `frontend/src/pages/printing/PrintLabelsPriceTagsPage.tsx` → `useEffect(load, [])` (рядки ~74-108) + `handleTypeChange()` (рядки ~140-155)

**Проблема:** Ефект завантаження запускається **один раз** з початковим `printType='price_tag'`. При перемиканні на `label`:
- `templateId` скидається → дефолтний шаблон підставляється ✅
- але `widthMm/heightMm/gapMm/marginMm` **зберігають значення цінника** (40/25/3/10) замість завантаження `label_width/label_height/label_gap` з БД (58/40/2)

**Рекомендація:** Додати `printType` у залежності `useEffect` (перезавантажувати ключі типу при зміні) або в `handleTypeChange` викликати завантаження для нового типу.

---

### 🟡 LOW — /print/preview не існує (невідповідність опису задачі)

**Місце:** `backend/app/api/v1/print.py`

**Проблема:** У ТЗ заявлено «додано /print/preview», але такого роута немає. Frontend (SettingsPage) коректно використовує `/print/test` — функціонально нічого не зламано, але опис не відповідає коду.

**Рекомендація:** Або реалізувати `/print/preview` як окремий ендпоінт, або виправити документацію ТЗ.

---

### 🟡 LOW — Дублювання ключів налаштувань (5+ файлів)

**Місце:** Ключі `price_tag_width`, `label_gap`, `barcode_type` тощо захардкоджені в:
1. `backend/app/api/v1/settings.py` — `PRINTING_KEYS`
2. `backend/seed_settings.py` — DEFAULT_SETTINGS
3. `backend/alembic/versions/f89706f0cc14...` — NEW_SETTINGS
4. `frontend/.../PrintLabelsPriceTagsPage.tsx` — SETTINGS_KEYS + getTypeSettingsKeys
5. `frontend/.../PrintSettingsPanel.tsx` — settingsPrefix
6. `frontend/.../PrintFromInvoiceModal.tsx` — DEFAULT_SETTINGS
7. `frontend/src/pages/settings/SettingsPage.tsx` — filter списки

**Ризик:** зміна ключа в одному місці не оновлюється в інших → тихі поломки.

**Рекомендація:** Створити єдиний константний модуль (напр. `frontend/src/constants/printSettings.ts` + backend `app/domain/constants/printing.py`).

---

### 🟡 LOW — Бізнес-логіка в роутері print.py

**Місце:** `backend/app/api/v1/print.py` → `_test_print_price_tag_or_label()` (3-рівнева резолюція шаблону: template_id → налаштування → is_default)

**Проблема:** Резолюція шаблону (бізнес-логіка) виконується в API-шарі, а не в сервісі.

**Рекомендація:** Винести в `PrintTemplateService.resolve_for_print_type()` аналогічно тому, як `get_default_for_type` вже інкапсульовано.

---

### 🟢 LOW — Dead code (невикористовувані експорти)

**Місце:**
- `usePrintAsImage.ts` → `captureToDataUrl` (не викликається ніде)
- `services/tauri/print.ts` → `openCashDrawer`, `getSystemInfo`, `printRasterImage` (не використовуються у frontend)
- Rust: команди `open_cash_drawer`, `get_system_info`, `save_receipt_image` існують, але не викликаються з UI

**Рекомендація:** Прибрати або задокументувати як «для зовнішнього API».

---

## ✅ ЩО ПРАЦЮЄ КОРЕКТНО

| Перевірка | Статус |
|-----------|--------|
| SQL-ін'єкції (settings.py, seed, міграція) | ✅ Всі запити параметризовані (SQLAlchemy select / text з bind) |
| Зворотна сумісність printImage (2 аргументи) | ✅ TS сигнатура + Rust Option дефолти (copies=1, auto_cut=true) |
| Option обробка в Rust | ✅ `unwrap_or(1)`, `unwrap_or(true)` — без panic |
| Panic при copies=0 | ✅ `max(1)` клампує до 1 + тест `test_build_multi_copy_escpos_zero_copies_clamped` |
| Межі масиву в raster (x+b < w) | ✅ перевірено |
| PNG decode помилки | ✅ `load_from_memory` → Err(General) + тест |
| useReceiptPrinter: receipt null | ✅ guard `if (!receipt) throw` в printReceipt/generatePreview/ensureHtmlInDom |
| PrintFromInvoiceModal: шлях друку | ✅ Tauri+label → printImage; price_tag → window.print; браузер → window.print |
| Звільнення ресурсів (iframe) | ✅ `printViaBrowser` видаляє iframe через 1000ms; hidden div зникає з модалкою |
| Міграція: ідемпотентність | ✅ `ON CONFLICT (key) DO NOTHING`, key має `unique=True` |
| Міграція: типи колонок | ✅ value=Text, value_type=String(20), options=Text |
| TS/Rust типи (snake_case) | ✅ `PrintImageData` збігається з Rust-структурою |
| try/catch + toast в async | ✅ скрізь |
| Принтер недоступний (PrinterSelector) | ✅ жовтий бейдж-попередження, не блокування |
| Fallback без Tauri | ✅ `printViaBrowser()` / `window.print()` |

---

## 📊 ЗВЕДЕНА ТАБЛИЦЯ ПРОБЛЕМ

| # | Критичність | Проблема | Файл |
|---|-------------|----------|------|
| 1 | 🔴 **CRITICAL** | Stored XSS: title/barcode без екранування → dangerouslySetInnerHTML | `price_tag_print_service.py` + `PrintLabelsPriceTagsPage.tsx:539`, `PrintFromInvoiceModal.tsx:564` |
| 2 | 🟠 HIGH | PUT /settings/{key} без валідації значень | `backend/app/api/v1/settings.py` |
| 3 | 🟠 MEDIUM | Rust copies без верхньої межі (OOM) | `commands/print.rs`, `print.rs` |
| 4 | 🟠 MEDIUM | show_logo інфертний (жоден шаблон не використовує) | `useReceiptPrinter.ts` |
| 5 | 🟠 MEDIUM | Налаштування не перезавантажуються при зміні типу | `PrintLabelsPriceTagsPage.tsx` |
| 6 | 🟡 LOW | /print/preview відсутній (невідповідність ТЗ) | `backend/app/api/v1/print.py` |
| 7 | 🟡 LOW | Дублювання ключів налаштувань (7 місць) | вся система |
| 8 | 🟡 LOW | Бізнес-логіка в роутері | `print.py` |
| 9 | 🟢 LOW | Dead code (4 функції) | `print.ts`, `usePrintAsImage.ts` |

---

## 🎯 РЕКОМЕНДАЦІЇ (пріоритет)

1. **🔴 ТЕРМІНОВО:** Екранувати поля товару в `PriceTagPrintService._render_single()` та `_generate_barcode_svg()` (title, barcode, article, category, created_date, barcode_text). Використати `html.escape()` (Python) або аналог `escapeHtml()`.
2. **🟠 HIGH:** Додати валідацію значень в `update_setting()` — діапазони для числових ключів, whitelist для `barcode_type`.
3. **🟠 MEDIUM:** Обмежити `copies` до 1..100 в Rust (`clamp(1, 100)`).
4. **🟠 MEDIUM:** Вирішити долю `show_logo` (додати в шаблони або прибрати).
5. **🟠 MEDIUM:** Додати `printType` у залежності `useEffect` завантаження налаштувань у PrintLabelsPriceTagsPage.
6. **🟡 LOW:** Винести ключі налаштувань у спільні константи; перенести резолюцію шаблону в сервіс; прибрати dead code.

---

## ⚖️ ВЕРДИКТ: **ПОТРІБНІ ВИПРАВЛЕННЯ**

Код компілюється (tsc ✅, cargo ✅, import ✅), логіка друку та зворотна сумісність працюють. АЛЕ:
- **1 CRITICAL** (stored XSS через дані товару) — має бути виправлений негайно
- **1 HIGH** (валідація settings) — бажано до релізу
- **4 MEDIUM** — планові виправлення

Після виправлення CRITICAL/HIGH рекомендується повторний аудит.
