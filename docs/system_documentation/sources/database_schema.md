# Схема бази даних Torgashka

## 📊 Огляд

Система використовує **PostgreSQL** з асинхронним підключенням через **SQLAlchemy 2.0** + **asyncpg**.
Всі фінансові поля зберігаються як `DECIMAL` (не `float`).

---

## 🧩 Моделі даних (16 таблиць)

### 📁 Довідники

#### 1. User (Користувач) — `users`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `name` | String(255) | ПІБ |
| `login` | String(100), unique | Логін |
| `password_hash` | String(255) | Хеш пароля (bcrypt) |
| `pin_code` | String(255), nullable | Хеш PIN-коду (bcrypt) |
| `role` | Enum('admin','cashier') | Роль |
| `is_active` | Boolean | Активний |
| `created_at` / `updated_at` | DateTime | Timestamps |

**Зв'язки:** `User → Receipt` (1:N — касир пробиває чеки)

---

#### 2. Category (Категорія) — `categories`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `name` | String(255) | Назва |
| `description` | Text, nullable | Опис |
| `parent_id` | UUID (FK → self), nullable | Батьківська категорія |
| `created_at` / `updated_at` | DateTime | Timestamps |

**Зв'язки:** self-referencing (parent_id → id) для дерева категорій

---

#### 3. Supplier (Постачальник) — `suppliers`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `name` | String(255) | Назва |
| `edrpou` | String(10), nullable | ЄДРПОУ |
| `phone` | String(20), nullable | Телефон |
| `email` | String(255), nullable | Email |
| `address` | Text, nullable | Адреса |
| `notes` | Text, nullable | Нотатки |
| `created_at` / `updated_at` | DateTime | Timestamps |

**Зв'язки:** `Supplier → Product` (1:N), `Supplier → Invoice` (1:N), `Supplier → SupplierLedger` (1:N)

---

### 📦 Товари

#### 4. Product (Товар) — `products` (Super-Product Model)

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `barcode` | String(50), unique, nullable | Основний штрих-код |
| `sku` | String(100), unique, nullable | Артикул |
| `title` | String(255) | Назва |
| `description` | Text, nullable | Опис |
| `price` | DECIMAL(10,2) | Роздрібна ціна |
| `cost_price` | DECIMAL(10,2) | Собівартість |
| `stock` | DECIMAL(10,3) | Залишок |
| `uktzed` | String(10), nullable | Код УКТЗЕД |
| `scan_excise` | Boolean | Сканувати акциз |
| `tax_rate` | DECIMAL(5,2) | Ставка ПДВ |
| `tax_group` | String(2) | Група оподаткування |
| `is_weight` | Boolean | Ваговий товар |
| `unit` | String(10) | Одиниця виміру |
| `category_id` | UUID (FK) | Категорія |
| `supplier_id` | UUID (FK), nullable | Постачальник |
| `created_at` / `updated_at` | DateTime | Timestamps |

**Індекси:** barcode (unique), sku (unique), title (btree), category_id, supplier_id, GIN trigram

---

#### 5. Barcode (Штрих-код) — `barcodes`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `product_id` | UUID (FK) | Товар |
| `barcode` | String(50), unique | Штрих-код |
| `is_primary` | Boolean | Основний |
| `created_at` / `updated_at` | DateTime | Timestamps |

---

#### 6. ProductImage (Зображення) — `product_images`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `product_id` | UUID (FK) | Товар |
| `url` | String(1024) | URL |
| `is_main` | Boolean | Головне |
| `sort_order` | Integer | Порядок |
| `created_at` / `updated_at` | DateTime | Timestamps |

---

### 📄 Документи

#### 7. Invoice (Прибуткова накладна) — `invoices`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `number` | String(50) | Номер накладної |
| `supplier_id` | UUID (FK) | Постачальник |
| `invoice_date` | DateTime | Дата накладної |
| `status` | Enum(draft/confirmed/cancelled) | Статус |
| `notes` | Text, nullable | Нотатки |
| `total_amount` | DECIMAL(12,2) | Загальна сума |
| `created_at` / `updated_at` | DateTime | Timestamps |

**Дочірня:** `InvoiceItem` (id, invoice_id, product_id, quantity, price, total)

---

#### 8. Transfer (Переміщення) — `transfers`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `number` | String(50) | Номер |
| `from_location` | String(255) | Звідки |
| `to_location` | String(255) | Куди |
| `transfer_date` | DateTime | Дата |
| `status` | Enum(draft/confirmed/cancelled) | Статус |
| `notes` | Text, nullable | Нотатки |

**Дочірня:** `TransferItem` (id, transfer_id, product_id, quantity)

---

#### 9. WriteOff (Списання) — `write_offs`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `number` | String(50) | Номер |
| `reason` | Enum(expired/damaged/defect/theft/inventory/other) | Причина |
| `write_off_date` | DateTime | Дата |
| `notes` | Text, nullable | Нотатки |

**Дочірня:** `WriteOffItem` (id, write_off_id, product_id, quantity)

---

#### 10. ReturnInvoice (Повернення постачальнику) — `return_invoices`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `number` | String(50) | Номер |
| `supplier_id` | UUID (FK) | Постачальник |
| `return_date` | DateTime | Дата |
| `status` | Enum(draft/confirmed/cancelled) | Статус |
| `notes` | Text, nullable | Нотатки |
| `total_amount` | DECIMAL(12,2) | Сума |

**Дочірня:** `ReturnInvoiceItem` (id, return_invoice_id, product_id, quantity, price, total)

---

### 🧾 Продажі

#### 11. Receipt (Чек) — `receipts`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `receipt_number` | String(50) | Номер чеку |
| `receipt_type` | Enum(sale/return) | Тип |
| `cashier_id` | UUID (FK → users) | Касир |
| `total_amount` | DECIMAL(12,2) | Сума |
| `is_return` | Boolean | Повернення |
| `notes` | Text, nullable | Нотатки |
| `created_at` | DateTime | Дата продажу |

**Дочірня:** `ReceiptItem` (id, receipt_id, product_id, quantity, price, total)

---

### 💰 Взаєморозрахунки

#### 12. SupplierLedger (Журнал оплат) — `supplier_ledger`

| Поле | Тип | Опис |
|------|-----|------|
| `id` | UUID (PK) | Ідентифікатор |
| `supplier_id` | UUID (FK) | Постачальник |
| `operation_type` | Enum(invoice/payment/return/correction) | Тип |
| `document_id` | UUID, nullable | ID документа |
| `document_number` | String(50), nullable | Номер документа |
| `amount` | DECIMAL(12,2) | Сума |
| `balance_after` | DECIMAL(12,2) | Баланс після |
| `operation_date` | DateTime | Дата |
| `notes` | Text, nullable | Нотатки |

---

## 🔗 Повна схема зв'язків

```
Category ──1:N──> Product ──1:N──> Barcode
                      │
                      ├──1:N──> ProductImage
                      │
                      ├──1:N──> InvoiceItem ──N:1── Invoice ──N:1── Supplier
                      │
                      ├──1:N──> TransferItem ──N:1── Transfer
                      │
                      ├──1:N──> WriteOffItem ──N:1── WriteOff
                      │
                      ├──1:N──> ReturnInvoiceItem ──N:1── ReturnInvoice ──N:1── Supplier
                      │
                      └──1:N──> ReceiptItem ──N:1── Receipt ──N:1── User (cashier)

Supplier ──1:N──> SupplierLedger
```

---

## 🚀 Materialized Views (план)

1. **SalesReportView** — звіт по продажах за період (суми, кількість, категорії)
2. **StockReportView** — звіт по залишках (товар, категорія, постачальник)
3. **SupplierLedgerView** — зведений баланс постачальників

---

## 📦 Seed-дані

| Сутність | Дані |
|----------|------|
| **Users** | admin/admin123 (PIN 1111), cashier/cashier123 (PIN 2222) |
| **Suppliers** | ТОВ "Галицький Дистриб'ютор", ФОП Петренко А.В. |
| **Categories** | Бакалія → Крупи та макарони, Алкоголь → Пиво, Молочні → Сири |
| **Products** | 4 товари (макарони, коньяк, сир ваговий, молоко) |
