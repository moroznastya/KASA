# 📋 ЗВІТ АУДИТУ: ХАРДКОД ПРИНТЕРІВ

**Дата:** 2026-07-31  
**Область перевірки:** `kasa/` (frontend, backend, src-tauri)  
**Метод:** grep за назвами моделей + аналіз конфігурацій

---

## 1. 🔴 HIGH: Хардкод моделей принтерів у SettingsPage

**Файл:** `frontend/src/pages/settings/SettingsPage.tsx`  
**Рядки:** 422–430

```tsx
<option value="">— Системний за замовчуванням —</option>
<option value="EPSON TM-T20">EPSON TM-T20</option>
<option value="EPSON TM-T20II">EPSON TM-T20II</option>
<option value="EPSON TM-T70">EPSON TM-T70</option>
<option value="EPSON TM-T88">EPSON TM-T88</option>
<option value="POS-58">POS-58 (USB)</option>
<option value="POS-80">POS-80 (USB)</option>
<option value="Star TSP100">Star TSP100</option>
<option value="custom">🔧 Інший (ввести вручну)...</option>
```

**Проблема:**
- 7 конкретних моделей принтерів захардкоджені в `<select>`
- Принтери мають визначатися **динамічно з системи** (через `lpstat -e`)
- Цей список ніколи не оновлюється при зміні обладнання
- Користувачі з іншими принтерами змушені використовувати "Інший" і вводити вручну

**Чому це погано:**
- Якщо в системі встановлено, наприклад, `EPSON TM-T82`, його немає в списку
- Доведеться підтримувати цей список вручну
- Порушує принцип "конфігурація, а не хардкод"

**Рекомендація:** Замінити статичний `<select>` на компонент `<PrinterSelector />`, який вже існує в `frontend/src/components/printing/PrinterSelector.tsx`. Він динамічно завантажує принтери через `lpstat -e`.

---

## 2. 🟡 MEDIUM: Хардкод назви принтера в docstring backend

**Файл:** `backend/app/api/v1/print.py`  
**Рядок:** 319

```python
"""
{
  "printer_name": "EPSON TM-T20",
  "template_type": "receipt_58mm"
}
"""
```

**Проблема:**
- Назва принтера `"EPSON TM-T20"` захардкоджена в прикладі тіла запиту в docstring
- Хоча це не функціональний код, а документація — це дезорієнтує
- В Swagger/OpenAPI документації буде показано саме цей приклад

**Рекомендація:**
Замінити на нейтральний приклад:
```python
{
  "printer_name": "system_printer_name",
  "template_type": "receipt_58mm"
}
```
Або прибрати `printer_name` з прикладу взагалі.

---

## 3. 🟢 LOW: Коментарі з моделями в print.rs

**Файл:** `frontend/src-tauri/src/print.rs`  
**Рядки:** 201–202

```rust
///   58mm папір → 384 dots (48mm друку) — Xprinter/POS-58
///   80mm папір → 576 dots (72mm друку) — Epson TM-T88
```

**Проблема:**
- Згадки `Xprinter/POS-58` та `Epson TM-T88` в коментарях
- Це довідкові коментарі для розробників, не функціональний код
- Не впливає на роботу системи

**Рекомендація:**
Бажано замінити на узагальнені:
```rust
///   58mm папір → 384 dots (48mm друку)
///   80mm папір → 576 dots (72mm друку)
```

---

## 4. ✅ Правильно: Динамічне отримання принтерів

Наступні компоненти **правильно** використовують динамічне отримання:

| Компонент | Файл | Як отримує |
|-----------|------|------------|
| `PrinterSelector` | `components/printing/PrinterSelector.tsx` | `getPrinters()` → `lpstat -e` |
| `PrintLabelsPriceTagsPage` | `pages/printing/PrintLabelsPriceTagsPage.tsx` | `settingsService.getValue('printer_name')` |
| `useReceiptPrinter` | `hooks/useReceiptPrinter.ts` | `settingsService` → ключ `printer_name` |
| `useTauri` | `hooks/useTauri.ts` | `getPrinters()` → `lpstat -e` |
| `print.ts` (Rust) | `src-tauri/src/print.rs` | `lpstat -e` |
| `seed_settings.py` | `backend/seed_settings.py` | Значення за замовчуванням: `""` (порожньо) |
| `print.ts` (service) | `services/tauri/print.ts` | `invoke('print_image', { printer_name })` |

---

## 5. ✅ Tauri конфігурації

**Файли:** `frontend/src-tauri/Cargo.toml`, `tauri.conf.json`  
**Результат:** Жодних згадок назв принтерів. ✅

---

## 📊 ЗВЕДЕНА ТАБЛИЦЯ

| # | Файл | Рядок | Тип | Критичність | Опис |
|---|------|-------|-----|--------------|------|
| 1 | `frontend/src/pages/settings/SettingsPage.tsx` | 422–430 | Хардкод моделей | 🔴 **HIGH** | 7 моделей принтерів у `<select>` |
| 2 | `backend/app/api/v1/print.py` | 319 | Docstring | 🟡 **MEDIUM** | `"EPSON TM-T20"` в прикладі |
| 3 | `frontend/src-tauri/src/print.rs` | 201–202 | Коментарі | 🟢 **LOW** | `Xprinter/POS-58`, `Epson TM-T88` |
| 4 | Всі інші файли | — | — | ✅ **OK** | Динамічне отримання через `lpstat -e` |

---

## 🎯 РЕКОМЕНДАЦІЇ

### 1. 🔴 Замінити хардкод на `PrinterSelector` в SettingsPage

**Поточна ситуація:** `SettingsPage` має власний статичний `<select>` з 7 моделями.  
**Потрібно:** Використати готовий компонент `<PrinterSelector />`.

```tsx
// Поточна реалізація (хардкод):
<div className="flex gap-2">
  <select ...>
    <option value="">— Системний —</option>
    <option value="EPSON TM-T20">EPSON TM-T20</option>
    ...
  </select>
</div>

// ⚡ Потрібно (динамічний):
<PrinterSelector 
  value={values.printer_name || ''} 
  onChange={(name) => onFieldChange('printer_name', name)} 
/>
```

### 2. 🟡 Виправити docstring в backend

```diff
-   "printer_name": "EPSON TM-T20",
+   "printer_name": "system_printer_name",
```

### 3. 🟢 Прибрати моделі з коментарів

```diff
-   ///   58mm папір → 384 dots (48mm друку) — Xprinter/POS-58
-   ///   80mm папір → 576 dots (72mm друку) — Epson TM-T88
+   ///   58mm папір → 384 dots (48mm друку)
+   ///   80mm папір → 576 dots (72mm друку)
```
