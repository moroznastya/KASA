# Дизайн: Друк цінників/етикеток з прибуткової накладної

## 1. Огляд

Функціонал дозволяє користувачу надрукувати цінники або етикетки для товарів з прибуткової накладної (`Invoice`) безпосередньо зі сторінки перегляду документа.

## 2. Архітектура

```
┌──────────────────────────────────────────────────────────────────┐
│                        FRONTEND                                  │
│                                                                  │
│  DocumentViewPage.tsx                                           │
│  ┌──────────────────────────────────────────────────────┐       │
│  │ Кнопка "Друк" → відкриває модалку                    │       │
│  │                                                      │       │
│  │ InvoicePrintModal.tsx (НОВИЙ)                        │       │
│  │  - Вибір: цінники / етикетки                         │       │
│  │  - Вибір шаблону                                     │       │
│  │  - Перемикач: всі / тільки змінні ціни               │       │
│  │  - Прев'ю (PrintPreview)                             │       │
│  │  - Кнопка "Надрукувати"                              │       │
│  └──────────────────────────────────────────────────────┘       │
│                                                                  │
│  ➡️ POST /api/v1/invoices/{id}/print-items                      │
└──────────────────────────┬───────────────────────────────────────┘
                           │
┌──────────────────────────▼───────────────────────────────────────┐
│                        BACKEND                                   │
│                                                                  │
│  app/api/v1/invoices.py  (змінений файл)                        │
│  ┌──────────────────────────────────────────────────────┐       │
│  │ @router.post("/{invoice_id}/print-items")            │       │
│  │  1. Отримуємо Invoice + InvoiceItems + Products     │       │
│  │  2. Фільтруємо товари (only_changed)                │       │
│  │  3. Формуємо PriceTagProduct[]                       │       │
│  │  4. Викликаємо PriceTagPrintService                  │       │
│  │  5. Повертаємо HTML + мета                          │       │
│  └──────────────────────────────────────────────────────┘       │
│                                                                  │
│  PriceTagPrintService (існуючий, без змін)                      │
│  PrintTemplate + SystemSetting (існуючі, без змін)              │
└──────────────────────────────────────────────────────────────────┘
```

## 3. Зміни в Backend

### 3.1 Новий ендпоінт: `POST /api/v1/invoices/{id}/print-items`

**Файл:** `kasa/backend/app/api/v1/invoices.py`

```python
# ─── Схеми для запиту/відповіді (додати в кінець файлу) ─────────────

class InvoicePrintRequest(BaseModel):
    """Запит на друк цінників/етикеток з накладної."""
    print_type: Literal["price_tag", "label"] = Field(
        "price_tag",
        description="Тип друку: price_tag (цінник A4) або label (етикетка термо)",
    )
    only_changed: bool = Field(
        False,
        description="Якщо True — друкувати тільки товари з ціною, відмінною від поточної",
    )
    template_id: UUID | None = Field(
        None,
        description="ID шаблону. Якщо None — використовується дефолтний",
    )


class InvoicePrintResponse(BaseModel):
    """Відповідь після рендеру."""
    html: str = Field(..., description="Готовий до друку HTML-документ")
    total_pages: int | None = Field(None, ge=0, description="Кількість сторінок (для цінників)")
    total_labels: int = Field(..., ge=0, description="Загальна кількість цінників/етикеток")
    items: list[dict] = Field(
        [],
        description="Інформація про товари: id, title, price (ціна в накладній), "
                    "current_price (поточна ціна), price_changed (bool)",
    )


# ─── ЕНДПОІНТ: Друк цінників/етикеток з накладної ────────────────

@router.post("/{invoice_id}/print-items", response_model=InvoicePrintResponse)
async def print_items_from_invoice(
    invoice_id: UUID,
    data: InvoicePrintRequest,
    session: AsyncSession = Depends(get_session),
    current_user: User = Depends(AuthService.get_current_user),
):
    """
    Генерує HTML для друку цінників/етикеток з прибуткової накладної.

    Логіка:
    1. Завантажує Invoice з items + products
    2. Для кожного товару порівнює InvoiceItem.price з Product.price
    3. Якщо only_changed=True — фільтрує тільки товари з різною ціною
    4. Отримує шаблон (з запиту або дефолтний для типу)
    5. Отримує налаштування полів з SystemSetting
    6. Викликає PriceTagPrintService для рендеру
    7. Повертає HTML + мета-інформацію + список товарів
    """
    # ... імплементація ...
```

### 3.2 Логіка ендпоінту (псевдокод):

```python
# 1. Отримуємо накладну з товарами
result = await session.execute(
    select(Invoice)
    .options(
        selectinload(Invoice.items).selectinload(InvoiceItem.product),
        selectinload(Invoice.supplier),
    )
    .where(Invoice.id == invoice_id)
)
invoice = result.scalar_one_or_none()
if not invoice:
    raise HTTPException(status_code=404, detail="Накладну не знайдено")

# 2. Формуємо список товарів для друку
products_for_print = []
items_info = []

for item in invoice.items:
    product = item.product
    invoice_price = float(item.price)
    current_price = float(product.price) if product.price else 0.0
    price_changed = abs(invoice_price - current_price) > 0.01  # порівняння з точністю до 1 коп.

    # Якщо only_changed і ціна не змінилась — пропускаємо
    if data.only_changed and not price_changed:
        continue

    # Категорія для шаблону
    category_name = product.category.name if product.category else ""

    products_for_print.append({
        "id": str(product.id),
        "title": product.title or "",
        "price": f"{invoice_price:.2f}",
        "barcode": product.barcode or "",
        "article": product.sku or "",
        "category": category_name,
        "copies": 1,
    })

    items_info.append({
        "id": str(product.id),
        "title": product.title,
        "price": f"{invoice_price:.2f}",
        "current_price": f"{current_price:.2f}",
        "price_changed": price_changed,
    })

if not products_for_print:
    return InvoicePrintResponse(
        html=PriceTagPrintService._empty_html(
            "label" if data.print_type == "label" else "A4"
        ),
        total_pages=0,
        total_labels=0,
        items=items_info,
    )

# 3. Отримуємо шаблон
if data.template_id:
    result = await session.execute(
        select(PrintTemplate).where(PrintTemplate.id == data.template_id)
    )
    template = result.scalar_one_or_none()
    if not template:
        raise HTTPException(status_code=404, detail="Шаблон не знайдено")
else:
    # Дефолтний шаблон для типу
    template_type = "price_tag" if data.print_type == "price_tag" else "label"
    result = await session.execute(
        select(PrintTemplate).where(
            PrintTemplate.template_type == template_type,
            PrintTemplate.is_default == True,
            PrintTemplate.is_active == True,
        )
    )
    template = result.scalar_one_or_none()
    if not template:
        raise HTTPException(status_code=404, detail="Дефолтний шаблон не знайдено")

# 4. Отримуємо налаштування полів
field_key = "price_tag_fields" if data.print_type == "price_tag" else "label_fields"
fields = await _get_fields_from_settings(session, field_key, ["title", "price", "barcode"])

# 5. Формуємо налаштування для сервісу
if data.print_type == "price_tag":
    settings = {
        "width_mm": 40,
        "height_mm": 25,
        "gap_mm": 3,
        "margin_mm": 10,
        "page_width_mm": 210,
        "page_height_mm": 297,
        "fields": fields,
        "barcode_type": "code128",
        "barcode_height_mm": 12,
    }
    html = PriceTagPrintService.render_price_tags_grid(
        template.content, products_for_print, settings
    )
    # Обчислюємо кількість сторінок
    total_labels = sum(p["copies"] for p in products_for_print)
    cols, rows, per_page = PriceTagPrintService._calc_grid(
        settings["width_mm"], settings["height_mm"],
        settings["gap_mm"], settings["page_width_mm"],
        settings["page_height_mm"], settings["margin_mm"],
    )
    total_pages = max(1, math.ceil(total_labels / per_page)) if per_page > 0 else 1
    
    return InvoicePrintResponse(
        html=html,
        total_pages=total_pages,
        total_labels=total_labels,
        items=items_info,
    )
else:  # label
    settings = {
        "width_mm": 58,
        "height_mm": 40,
        "gap_mm": 2,
        "fields": fields,
        "barcode_type": "code128",
        "barcode_height_mm": 12,
    }
    html = PriceTagPrintService.render_labels_sequential(
        template.content, products_for_print, settings
    )
    total_labels = sum(p["copies"] for p in products_for_print)
    
    return InvoicePrintResponse(
        html=html,
        total_pages=None,
        total_labels=total_labels,
        items=items_info,
    )
```

### 3.3 Додаткові імпорти для invoices.py:
```python
from typing import Literal
from pydantic import Field
from app.infrastructure.persistence.models.print_template import PrintTemplate
from app.infrastructure.persistence.models.system_setting import SystemSetting
from app.infrastructure.services.price_tag_print_service import PriceTagPrintService
import math
```

## 4. Зміни в Frontend

### 4.1 Новий компонент: `InvoicePrintModal.tsx`

**Файл:** `kasa/frontend/src/components/printing/InvoicePrintModal.tsx`

```tsx
interface InvoicePrintModalProps {
  isOpen: boolean;
  onClose: () => void;
  invoiceId: string;
  invoiceItems: InvoiceItem[]; // список товарів з накладної
}

// Стан:
// - printType: 'price_tag' | 'label'
// - onlyChanged: boolean
// - templateId: string
// - previewHtml: string | null
// - isLoading: boolean
// - items: ItemInfo[] (для відображення статусу цін)

// Етапи:
// 1. Вибір налаштувань (тип, шаблон, only_changed)
// 2. Натискання "Попередній перегляд" → POST /invoices/{id}/print-items
// 3. Прев'ю через PrintPreview компонент
// 4. Натискання "Надрукувати" → друк HTML
```

### 4.2 Зміна: `DocumentViewPage.tsx`

**Зміни:**
1. Додати імпорт `Printer` з lucide-react
2. Додати стан: `showPrintModal: boolean`
3. Додати кнопку "Друк" в панелі дій (поруч з "Провести")
4. Додати модалку `InvoicePrintModal` при `showPrintModal=true`
5. Додати колонку **"Ціна в базі"** та **"Зміна ціни"** в таблицю товарів
   - Колонка з індикатором (стрілка вгору/вниз), якщо `item.price != item.product?.price`
   - Стиль: зелений `↑` (ціна зросла), червоний `↓` (ціна знизилась), сірий `–` (без змін)

**Колонка для індикатора зміни ціни:**
```tsx
// В таблиці товарів, після колонки "Ціна продажу" / "Собівартість"
<th className="table-header w-20 text-center">Зміна</th>
...
<td className="table-cell text-center">
  {(() => {
    const itemPrice = Number(item.price || 0);
    const productPrice = Number(item.product?.price || 0);
    if (itemPrice > productPrice) {
      return <span className="text-green-600 dark:text-green-400 font-bold" title="Ціна зросла">↑</span>;
    } else if (itemPrice < productPrice) {
      return <span className="text-red-600 dark:text-red-400 font-bold" title="Ціна знизилась">↓</span>;
    } else {
      return <span className="text-gray-400">–</span>;
    }
  })()}
</td>
```

### 4.3 Новий API клієнт в `printService.ts`

```typescript
/** Запит на друк з накладної */
interface InvoicePrintRequest {
  print_type: 'price_tag' | 'label';
  only_changed: boolean;
  template_id?: string | null;
}

/** Відповідь на друк з накладної */
interface InvoicePrintResponse {
  html: string;
  total_pages: number | null;
  total_labels: number;
  items: Array<{
    id: string;
    title: string;
    price: string;
    current_price: string;
    price_changed: boolean;
  }>;
}

/** Друк цінників/етикеток з накладної */
async function renderFromInvoice(
  invoiceId: string,
  data: InvoicePrintRequest,
): Promise<InvoicePrintResponse> {
  const res = await api.post<InvoicePrintResponse>(
    `/invoices/${invoiceId}/print-items`,
    data,
  );
  return res.data;
}
```

### 4.4 Новий тип в `types/print.ts`

```typescript
/** Інформація про товар для відображення в результатах друку */
export interface PrintedItemInfo {
  id: string;
  title: string;
  price: string;
  current_price: string;
  price_changed: boolean;
}
```

## 5. Потік даних (Data Flow)

```
Користувач натискає "Друк" на DocumentViewPage
│
▼
Відкривається InvoicePrintModal
│  - Вибір типу (цінники / етикетки)
│  - Вибір шаблону
│  - Перемикач "Тільки змінні ціни"
│
▼
Натискання "Попередній перегляд"
│
▼
POST /api/v1/invoices/{id}/print-items
{
  "print_type": "price_tag",
  "only_changed": true,
  "template_id": "uuid-or-null"
}
│
▼
Backend:
  1. SELECT invoice + items + products
  2. Для кожного item: порівняти price з product.price
  3. Якщо only_changed=true — фільтр
  4. Отримати шаблон + налаштування полів
  5. PriceTagPrintService.render_price_tags_grid()
  6. Повернути { html, total_pages, total_labels, items }
│
▼
Frontend:
  - Показати прев'ю (PrintPreview)
  - Показати статистику (скільки товарів, скільки зі змінною ціною)
│
▼
Користувач натискає "Надрукувати"
│
▼
Друк через браузер (window.print()) або Tauri
```

## 6. Файли для змін

### Backend:
| Файл | Зміна |
|------|-------|
| `app/api/v1/invoices.py` | Додати ендпоінт + Pydantic схеми + імпорти |
| *(без змін)* `app/schemas/print.py` | Використовуємо існуючі типи |
| *(без змін)* `app/infrastructure/services/price_tag_print_service.py` | Використовуємо існуючі методи |

### Frontend:
| Файл | Зміна |
|------|-------|
| `src/types/print.ts` | Додати тип `PrintedItemInfo` |
| `src/services/printService.ts` | Додати метод `renderFromInvoice()` |
| `src/components/printing/InvoicePrintModal.tsx` | **НОВИЙ** компонент модалки |
| `src/pages/documents/DocumentViewPage.tsx` | Додати кнопку "Друк", модалку, колонку "Зміна ціни" |

## 7. Послідовність впровадження (план)

### Крок 1: Backend ендпоінт (1-2 години)
1. Додати Pydantic схеми `InvoicePrintRequest`, `InvoicePrintResponse` в `invoices.py`
2. Додати ендпоінт `POST /{invoice_id}/print-items`
3. Реалізувати логіку: завантаження накладної, фільтрація, виклик PriceTagPrintService
4. Додати fallback на дефолтний шаблон
5. Додати `items` у відповідь (інформація про товари для UI)

### Крок 2: Frontend API та типи (30 хв)
1. Додати типи `InvoicePrintRequest`, `InvoicePrintResponse` в `types/print.ts`
2. Додати `renderFromInvoice()` в `printService.ts`

### Крок 3: InvoicePrintModal (1-2 години)
1. Створити `InvoicePrintModal.tsx` з:
   - Select для вибору типу друку
   - Select для вибору шаблону (з існуючого списку)
   - Toggle для `only_changed`
   - Кнопка "Попередній перегляд" → виклик API
   - `PrintPreview` для відображення HTML
   - Кнопка "Надрукувати"
2. Додати індикацію скільки товарів буде надруковано vs загальна кількість

### Крок 4: DocumentViewPage зміни (1 година)
1. Додати кнопку "Друк" в панель дій (тільки для invoice)
2. Додати стан `showPrintModal` та рендер `InvoicePrintModal`
3. Додати колонку "Зміна ціни" з індикатором (↑ / ↓ / –)
4. Додати колонку "Ціна в базі" (current_price)

### Крок 5: Тестування (1 година)
1. Перевірити друк для всіх товарів
2. Перевірити друк тільки для змінних цін
3. Перевірити цінники (A4) та етикетки (термо)
4. Перевірити індикатори зміни ціни
5. Перевірити edge cases: накладна без товарів, всі ціни однакові

**Загальний час: ~5-6 годин**

## 8. Edge Cases та ризики

| Edge Case | Обробка |
|-----------|---------|
| Накладна без товарів | Повернути пустий HTML з повідомленням |
| Всі ціни однакові, `only_changed=true` | Пустий результат, повідомити користувача |
| Товар не знайдено в БД (видалений) | Пропустити товар, логувати помилку |
| Немає дефолтного шаблону | Помилка 404 з пропозицією створити шаблон |
| Термопринтер не під'єднаний (Tauri) | Показати прев'ю, дати можливість зберегти/експортувати |
