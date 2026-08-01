# 📋 ЗВІТ АУДИТУ: МОДУЛЬ ШРИФТУ ДРУКУ (PrintFontService)

**Дата:** 2026-07-31
**Область:** `print_font_service.py`, `print.py`, `print_templates.py`, `invoices.py`, `SettingsPage.tsx`
**Метод:** читання коду + grep-аналіз покриття + pytest + tsc

---

## ✅ БАЗОВІ ПЕРЕВІРКИ

| Перевірка | Результат |
|-----------|-----------|
| `python -m pytest tests/ -q` | ✅ **260 passed** (33.50s) — включає 11 тестів PrintFontService, 14 XSS-тестів, 19 тестів валідатора |
| `npx tsc --noEmit` | ✅ 0 помилок |
| Попередній CRITICAL (stored XSS у price_tag_print_service) | ✅ **ВИПРАВЛЕНО** — `_escape_html(html.escape quote=True)` застосовується до всіх полів товару |

---

## 🟠 ЗНАЙДЕНІ ПРОБЛЕМИ

### 🟠 ВАЖЛИВО — Пропущений endpoint: /invoices/{id}/print-items не застосовує шрифт

**Місце:** `backend/app/api/v1/invoices.py:619-650`

**Проблема:** `apply_font_to_html` викликається в 5 місцях:
- ✅ `print.py:236` → render_price_tags
- ✅ `print.py:345` → render_labels
- ✅ `print.py:546` → test_print (цінник/етикетка)
- ✅ `print.py:653` → test_print (чек)
- ✅ `print_templates.py:312` → render_template (чеки)

АЛЕ **`/invoices/{id}/print-items`** (invoices.py, рядки 619/636 — рендер цінників/етикеток з накладної, повернення html= на рядку 650) — **НЕ покритий**. Цей ендпоінт використовує фронтенд `PrintFromInvoiceModal` через `printService.renderInvoicePrintItems()` → друк цінників/етикеток з накладної буде зі шрифтом за замовчуванням, ігноруючи налаштування.

**Рекомендація:**
```python
# invoices.py, перед return InvoicePrintResponse(...)
font = await PrintFontService.get_font_family(session)
html = PrintFontService.apply_font_to_html(html, font)
```
(додати імпорт `PrintFontService` — перевірити, чи вже імпортовано).

---

### 🟠 ВАЖЛИВО — font_family вставляється в HTML без валідації (ін'єкція)

**Місце:** `print_font_service.py:apply_font_to_html()` + `settings.py` (PUT /settings/{key})

**Проблема:** `font_family` — це рядок з БД, який вставляється **без жодної санації** в 3 HTML-контексти:
1. `style="font-family: {font}"` — вихід з атрибута через `"` → `Arial" onmouseover="alert(1)`
2. `<style>body { font-family: {font}; }</style>` — вихід з тега через `</style><script>alert(1)</script>`
3. Вставлений `<style>` тег — той самий вектор

**Куди потрапляє результат:**
- Прев'ю в SettingsPage — iframe `sandbox="allow-same-origin"` (скрипти заблоковані) → низький ризик ✅
- `PrintLabelsPriceTagsPage.tsx:539` / `PrintFromInvoiceModal.tsx:564` — `dangerouslySetInnerHTML` **у головний DOM** → `<img onerror>`/`<svg onload>` **виконуються** 🔴
- `printViaBrowser()` — `window.open('', '_blank')` + `document.write` → скрипти виконуються в same-origin вікні 🔴

**Реальний ризик:** значення вводить тільки адмін (PUT /settings — admin-only), і `SettingsValueValidator` пропускає `print_font_family` БЕЗ обмежень (гілка «Інші ключі»). Адмін і так має повний HTML-ін'єкційний доступ через редагування шаблонів → **підвищення привілеїв немає**. АЛЕ це stored XSS, що спрацьовує в браузері кожного касира при друку, і порушує defense-in-depth.

**Рекомендація (дешево й ефективно):**
1. У `SettingsValueValidator` додати правило для `print_font_family`: whitelist-регулярка `^[A-Za-z0-9 ,'"-]+$` (покриває всі 14 пресетів + типові ручні значення типу `Ubuntu, sans-serif`; відсікає `<`, `>`, `;`, `}`, `{`, `(` тощо).
2. Опційно — у `apply_font_to_html` додати той самий фільтр як defense-in-depth (якщо значення не проходить — використати DEFAULT_FONT_FAMILY).
3. На фронтенді — обмежити custom-input тими самими символами.

---

### 🟡 НЕЗНАЧНО — EDGE CASE: значення `'custom'` → невалідний CSS (тихий no-op)

**Місце:** `SettingsPage.tsx` (select + custom input) → `print_font_service.py`

**Проблема:** Якщо адмін вибрав «✏️ Інший...» і **не ввів значення** → зберігається літерал `'custom'` → `apply_font_to_html` робить `font-family: custom` → невалідний CSS → браузер мовчки застосовує fallback. Помилки немає, налаштування просто «не працює». При повторному відкритті сторінки input знову порожній — адмін може не зрозуміти, чому шрифт не змінився.

**Рекомендація (3 рівні захисту):**
1. **Frontend (обов'язково):** при збереженні, якщо `values.print_font_family === 'custom'` → підставити дефолт `'Arial, sans-serif'` або показати toast «Введіть назву шрифту».
2. **Backend get_font_family (бажано):** `if font in ('custom', '', None): font = DEFAULT_FONT_FAMILY`.
3. **Тест:** додати unit-тест на значення `'custom'`.

---

### 🟡 НЕЗНАЧНО — Select бреше при значенні з БД, якого немає в списку

**Місце:** `SettingsPage.tsx` (блок «Шрифт для друку»)

**Проблема:** Якщо в БД збережено кастомний шрифт, якого немає в `PRINT_FONT_OPTIONS` (напр. `'Ubuntu, sans-serif'`):
```tsx
<select value={values.print_font_family || 'Arial, sans-serif'}>
```
жоден `<option>` не збігається → браузер **візуально показує першу опцію** («Arial (Liberation Sans)»), хоча реально активний `'Ubuntu, sans-serif'`. При цьому custom-input (показаний коректно, бо `isCustom=true`) містить `'Ubuntu, sans-serif'`. Підсумок: **дропдаун показує «Arial», а друк буде шрифтом Ubuntu** — оманливий UI, адмін може випадково перезаписати кастомний шрифт, обравши пресет.

**Рекомендація:**
```tsx
const currentFont = values.print_font_family || 'Arial, sans-serif';
const options = PRINT_FONT_OPTIONS.some(o => o.value === currentFont)
  ? PRINT_FONT_OPTIONS
  : [...PRINT_FONT_OPTIONS, { value: currentFont, label: `${currentFont} (збережений)` }];
```
і рендерити `options`.

---

### 🟡 НЕЗНАЧНО — @font-face руйнується заміною

**Місце:** `print_font_service.py` — regex замінює ВСІ `font-family`, включно з тими, що всередині `@font-face { ... }`

**Проблема:** `@font-face { font-family: 'BarcodeFont'; src: url(...) }` → заміна дає `font-family: Arial, sans-serif` всередині @font-face — це **невалідно** (у @font-face дозволено тільки одне ім'я сімейства) → правило @font-face анулюється → кастомний шрифт зникає з документа. Якщо шаблони використовують @font-face (спеціальні шрифти для штрих-кодів, фірмові шрифти) — вони зламаються.

**Рекомендація:** перед заміною витягти блоки `@font-face {...}` у тимчасові плейсхолдери, виконати заміну, повернути блоки назад. АБО задокументувати обмеження і не використовувати @font-face у шаблонах друку.

---

### 🟡 НЕЗНАЧНО — `!important` губиться при заміні

**Місце:** `print_font_service.py`

**Проблема:** `font-family: X !important;` → значення захоплюється цілком (`X !important`) і замінюється на `font-family: {font}` **без** `!important`. Якщо в шаблоні є інше правило з `!important`/вищою специфічністю — вибраний шрифт може НЕ застосуватися до цього елемента (часткова відмова налаштування).

**Рекомендація:** зберегти суфікс:
```python
m = _FONT_FAMILY_RE.search(...)
suffix = " !important" if value.rstrip().endswith("!important") else ""
# заміна: f"font-family: {font}{suffix}"
```
або в lambda: `lambda m: f"font-family: {font_family}" + (" !important" if m.group(1).rstrip().endswith('!important') else '')`.

---

### 🟢 НЕЗНАЧНО — Документація: «кешується в пам'яті» не відповідає коду

**Місце:** `print_font_service.py` docstring модуля та `get_font_family`

**Проблема:** У docstring заявлено кешування, але реалізація читає БД **при кожному виклику** (кожен рендер = +1 запит). З одного боку, відсутність кешу — це навіть добре (зміна шрифту миттєво застосовується без рестарту). Але docstring вводить в оману.

**Рекомендація:** прибрати згадку про кешування з docstring (або додати реальний кеш з TTL і інвалідацією при зміні налаштування — складніше).

---

## ✅ ЩО ПРАЦЮЄ КОРЕКТНО

| Перевірка | Статус |
|-----------|--------|
| Regex: лапки `"Times New Roman"`, `'Courier New'`, змішані значення | ✅ тест `test_replaces_all_font_family` (5 замін) |
| Regex: декілька font-family в одному HTML | ✅ re.sub замінює всі |
| Regex: inline style без `;` | ✅ тест `test_replaces_inline_without_semicolon` |
| Regex: `var(--x)` (CSS-змінні) | ✅ зіставляється класом `[^;"'}]+` (не покрито тестом — додати) |
| Regex: IGNORECASE + пробіли навколо `:` | ✅ `font-family\s*:\s*` |
| Вставка `<style>body{...}</style>` перед `</head>` (case-insensitive find, коректний slice) | ✅ тест `test_inserts_style_before_head_end` |
| Вставка в кінець, якщо `</head>` немає | ✅ тест `test_appends_style_when_no_head_end` |
| Порожні html/font → без змін | ✅ тест `test_empty_inputs_unchanged` |
| Спецсимволи `$` і `\` у шрифті (lambda-заміна) | ✅ тест `test_special_chars_in_font_safe` |
| Ідемпотентність повторного застосування | ✅ тест `test_idempotent_application` |
| get_font_family: дефолт при відсутності/порожньому значенні | ✅ тести |
| get_font_family: визначення module='printing' за префіксом `print_` | ✅ тест |
| Інтеграція: 5 з 6 HTML-шляхів покриті | ✅ (крім invoices.py) |
| Frontend: `onFieldChange('print_font_family', value)` — ключ збігається з backend | ✅ |
| Frontend: custom-input показується при значенні не зі списку | ✅ (з нюансом відображення select, див. вище) |

---

## 📊 ЗВЕДЕНА ТАБЛИЦЯ

| # | Критичність | Проблема | Файл |
|---|-------------|----------|------|
| 1 | 🟠 ВАЖЛИВО | `/invoices/{id}/print-items` не застосовує шрифт (пропущений шлях друку з накладної) | `invoices.py:619-650` |
| 2 | 🟠 ВАЖЛИВО | font_family без валідації → HTML/атрибут-ін'єкція (admin-only, defense-in-depth) | `print_font_service.py` + `settings_value_validator.py` |
| 3 | 🟡 НЕЗНАЧНО | EDGE CASE: значення `'custom'` → невалідний CSS, тихий no-op | `SettingsPage.tsx` + `print_font_service.py` |
| 4 | 🟡 НЕЗНАЧНО | Select показує першу опцію замість кастомного значення з БД (оманливий UI) | `SettingsPage.tsx` |
| 5 | 🟡 НЕЗНАЧНО | @font-face руйнується заміною font-family | `print_font_service.py` |
| 6 | 🟡 НЕЗНАЧНО | `!important` губиться при заміні | `print_font_service.py` |
| 7 | 🟢 НЕЗНАЧНО | Docstring «кешується в пам'яті» не відповідає коду | `print_font_service.py` |

---

## 🎯 РЕКОМЕНДАЦІЇ (пріоритет)

1. **🟠 Застосувати шрифт у `invoices.py`** — додати `get_font_family` + `apply_font_to_html` перед `return InvoicePrintResponse(html=...)`.
2. **🟠 Валідація font_family** — правило в `SettingsValueValidator`: `^[A-Za-z0-9 ,'"-]+$`; у `apply_font_to_html` — той самий фільтр як fallback (не пройшло → DEFAULT_FONT_FAMILY).
3. **🟡 'custom' edge case** — фронтенд: при збереженні `'custom'` → дефолт або toast; бекенд: `get_font_family` трактує `'custom'` як дефолт.
4. **🟡 Select** — динамічно додавати поточне значення з БД в options, якщо його немає в списку.
5. **🟡 @font-face** — виключити блоки @font-face із заміни (плейсхолдери).
6. **🟡 !important** — зберігати суфікс при заміні.
7. **🟢 Тести** — додати: `'custom'` edge case, `</style><script>` ін'єкцію (що вона не проходить валідацію), `var(--x)`, `!important`, @font-face.
8. **🟢 Docstring** — прибрати згадку про кешування.

---

## ⚖️ ВЕРДИКТ: **ПОТРІБНІ НЕВЕЛИКІ ВИПРАВЛЕННЯ**

Модуль написаний якісно: regex коректний і покритий тестами, 5 із 6 шляхів інтеграції на місці, попередній CRITICAL-XSS виправлено, 260 тестів і tsc проходять. **Критичних проблем немає.** Обов'язково до виправлення: пропущений endpoint `invoices.py` (функціональний дефект) та валідація font_family (безпека/гігієна). Решта — дрібні UX-та CSS-нюанси.
