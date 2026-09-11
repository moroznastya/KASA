# 🧩 Карта модулів Torgashka

> **Версія:** 2.0.0  
> **Дата:** 2026-07-20  
> **Принцип:** Module cohesion — високе зчеплення всередині, слабка зв'язність між модулями

---

## 1️⃣ МОДУЛЬНА СТРУКТУРА (HIGH-LEVEL)

```
kasa/
├── backend/
│   ├── app/
│   │   ├── api/v1/              # [Presentation] HTTP роутери
│   │   ├── application/         # [Application] Use Cases, DTO, Mappers
│   │   ├── domain/              # [Domain] Entities, VOs, Events, Repositories
│   │   ├── infrastructure/      # [Infrastructure] Persistence, DI, External
│   │   ├── core/                # [Shared] Config, Exceptions, Security (utils)
│   │   ├── main.py              # Точка входу FastAPI
│   │   └── database.py          # Підключення до БД (залишається)
│   └── alembic/                 # Міграції (переміщаються в infrastructure/)
│
├── frontend/
│   └── src/
│       ├── components/          # [Presentation] UI компоненти
│       ├── hooks/               # [Presentation] React хуки
│       ├── pages/               # [Presentation] Сторінки
│       ├── services/            # [Application] API клієнти
│       ├── store/               # [Application] Глобальний стан
│       ├── types/               # [Domain] TypeScript типи
│       └── utils/               # [Shared] Утиліти
│
└── docs/                        # Документація
```

---

## 2️⃣ ДЕТАЛЬНА КАРТА МОДУЛІВ (BACKEND)

### 2.1 Модуль: Products (Товари)

```
┌─────────────────────────────────────────────────────────────────────┐
│  МОДУЛЬ: Products                                                    │
│  Відповідальність: Управління номенклатурою товарів                 │
│  Власник: Product Owner                                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Presentation Layer:                                                │
│  └── api/v1/products.py                                             │
│      - list_products()        GET    /api/v1/products               │
│      - get_product()          GET    /api/v1/products/{id}          │
│      - get_by_barcode()       GET    /api/v1/products/barcode/{bc}  │
│      - create_product()       POST   /api/v1/products               │
│      - update_product()       PUT    /api/v1/products/{id}          │
│      - delete_product()       DELETE /api/v1/products/{id}          │
│                                                                     │
│  Application Layer:                                                 │
│  └── application/use_cases/product_use_case.py                      │
│      - create_product(dto) → ProductResponse                        │
│      - update_product(id, dto) → ProductResponse                    │
│      - delete_product(id) → None                                    │
│      - get_product(id) → ProductResponse                            │
│      - search_products(params) → PaginatedResult<ProductResponse>   │
│      - get_by_barcode(barcode) → ProductResponse                    │
│      - update_stock(id, quantity) → ProductResponse                 │
│                                                                     │
│  └── application/dto/product_dto.py                                 │
│      - ProductCreateDTO, ProductUpdateDTO, ProductResponseDTO       │
│      - ProductSearchParamsDTO                                       │
│                                                                     │
│  └── application/mappers/product_mapper.py                          │
│      - dto_to_entity(dto) → Product                                 │
│      - entity_to_dto(entity) → ProductResponseDTO                   │
│      - entity_to_response(entity) → ProductResponse                 │
│                                                                     │
│  Domain Layer:                                                      │
│  └── domain/entities/product.py                                     │
│      - class Product: Aggregate Root                                │
│        Fields: id, barcode, sku, title, description, price,         │
│                cost_price, stock, uktzed, scan_excise,              │
│                tax_rate, tax_group, is_weight, unit,                │
│                category_id, supplier_id, created_at, updated_at      │
│        Methods:                                                     │
│          - update_stock(quantity: Decimal) → void                   │
│          - change_price(new_price: Money) → void                    │
│          - validate_barcode() → bool                                │
│          - is_low_stock(threshold: Decimal) → bool                  │
│                                                                     │
│  └── domain/value_objects/                                          │
│      - money.py: class Money(amount: Decimal, currency: str)        │
│      - barcode.py: class Barcode(value: str, type: BarcodeType)     │
│      - quantity.py: class Quantity(value: Decimal, unit: str)       │
│      - tax_rate.py: class TaxRate(rate: Decimal)                    │
│                                                                     │
│  └── domain/repositories/i_product_repository.py                    │
│      - class IProductRepository(ABC):                               │
│        @abstractmethod save(product: Product) → Product             │
│        @abstractmethod find_by_id(id: UUID) → Product | None        │
│        @abstractmethod find_by_barcode(bc: str) → Product | None    │
│        @abstractmethod find_by_sku(sku: str) → Product | None       │
│        @abstractmethod search(params) → tuple[list[Product], int]   │
│        @abstractmethod delete(id: UUID) → None                      │
│                                                                     │
│  └── domain/events/product_events.py                                │
│      - class ProductCreated(DomainEvent): ...                       │
│      - class ProductUpdated(DomainEvent): ...                       │
│      - class StockChanged(DomainEvent): ...                         │
│                                                                     │
│  Infrastructure Layer:                                              │
│  └── infrastructure/persistence/repositories/product_repository.py  │
│      - class ProductRepository(IProductRepository):                 │
│        Реалізація через SQLAlchemy ProductModel                     │
│                                                                     │
│  └── infrastructure/persistence/models/product_model.py             │
│      - class ProductModel(Base): SQLAlchemy ORM модель              │
│                                                                     │
│  ─── Залежності: Category, Supplier (через репозиторії)            │
│  ─── Події: ProductCreated → (LedgerModule, SearchIndex)           │
│  ─── Тести: test_product_use_case.py (мок репозиторію)             │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 2.2 Модуль: Inventory (Складський облік)

```
┌─────────────────────────────────────────────────────────────────────┐
│  МОДУЛЬ: Inventory                                                   │
│  Відповідальність: Документообіг складу, залишки                   │
│  Підмодулі: Invoices, Transfers, WriteOffs, ReturnInvoices         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Presentation Layer:                                                │
│  ├── api/v1/invoices.py          # Прибуткові накладні              │
│  ├── api/v1/transfers.py         # Переміщення                      │
│  ├── api/v1/write_offs.py        # Списання                         │
│  ├── api/v1/return_invoices.py   # Повернення постачальнику         │
│  └── api/v1/documents.py         # Узагальнений перегляд            │
│                                                                     │
│  Application Layer:                                                 │
│  ├── application/use_cases/invoice_use_case.py                      │
│  │   - create_draft() → InvoiceResponse                             │
│  │   - confirm(id) → InvoiceResponse (проведення)                   │
│  │   - cancel(id) → InvoiceResponse (скасування)                    │
│  │   - get_by_id(id) → InvoiceResponse                              │
│  │   - search(params) → PaginatedResult                             │
│  │                                                                   │
│  ├── application/use_cases/transfer_use_case.py                     │
│  ├── application/use_cases/write_off_use_case.py                    │
│  ├── application/use_cases/return_invoice_use_case.py               │
│  └── application/use_cases/document_use_case.py                     │
│                                                                     │
│  Domain Layer:                                                      │
│  ├── domain/entities/invoice.py                                     │
│  │   - class Invoice(AggregateRoot):                                │
│  │     Fields: id, number, supplier_id, invoice_date,               │
│  │             status(Draft|Confirmed|Cancelled), notes,             │
│  │             total_amount, items: list[InvoiceItem]                │
│  │     Methods:                                                     │
│  │       - confirm() → void (зміна статусу, валідація)              │
│  │       - cancel() → void                                          │
│  │       - add_item(product_id, qty, price) → void                  │
│  │       - calculate_total() → Money                                │
│  │                                                                   │
│  ├── domain/entities/transfer.py                                    │
│  ├── domain/entities/write_off.py                                   │
│  ├── domain/entities/return_invoice.py                              │
│  │                                                                   │
│  ├── domain/services/stock_service.py                               │
│  │   - apply_document(document) → list[StockChange]                 │
│  │   - rollback_document(document) → list[StockChange]              │
│  │   - check_availability(product_id, qty) → bool                   │
│  │   - handle_negative_stock(product_id) → Alert                    │
│  │                                                                   │
│  ├── domain/repositories/i_invoice_repository.py                    │
│  ├── domain/repositories/i_transfer_repository.py                   │
│  ├── domain/repositories/i_write_off_repository.py                  │
│  └── domain/repositories/i_return_invoice_repository.py             │
│                                                                     │
│  Infrastructure Layer:                                              │
│  ├── infrastructure/persistence/repositories/invoice_repository.py  │
│  ├── infrastructure/persistence/repositories/transfer_repository.py │
│  ├── infrastructure/persistence/repositories/write_off_repository.py│
│  ├── infrastructure/persistence/repositories/return_repository.py   │
│  └── infrastructure/persistence/models/                             │
│      ├── invoice_model.py                                           │
│      ├── transfer_model.py                                          │
│      ├── write_off_model.py                                         │
│      └── return_invoice_model.py                                    │
│                                                                     │
│  ─── Залежності: Products (через IProductRepository)               │
│  ─── Події: InvoiceConfirmed → (StockModule, LedgerModule)         │
│  ─── Події: InvoiceCancelled → (StockModule, LedgerModule)         │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 2.3 Модуль: Sales (Продажі)

```
┌─────────────────────────────────────────────────────────────────────┐
│  МОДУЛЬ: Sales                                                       │
│  Відповідальність: POS-термінал, чеки, фіскалізація                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Presentation Layer:                                                │
│  └── api/v1/receipts.py                                             │
│      - create_receipt()     POST /api/v1/receipts                   │
│      - get_receipt()        GET  /api/v1/receipts/{id}              │
│      - list_receipts()      GET  /api/v1/receipts                   │
│      - return_receipt()     POST /api/v1/receipts/{id}/return       │
│                                                                     │
│  Application Layer:                                                 │
│  └── application/use_cases/receipt_use_case.py                      │
│      - create_receipt(dto) → ReceiptResponse                        │
│      - return_receipt(id, items) → ReceiptResponse                  │
│      - get_receipt(id) → ReceiptResponse                            │
│      - search(params) → PaginatedResult                             │
│                                                                     │
│  Domain Layer:                                                      │
│  ├── domain/entities/receipt.py                                     │
│  │   - class Receipt(AggregateRoot):                                │
│  │     Fields: id, number, items, total, payment_method,            │
│  │             cashier_id, created_at                               │
│  │     Methods:                                                     │
│  │       - add_item(product, qty, price) → void                     │
│  │       - remove_item(item_id) → void                              │
│  │       - calculate_total() → Money                                │
│  │       - apply_discount(percent) → void                           │
│  │       - process_payment(method, amount) → PaymentResult          │
│  │                                                                   │
│  ├── domain/services/pricing_service.py                             │
│  │   - calculate_price_with_vat(price, tax_rate) → Money            │
│  │   - apply_discount(price, percent) → Money                       │
│  │   - round_to_copecks(amount) → Money                             │
│  │                                                                   │
│  └── domain/repositories/i_receipt_repository.py                    │
│                                                                     │
│  Infrastructure Layer:                                              │
│  ├── infrastructure/persistence/repositories/receipt_repository.py  │
│  ├── infrastructure/persistence/models/receipt_model.py             │
│  └── infrastructure/external/prro/                                  │
│      - prro_client.py  # Інтеграція з ПРРО (фіскальний чек)        │
│                                                                     │
│  ─── Залежності: Products (через IProductRepository)               │
│  ─── Події: ReceiptCreated → (StockModule, ReportsModule)          │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 2.4 Модуль: Finance (Фінанси)

```
┌─────────────────────────────────────────────────────────────────────┐
│  МОДУЛЬ: Finance                                                     │
│  Відповідальність: Взаєморозрахунки, баланси, платежі              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Presentation Layer:                                                │
│  └── api/v1/ledger.py                                               │
│      - get_ledger()          GET  /api/v1/ledger                    │
│      - get_supplier_balance() GET /api/v1/ledger/suppliers/{id}     │
│      - register_payment()    POST /api/v1/ledger/payment            │
│                                                                     │
│  Application Layer:                                                 │
│  └── application/use_cases/ledger_use_case.py                       │
│      - create_entry(dto) → LedgerEntryResponse                      │
│      - get_supplier_balance(id) → BalanceResponse                   │
│      - get_history(supplier_id, params) → PaginatedResult           │
│      - register_payment(dto) → LedgerEntryResponse                  │
│                                                                     │
│  Domain Layer:                                                      │
│  ├── domain/entities/supplier_ledger.py                             │
│  │   - class LedgerEntry:                                           │
│  │     Fields: id, supplier_id, operation_type, amount,             │
│  │             balance_after, document_id, operation_date           │
│  │                                                                   │
│  ├── domain/services/balance_service.py                             │
│  │   - calculate_balance(supplier_id) → Money                       │
│  │   - validate_payment(amount, balance) → bool                     │
│  │                                                                   │
│  └── domain/repositories/i_ledger_repository.py                     │
│                                                                     │
│  Infrastructure Layer:                                              │
│  ├── infrastructure/persistence/repositories/ledger_repository.py   │
│  └── infrastructure/persistence/models/supplier_ledger_model.py     │
│                                                                     │
│  ─── Залежності: Suppliers (через ISupplierRepository)             │
│  ─── Слухає події: InvoiceConfirmed, ReturnInvoiceConfirmed        │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 2.5 Модуль: Auth (Авторизація)

```
┌─────────────────────────────────────────────────────────────────────┐
│  МОДУЛЬ: Auth                                                        │
│  Відповідальність: Користувачі, ролі, аутентифікація               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Presentation Layer:                                                │
│  └── api/v1/users.py                                                │
│      - login()              POST /api/v1/auth/login                 │
│      - login_pin()          POST /api/v1/auth/login-pin             │
│      - refresh_token()      POST /api/v1/auth/refresh               │
│      - list_users()         GET  /api/v1/users                      │
│      - create_user()        POST /api/v1/users                      │
│      - update_user()        PUT  /api/v1/users/{id}                 │
│      - delete_user()        DELETE /api/v1/users/{id}               │
│                                                                     │
│  Application Layer:                                                 │
│  └── application/use_cases/auth_use_case.py                         │
│      - login(login, password) → AuthResult                          │
│      - login_by_pin(login, pin) → AuthResult                        │
│      - refresh_token(token) → AuthResult                            │
│      - get_current_user(token) → User                               │
│      - require_admin(user) → User                                   │
│                                                                     │
│  └── application/use_cases/user_use_case.py                         │
│      - create_user(dto) → UserResponse                              │
│      - update_user(id, dto) → UserResponse                          │
│      - deactivate_user(id) → None                                   │
│                                                                     │
│  Domain Layer:                                                      │
│  ├── domain/entities/user.py                                        │
│  │   - class User:                                                  │
│  │     Fields: id, login, password_hash, pin_code, role,            │
│  │             full_name, is_active                                 │
│  │     Methods:                                                     │
│  │       - verify_password(password) → bool                         │
│  │       - verify_pin(pin) → bool                                   │
│  │       - change_password(old, new) → void                         │
│  │       - set_pin(pin) → void                                      │
│  │       - deactivate() → void                                      │
│  │                                                                   │
│  ├── domain/value_objects/                                          │
│  │   - password.py: class PasswordHash(value: str)                  │
│  │   - pin_code.py: class PinCode(value: str)                       │
│  │   - role.py: class UserRole(Enum): ADMIN, CASHIER, WAREHOUSE    │
│  │                                                                   │
│  └── domain/repositories/i_user_repository.py                       │
│                                                                     │
│  Infrastructure Layer:                                              │
│  ├── infrastructure/persistence/repositories/user_repository.py     │
│  ├── infrastructure/persistence/models/user_model.py                │
│  └── infrastructure/auth/                                           │
│      - jwt_service.py  # JWT генерація/верифікація                  │
│      - password_hasher.py  # bcrypt хешування                       │
│                                                                     │
│  ─── Події: UserLoggedIn → (AuditModule)                           │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 2.6 Модуль: Catalog (Довідники)

```
┌─────────────────────────────────────────────────────────────────────┐
│  МОДУЛЬ: Catalog                                                     │
│  Відповідальність: Категорії, постачальники                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Presentation Layer:                                                │
│  ├── api/v1/categories.py                                           │
│  └── api/v1/suppliers.py                                            │
│                                                                     │
│  Application Layer:                                                 │
│  ├── application/use_cases/category_use_case.py                     │
│  └── application/use_cases/supplier_use_case.py                     │
│                                                                     │
│  Domain Layer:                                                      │
│  ├── domain/entities/category.py                                    │
│  │   - class Category:                                              │
│  │     Fields: id, name, parent_id, sort_order                      │
│  │     Methods:                                                     │
│  │       - add_child(child) → void                                  │
│  │       - get_path() → list[Category]                              │
│  │       - is_root() → bool                                         │
│  │                                                                   │
│  ├── domain/entities/supplier.py                                    │
│  │   - class Supplier:                                              │
│  │     Fields: id, name, edrpou, phone, email, address              │
│  │                                                                   │
│  ├── domain/repositories/i_category_repository.py                   │
│  └── domain/repositories/i_supplier_repository.py                   │
│                                                                     │
│  Infrastructure Layer:                                              │
│  ├── infrastructure/persistence/repositories/category_repository.py │
│  ├── infrastructure/persistence/repositories/supplier_repository.py │
│  ├── infrastructure/persistence/models/category_model.py            │
│  └── infrastructure/persistence/models/supplier_model.py            │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 3️⃣ МІЖМОДУЛЬНА ВЗАЄМОДІЯ (EVENT-DRIVEN)

```
                    ┌─────────────┐
                    │   Products  │
                    └──────┬──────┘
                           │ ProductCreated
                           │ StockChanged
                           ▼
┌─────────────┐   ┌───────────────┐   ┌─────────────┐
│   Catalog   │   │   Event Bus   │   │   Sales     │
│ (Categories)│   │  (RabbitMQ /  │   │  (Receipts) │
│ (Suppliers) │   │   In-Memory)  │   └──────┬──────┘
└─────────────┘   └───────┬───────┘          │
                          │                  │
              ┌───────────┴───────────┐      │
              ▼                       ▼      │
       ┌─────────────┐        ┌────────────┐ │
       │  Inventory  │        │  Finance   │ │
       │ (Documents) │        │  (Ledger)  │ │
       └─────────────┘        └────────────┘ │
              │                       │      │
              └───────────────────────┴──────┘
                                      │
                                      ▼
                               ┌─────────────┐
                               │   Reports   │
                               └─────────────┘

Потоки подій:
1. InvoiceConfirmed → StockService.apply_document() + LedgerService.create_entry()
2. ReceiptCreated → StockService.decrease_stock() + ReportsService.record_sale()
3. ProductCreated → SearchIndexService.index_product()
```

---

## 4️⃣ ФРОНТЕНД МОДУЛІ

```
frontend/src/
│
├── [Presentation] components/         # UI компоненти
│   ├── layout/                       # AppLayout, Sidebar, Header
│   └── ui/                           # Button, Input, Table, Modal, etc.
│
├── [Presentation] pages/             # Сторінки (1:1 з API модулями)
│   ├── auth/                         # LoginPage
│   ├── dashboard/                    # DashboardPage
│   ├── pos/                          # PosPage
│   ├── products/                     # ProductListPage, ProductFormPage
│   ├── categories/                   # CategoryListPage
│   ├── suppliers/                    # SupplierListPage, SupplierFormPage
│   ├── documents/                    # DocumentListPage, InvoiceFormPage, etc.
│   ├── ledger/                       # LedgerPage
│   └── reports/                      # ReportsPage
│
├── [Application] hooks/              # Бізнес-логіка React
│   ├── useAuth.ts                    # Логін, токен, ролі
│   ├── useProducts.ts                # CRUD товарів
│   ├── useCategories.ts              # CRUD категорій
│   ├── useSuppliers.ts               # CRUD постачальників
│   ├── useDocuments.ts               # CRUD документів
│   ├── useBarcodeSearch.ts           # Пошук за ШК (debounce)
│   └── usePagination.ts              # Пагінація
│
├── [Application] services/           # API клієнти
│   ├── api.ts                        # Axios instance + interceptors
│   ├── authService.ts                # Auth API
│   ├── productService.ts             # Products API
│   ├── categoryService.ts            # Categories API
│   ├── supplierService.ts            # Suppliers API
│   ├── documentService.ts            # Documents API
│   ├── receiptService.ts             # Receipts API
│   └── ledgerService.ts              # Ledger API
│
├── [Application] store/              # Глобальний стан
│   ├── authStore.ts                  # Zustand: auth state
│   └── uiStore.ts                    # Zustand: UI state
│
├── [Domain] types/                   # TypeScript типи (Domain)
│   ├── api.ts                        # PaginatedResponse, SearchParams
│   ├── auth.ts                       # User, LoginRequest, Tokens
│   ├── product.ts                    # Product, Category
│   ├── supplier.ts                   # Supplier
│   ├── document.ts                   # Invoice, Transfer, WriteOff, ReturnInvoice
│   ├── receipt.ts                    # Receipt, ReceiptItem
│   └── ledger.ts                     # LedgerEntry, Payment
│
└── [Shared] utils/                   # Утиліти
    ├── format.ts                     # Форматування (валюта, дата)
    └── validation.ts                 # Валідація (ШК, ЄДРПОУ)
```

---

## 5️⃣ МАТРИЦЯ ЗАЛЕЖНОСТЕЙ МОДУЛІВ

| Модуль | Залежить від | Не залежить від |
|--------|-------------|-----------------|
| **Products** | Catalog (Category, Supplier) | Sales, Finance, Auth |
| **Inventory** | Products, Catalog | Sales, Auth (частково) |
| **Sales** | Products, Auth | Finance, Catalog |
| **Finance** | Catalog (Supplier) | Sales, Products |
| **Auth** | — | Всі інші |
| **Catalog** | — | Sales, Finance |
| **Reports** | Всі модулі (read-only) | — |

---

> **Документ створено:** System Architect Agent (AEGIS v3)  
> **Останнє оновлення:** 2026-07-20
