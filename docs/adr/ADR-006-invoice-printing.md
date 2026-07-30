# ADR 006: Друк цінників/етикеток з прибуткової накладної

## Статус
✅ Прийнято

## Контекст
Потрібно додати функціонал друку цінників та етикеток безпосередньо з прибуткової накладної (Invoice). Користувач переглядає накладну, бачить список товарів і може однією кнопкою надрукувати цінники для цих товарів.

**Варіанти реалізації:**

### Варіант A: Новий backend ендпоінт
```
POST /invoices/{id}/print-items
```
- Backend отримує ID накладної, завантажує товари з цінами
- Порівнює ціни з накладної з поточними цінами товарів (для режиму "тільки змінні")
- Викликає існуючий `PriceTagPrintService.render_price_tags_grid()` або `render_labels_sequential()`
- Повертає HTML + мета-інформацію

### Варіант B: Логіка на фронтенді
- Frontend завантажує накладну, трансформує `InvoiceItem[]` у `PriceTagProduct[]`
- Викликає існуючі ендпоінти `POST /print/price-tags/render` та `POST /print/labels/render`
- Порівнює ціни на фронтенді

## Рішення
**Обрано Варіант A** — новий backend ендпоінт `POST /invoices/{id}/print-items`.

### Обґрунтування:
1. **Business Logic в Application Layer** — порівняння цін (InvoiceItem.price vs Product.price) є бізнес-логікою, яка має бути на backend згідно Clean Architecture
2. **Ізольованість** — фронтенд не потребує знати про структуру цін, достатньо отримати готовий HTML
3. **Менше мережевих запитів** — один запит замість двох (спочатку fetch invoice, потім render)
4. **Розширюваність** — в майбутньому можна додати логування, аудит, валідації
5. **Повторне використання** — легше додати друк з інших типів документів (замовлення, повернення)

### Місце в Clean Architecture:
```
Presentation Layer (API v1)
  └── POST /invoices/{id}/print-items

Application Layer
  └── InvoicePrintUseCase (новий use case)

Domain Layer
  └── PriceTagPrintService (існуючий, в infrastructure)
  └── Invoice, InvoiceItem, Product (існуючі)

Infrastructure Layer
  └── PriceTagPrintService (існуючий сервіс рендеру)
  └── PrintTemplate (існуюча модель)
  └── SystemSetting (існуюча модель — поля шаблону)
```

## Наслідки
✅ + Чиста архітектура: бізнес-логіка на backend
✅ + Мінімум змін на фронтенді
✅ + Один запит = готовий HTML
✅ + Можливість повторного використання для інших документів
⚠️ - Новий ендпоінт (потрібно документувати)
⚠️ - Існуючий PriceTagPrintService знаходиться в Infrastructure (не Domain), але це прийнятно, оскільки це сервіс рендеру, а не бізнес-логіка
