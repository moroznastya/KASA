# Карта контексту системи Torgashka

**Версія:** 1.0.0  
**Дата:** 2025-01-20  
**Статус:** Проєкт (Contract First)

---

## 1. Контекстна карта системи

```
┌─────────────────────────────────────────────────────────────────┐
│                     ЗОВНІШНІ СИСТЕМИ                            │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │  Користувач  │  │  Сканер      │  │  Податкова (ДПС)    │   │
│  │  (касир)     │  │  штрих-кодів │  │  (REGPACK API)      │   │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────┘   │
└─────────┼─────────────────┼─────────────────────┼───────────────┘
          │                 │                     │
          ▼                 ▼                     ▼
┌─────────────────────────────────────────────────────────────────┐
│                     API GATEWAY (FastAPI)                       │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                   MIDDLEWARE                              │   │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────┐  │   │
│  │  │   CORS      │  │     Auth     │  │  Context       │  │   │
│  │  │             │  │  (JWT Bearer)│  │  Provider      │  │   │
│  │  └─────────────┘  └──────────────┘  └────────────────┘  │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    API ROUTERS (v1)                       │   │
│  │  ┌────────┐┌────────┐┌────────┐┌────────┐┌──────────┐  │   │
│  │  │Товари  ││Категор.││Докумен.││Чеки    ││Взаємороз.│  │   │
│  │  │/products││/categor││/docs   ││/receipt││/ledger   │  │   │
│  │  └────┬───┘└────┬───┘└────┬───┘└────┬───┘└─────┬────┘  │   │
│  └───────┼─────────┼─────────┼─────────┼──────────┼───────┘   │
└──────────┼─────────┼─────────┼─────────┼──────────┼───────────┘
           │         │         │         │          │
           ▼         ▼         ▼         ▼          ▼
┌─────────────────────────────────────────────────────────────────┐
│                    EVENT BUS (Шина подій)                       │
│                                                                  │
│  ProductCreated  StockChanged  InvoiceConfirmed  ReceiptCreated  │
│  ──────────────  ────────────  ─────────────────  ─────────────  │
│  ProductUpdated  StockLow     InvoiceCancelled   PaymentReceived │
│  ──────────────  ────────────  ─────────────────  ─────────────  │
│  ProductDeleted  StockMoved   ReturnConfirmed    LedgerUpdated   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
           │         │         │         │          │
           ▼         ▼         ▼         ▼          ▼
┌─────────────────────────────────────────────────────────────────┐
│                    МОДУЛІ (СЕРВІСИ)                              │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │ ProductModule │  │  StockModule  │  │  DocumentModule      │   │
│  │              │  │              │  │                      │   │
│  │ - CRUD       │  │ - Залишки    │  │ - Прибуткові накл.  │   │
│  │ - Пошук      │  │ - Резерви    │  │ - Переміщення       │   │
│  │ - Штрих-коди │  │ - Мінімуми   │  │ - Списання          │   │
│  └──────┬───────┘  └──────┬───────┘  │ - Повернення        │   │
│         │                │           └──────────┬───────────┘   │
│         │                │                      │               │
│  ┌──────┴───────┐  ┌─────┴───────┐  ┌──────────┴───────────┐   │
│  │ LedgerModule  │  │  AuthModule  │  │  ReceiptModule       │   │
│  │              │  │             │  │                      │   │
│  │ - Журнал     │  │ - Логін     │  │ - Створення чеків   │   │
│  │ - Баланс     │  │ - JWT       │  │ - Розрахунок суми   │   │
│  │ - Історія    │  │ - Ролі      │  │ - Податкові чеки    │   │
│  └──────┬───────┘  └──────┬──────┘  └──────────────────────┘   │
└─────────┼─────────────────┼────────────────────────────────────┘
          │                 │
          ▼                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                     СХОВИЩЕ ДАНИХ                               │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              PostgreSQL (через SQLAlchemy 2.0)            │   │
│  │                                                          │   │
│  │  products │ categories │ suppliers │ users │ invoices    │   │
│  │  barcodes │ receipts   │ transfers │ write_offs          │   │
│  │  supplier_ledger │ return_invoices │ product_images      │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Redis (кеш, черги) — майбутнє               │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. Межі контексту (Bounded Contexts)

### 2.1 Product Context (Товари)

| Аспект | Опис |
|--------|------|
| **Відповідальність** | Управління товарами, категоріями, штрих-кодами |
| **Моделі** | Product, Category, Barcode, ProductImage |
| **Події** | `product.created`, `product.updated`, `product.deleted` |
| **Залежить від** | AuthModule (авторизація) |
| **API** | `/api/v1/products`, `/api/v1/categories` |

### 2.2 Stock Context (Склад)

| Аспект | Опис |
|--------|------|
| **Відповідальність** | Управління залишками, резервами, мінімальними рівнями |
| **Моделі** | Product.stock (поле), StockMovement (майбутнє) |
| **Події** | `stock.changed`, `stock.low`, `stock.moved` |
| **Залежить від** | ProductModule, DocumentModule |
| **API** | Частина `/api/v1/products` |

### 2.3 Document Context (Документи)

| Аспект | Опис |
|--------|------|
| **Відповідальність** | Прибуткові накладні, переміщення, списання, повернення |
| **Моделі** | Invoice, Transfer, WriteOff, ReturnInvoice + Items |
| **Події** | `invoice.confirmed`, `invoice.cancelled`, `transfer.confirmed`, `return.confirmed` |
| **Залежить від** | StockModule, LedgerModule |
| **API** | `/api/v1/invoices`, `/api/v1/transfers`, `/api/v1/write-offs`, `/api/v1/return-invoices` |

### 2.4 Ledger Context (Взаєморозрахунки)

| Аспект | Опис |
|--------|------|
| **Відповідальність** | Журнал операцій, баланс постачальників |
| **Моделі** | SupplierLedger |
| **Події** | `ledger.entry_created`, `ledger.balance_changed` |
| **Залежить від** | DocumentModule |
| **API** | `/api/v1/ledger` |

### 2.5 Auth Context (Авторизація)

| Аспект | Опис |
|--------|------|
| **Відповідальність** | Аутентифікація, авторизація, управління користувачами |
| **Моделі** | User |
| **Події** | `user.logged_in`, `user.logged_out`, `user.created` |
| **Залежить від** | Немає |
| **API** | `/api/v1/auth`, `/api/v1/users` |

### 2.6 Receipt Context (Чеки)

| Аспект | Опис |
|--------|------|
| **Відповідальність** | Створення чеків продажу, розрахунок сум |
| **Моделі** | Receipt, ReceiptItem |
| **Події** | `receipt.created`, `receipt.cancelled` |
| **Залежить від** | StockModule, ProductModule |
| **API** | `/api/v1/receipts` |

---

## 3. Взаємодія між контекстами

```
                    ┌─────────────┐
                    │    Auth     │
                    │   Context   │
                    └──────┬──────┘
                           │ (авторизація всіх запитів)
                           ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Product   │◄────│   Event    │────►│    Stock    │
│   Context   │     │    Bus     │     │   Context   │
└──────┬──────┘     └─────────────┘     └──────┬──────┘
       │                                        │
       │  product.created ──────────────────────┤
       │  product.updated ──────────────────────┤
       │  product.deleted ──────────────────────┘
       │
       │                        ┌─────────────┐
       │────────────────────────│  Document   │
       │  stock.changed ◄───────│   Context   │
       │                        └──────┬──────┘
       │                               │
       │                        ┌──────┴──────┐
       │                        │   Ledger    │
       │────────────────────────│   Context   │
          invoice.confirmed ───►│             │
          return.confirmed ────►└─────────────┘
       
       ┌──────────────────────────────────────────┐
       │              Receipt Context              │
       │  (створює stock.changed при продажу)      │
       └──────────────────────────────────────────┘
```

---

## 4. Контекстна діаграма (C4 рівень)

### Level 1: System Context

```
[Користувач] ──► [Torgashka API] ──► [PostgreSQL]
                      │
                      └──► [Зовнішні системи (майбутнє)]
```

### Level 2: Container Context

```
[Користувач] ──► [FastAPI App] ──► [PostgreSQL]
                      │
              ┌───────┼───────┐
              ▼       ▼       ▼
        [Product] [Document] [Auth]
        [Service] [Service]  [Service]
```

### Level 3: Component Context

```
[FastAPI App]
    │
    ├── Middleware
    │   ├── AuthMiddleware (JWT перевірка)
    │   └── ContextMiddleware (встановлює SystemContext)
    │
    ├── API Routers
    │   ├── products.py ──► ProductService
    │   ├── invoices.py ──► DocumentService
    │   ├── receipts.py ──► ReceiptService
    │   └── ...
    │
    ├── Event Bus
    │   ├── publish(event)
    │   └── subscribe(event_type, handler)
    │
    ├── Services (через Protocols)
    │   ├── ProductService implements ProductModuleInterface
    │   ├── DocumentService implements DocumentModuleInterface
    │   ├── LedgerService implements LedgerModuleInterface
    │   └── AuthService implements AuthModuleInterface
    │
    ├── DI Container
    │   ├── register(name, factory)
    │   └── resolve(name) → instance
    │
    └── Service Registry
        ├── register(service_info)
        └── get_info(name) → ServiceInfo
```

---

## 5. Правила контекстної ізоляції

1. **Модуль НЕ може напряму викликати методи іншого модуля** — тільки через Event Bus
2. **Модуль НЕ може напряму читати БД іншого модуля** — тільки через його API/події
3. **Кожен модуль має власний набір подій** (publishes/subscribes)
4. **Контекст користувача передається через ContextProvider** — не через параметри
5. **Всі залежності реєструються в DI Container** — жодного `new Service(session)` в коді
