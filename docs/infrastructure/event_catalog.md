# Каталог подій Torgashka

**Версія:** 1.0.0  
**Дата:** 2025-01-20  
**Статус:** Проєкт (Contract First)

---

## 1. Формат подій

Всі події в системі мають єдиний формат:

```python
@dataclass
class Event:
    event_id: str          # UUID (генерується автоматично)
    event_type: str        # Назва події (наприклад, "product.created")
    timestamp: datetime    # Час створення (UTC)
    payload: Dict[str, Any]  # Дані події
    source_module: str     # Модуль-джерело
```

---

## 2. Категорії подій

| Категорія | Префікс | Приклади |
|-----------|---------|----------|
| Товари | `product.` | `product.created`, `product.updated` |
| Склад | `stock.` | `stock.changed`, `stock.low` |
| Документи | `invoice.`, `transfer.`, `write_off.`, `return.` | `invoice.confirmed` |
| Чеки | `receipt.` | `receipt.created` |
| Взаєморозрахунки | `ledger.` | `ledger.entry_created` |
| Користувачі | `user.` | `user.logged_in` |

---

## 3. Детальний опис подій

### 3.1 Товари (Product)

#### `product.created`

**Опис:** Новий товар створено.

**Джерело:** `ProductModule`

**Підписники:** `StockModule` (ініціалізація залишку)

**Payload:**
```json
{
    "product_id": "uuid",
    "barcode": "4821234567890",
    "sku": "ART-001",
    "title": "Кава Львівська 200г",
    "category_id": "uuid",
    "supplier_id": "uuid",
    "price": 120.00,
    "cost_price": 85.00,
    "initial_stock": 0.000
}
```

---

#### `product.updated`

**Опис:** Дані товару оновлено.

**Джерело:** `ProductModule`

**Підписники:** `StockModule` (якщо змінено ціну)

**Payload:**
```json
{
    "product_id": "uuid",
    "changed_fields": ["price", "title"],
    "old_values": {"price": 100.00},
    "new_values": {"price": 120.00}
}
```

---

#### `product.deleted`

**Опис:** Товар видалено.

**Джерело:** `ProductModule`

**Підписники:** `StockModule` (очищення залишків)

**Payload:**
```json
{
    "product_id": "uuid",
    "barcode": "4821234567890",
    "title": "Кава Львівська 200г"
}
```

---

### 3.2 Склад (Stock)

#### `stock.changed`

**Опис:** Залишок товару змінено.

**Джерело:** `StockModule` (або `DocumentModule` через Event Bus)

**Підписники:** `ProductModule` (оновити кеш), `NotificationModule` (перевірка мінімуму)

**Payload:**
```json
{
    "product_id": "uuid",
    "product_title": "Кава Львівська 200г",
    "old_quantity": 50.000,
    "change": 10.000,
    "new_quantity": 60.000,
    "reason": "invoice_confirmed",
    "document_id": "uuid",
    "document_type": "invoice",
    "document_number": "INV-2025-001"
}
```

---

#### `stock.low`

**Опис:** Залишок товару нижче мінімального рівня.

**Джерело:** `StockModule`

**Підписники:** `NotificationModule` (сповіщення), `DocumentModule` (автоматичне замовлення)

**Payload:**
```json
{
    "product_id": "uuid",
    "product_title": "Кава Львівська 200г",
    "current_stock": 5.000,
    "min_stock": 10.000,
    "supplier_id": "uuid",
    "supplier_name": "ТОВ Кава-Трейд"
}
```

---

#### `stock.moved`

**Опис:** Товар переміщено між складами.

**Джерело:** `DocumentModule` (при підтвердженні Transfer)

**Підписники:** `StockModule` (оновити залишки)

**Payload:**
```json
{
    "product_id": "uuid",
    "quantity": 20.000,
    "from_location": "Склад №1",
    "to_location": "Склад №2",
    "document_id": "uuid",
    "document_number": "TR-2025-001"
}
```

---

### 3.3 Документи (Document)

#### `invoice.confirmed`

**Опис:** Прибуткову накладну підтверджено.

**Джерело:** `DocumentModule`

**Підписники:** `StockModule` (збільшити залишки), `LedgerModule` (створити запис)

**Payload:**
```json
{
    "invoice_id": "uuid",
    "invoice_number": "INV-2025-001",
    "supplier_id": "uuid",
    "supplier_name": "ТОВ Кава-Трейд",
    "total_amount": 15000.00,
    "invoice_date": "2025-01-20",
    "items": [
        {
            "product_id": "uuid",
            "product_title": "Кава Львівська 200г",
            "quantity": 100.000,
            "price": 85.00,
            "total": 8500.00
        }
    ],
    "confirmed_by": "user_uuid",
    "confirmed_at": "2025-01-20T15:30:00Z"
}
```

---

#### `invoice.cancelled`

**Опис:** Прибуткову накладну скасовано.

**Джерело:** `DocumentModule`

**Підписники:** `StockModule` (зменшити залишки), `LedgerModule` (створити запис)

**Payload:**
```json
{
    "invoice_id": "uuid",
    "invoice_number": "INV-2025-001",
    "reason": "Помилка в накладній",
    "cancelled_by": "user_uuid",
    "cancelled_at": "2025-01-21T10:00:00Z"
}
```

---

#### `transfer.confirmed`

**Опис:** Переміщення підтверджено.

**Джерело:** `DocumentModule`

**Підписники:** `StockModule` (оновити залишки)

**Payload:**
```json
{
    "transfer_id": "uuid",
    "transfer_number": "TR-2025-001",
    "items": [
        {
            "product_id": "uuid",
            "quantity": 20.000
        }
    ]
}
```

---

#### `return.confirmed`

**Опис:** Повернення постачальнику підтверджено.

**Джерело:** `DocumentModule`

**Підписники:** `StockModule` (зменшити залишки), `LedgerModule` (створити запис)

**Payload:**
```json
{
    "return_id": "uuid",
    "return_number": "RET-2025-001",
    "supplier_id": "uuid",
    "total_amount": -3000.00,
    "items": [...]
}
```

---

### 3.4 Чеки (Receipt)

#### `receipt.created`

**Опис:** Чек продажу створено.

**Джерело:** `ReceiptModule`

**Підписники:** `StockModule` (зменшити залишки), `ProductModule` (оновити статистику)

**Payload:**
```json
{
    "receipt_id": "uuid",
    "receipt_number": "RCPT-2025-0001",
    "total_amount": 450.00,
    "payment_type": "cash",
    "items": [
        {
            "product_id": "uuid",
            "product_title": "Кава Львівська 200г",
            "quantity": 2.000,
            "price": 120.00,
            "total": 240.00
        }
    ],
    "created_by": "user_uuid",
    "created_at": "2025-01-20T16:45:00Z"
}
```

---

#### `receipt.cancelled`

**Опис:** Чек продажу скасовано (повернення товару).

**Джерело:** `ReceiptModule`

**Підписники:** `StockModule` (збільшити залишки)

**Payload:**
```json
{
    "receipt_id": "uuid",
    "receipt_number": "RCPT-2025-0001",
    "reason": "Повернення товару",
    "cancelled_by": "user_uuid"
}
```

---

### 3.5 Взаєморозрахунки (Ledger)

#### `ledger.entry_created`

**Опис:** Створено новий запис у журналі взаєморозрахунків.

**Джерело:** `LedgerModule`

**Підписники:** `NotificationModule` (якщо баланс перевищив ліміт)

**Payload:**
```json
{
    "entry_id": "uuid",
    "supplier_id": "uuid",
    "supplier_name": "ТОВ Кава-Трейд",
    "operation_type": "invoice",
    "amount": 15000.00,
    "balance_after": 45000.00,
    "document_id": "uuid",
    "document_number": "INV-2025-001",
    "operation_date": "2025-01-20"
}
```

---

### 3.6 Користувачі (User)

#### `user.logged_in`

**Опис:** Користувач увійшов в систему.

**Джерело:** `AuthModule`

**Підписники:** `AuditModule` (майбутнє)

**Payload:**
```json
{
    "user_id": "uuid",
    "user_login": "admin",
    "user_role": "admin",
    "login_method": "password",
    "ip_address": "192.168.1.100",
    "logged_in_at": "2025-01-20T09:00:00Z"
}
```

---

#### `user.created`

**Опис:** Створено нового користувача.

**Джерело:** `AuthModule`

**Підписники:** `AuditModule` (майбутнє)

**Payload:**
```json
{
    "user_id": "uuid",
    "user_login": "cashier1",
    "user_role": "cashier",
    "created_by": "admin_uuid"
}
```

---

## 4. Матриця подій

| Подія | ProductModule | StockModule | DocumentModule | LedgerModule | ReceiptModule | AuthModule |
|-------|:---:|:---:|:---:|:---:|:---:|:---:|
| `product.created` | 🔵 | 🟢 |  |  |  |  |
| `product.updated` | 🔵 | 🟢 |  |  |  |  |
| `product.deleted` | 🔵 | 🟢 |  |  |  |  |
| `stock.changed` | 🟢 | 🔵 | 🟢 |  |  |  |
| `stock.low` |  | 🔵 | 🟢 |  |  |  |
| `stock.moved` |  | 🟢 | 🔵 |  |  |  |
| `invoice.confirmed` |  | 🟢 | 🔵 | 🟢 |  |  |
| `invoice.cancelled` |  | 🟢 | 🔵 | 🟢 |  |  |
| `transfer.confirmed` |  | 🟢 | 🔵 |  |  |  |
| `return.confirmed` |  | 🟢 | 🔵 | 🟢 |  |  |
| `receipt.created` | 🟢 | 🟢 |  |  | 🔵 |  |
| `receipt.cancelled` |  | 🟢 |  |  | 🔵 |  |
| `ledger.entry_created` |  |  |  | 🔵 |  |  |
| `user.logged_in` |  |  |  |  |  | 🔵 |
| `user.created` |  |  |  |  |  | 🔵 |

**Легенда:** 🔵 = публікує, 🟢 = підписується

---

## 5. Правила роботи з подіями

1. **Одна подія — одна відповідальність.** Не змішувати різні типи даних в одній події.
2. **Події імутабельні.** Після публікації подію не можна змінити.
3. **Події не містять секретів.** Ніколи не передавати паролі, токени в payload.
4. **ID події унікальний.** Використовується для дедуплікації при повторній обробці.
5. **Обробники подій не блокують один одного.** Кожен підписник отримує копію події.
6. **Порядок обробки не гарантований.** Не покладатися на порядок виконання обробників.
7. **Події зберігаються в історії.** Event Bus зберігає всі події для аудиту.
