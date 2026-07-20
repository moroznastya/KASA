# 🏗️ Архітектура Kasa POS — Шари (Layered Architecture)

> **Версія:** 2.0.0  
> **Стиль:** Clean Architecture / DDD (Domain-Driven Design)  
> **Принцип:** Dependency Rule — нижчі шари не знають про вищі  
> **Дата:** 2026-07-20  
> **Схвалено:** ADR-001, ADR-002, ADR-003

---

## 1️⃣ ОГЛЯД ШАРІВ (4+1 модель)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    PRESENTATION LAYER (API/UI)                      │
│  ┌─────────────────────┐  ┌──────────────────────────────────────┐  │
│  │   FastAPI Routers   │  │   React Components / Pages           │  │
│  │   (api/v1/*.py)     │  │   (frontend/src/pages/*.tsx)         │  │
│  └─────────┬───────────┘  └──────────────┬───────────────────────┘  │
│            │                              │                          │
│            │         HTTP / WebSocket     │                          │
│            ▼                              ▼                          │
├────────────┼──────────────────────────────┼──────────────────────────┤
│            │         APPLICATION LAYER    │                          │
│  ┌─────────┴──────────────────────────────┴──────────────────────┐  │
│  │                    Use Cases / Application Services            │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌────────────────────┐   │  │
│  │  │ ProductUseCase│  │InvoiceUseCase│  │  AuthUseCase       │   │  │
│  │  └──────┬───────┘  └──────┬───────┘  └───────┬────────────┘   │  │
│  │         │                 │                   │                │  │
│  │  ┌──────┴─────────────────┴───────────────────┴────────────┐  │  │
│  │  │           DTO / Mappers / Validators                     │  │  │
│  │  └──────────────────────────────────────────────────────────┘  │  │
│  └─────────────────────────────────────────────────────────────────┘  │
│                              │                                         │
│         Dependency Injection │ Direction                                │
│                              ▼                                         │
├────────────────────────────────────────────────────────────────────────┤
│                        DOMAIN LAYER                                    │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │  ┌─────────────┐  ┌─────────────────┐  ┌────────────────────┐   │  │
│  │  │  Entities    │  │  Value Objects  │  │  Domain Events     │   │  │
│  │  │  (Product,   │  │  (Money,        │  │  (StockChanged,    │   │  │
│  │  │   Invoice,   │  │   Barcode,      │  │   InvoiceConfirmed)│   │  │
│  │  │   User)      │  │   Quantity)     │  │                    │   │  │
│  │  └──────┬───────┘  └────────┬────────┘  └────────┬───────────┘   │  │
│  │         │                   │                     │               │  │
│  │  ┌──────┴───────────────────┴─────────────────────┴───────────┐  │  │
│  │  │              Repository Interfaces (Ports)                   │  │  │
│  │  │  IProductRepository │ IInvoiceRepository │ IUserRepository  │  │  │
│  │  └─────────────────────────────────────────────────────────────┘  │  │
│  │                                                                   │  │
│  │  ┌─────────────────────────────────────────────────────────────┐  │  │
│  │  │              Domain Services (Pure Business Logic)           │  │  │
│  │  │  PricingService │ StockService │ TaxCalculationService      │  │  │
│  │  └─────────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                              │                                         │
│         Repository Pattern   │ (Interfaces only, no implementations)   │
│                              ▼                                         │
├────────────────────────────────────────────────────────────────────────┤
│                     INFRASTRUCTURE LAYER                               │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │  ┌──────────────────┐  ┌──────────────┐  ┌──────────────────┐   │  │
│  │  │  Repository Impl │  │  ORM Models  │  │  External APIs   │   │  │
│  │  │  ProductRepo     │  │  (SQLAlchemy) │  │  (PRRO, NovaPoshta│  │  │
│  │  │  InvoiceRepo     │  │  ProductModel │  │   Email, SMS)    │   │  │
│  │  └────────┬─────────┘  └──────┬───────┘  └────────┬─────────┘   │  │
│  │           │                   │                     │            │  │
│  │  ┌────────┴───────────────────┴─────────────────────┴────────┐  │  │
│  │  │  Database (PostgreSQL via asyncpg) / Cache (Redis)        │  │  │
│  │  └───────────────────────────────────────────────────────────┘  │  │
│  │                                                                   │  │
│  │  ┌─────────────────────────────────────────────────────────────┐  │  │
│  │  │  DI Container (FastAPI Depends / custom container)          │  │  │
│  │  └─────────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2️⃣ ДЕТАЛЬНИЙ ОПИС ШАРІВ

### 2.1 Presentation Layer (Шар презентації)

**Призначення:** Взаємодія із зовнішнім світом (HTTP, WebSocket, CLI).

**Склад:**
```
backend/app/api/v1/           # FastAPI роутери
  ├── products.py             # → делегує ProductUseCase
  ├── invoices.py             # → делегує InvoiceUseCase
  ├── categories.py           # → делегує CategoryUseCase
  ├── suppliers.py            # → делегує SupplierUseCase
  ├── users.py                # → делегує AuthUseCase + UserUseCase
  ├── receipts.py             # → делегує ReceiptUseCase
  ├── transfers.py            # → делегує TransferUseCase
  ├── write_offs.py           # → делегує WriteOffUseCase
  ├── return_invoices.py      # → делегує ReturnInvoiceUseCase
  ├── ledger.py               # → делегує LedgerUseCase
  └── documents.py            # → делегує DocumentUseCase

frontend/src/pages/           # React сторінки
frontend/src/components/      # React компоненти
```

**Правила:**
- ✅ Може залежати тільки від Application Layer (Use Cases, DTO)
- ✅ Не має прямої залежності від Domain або Infrastructure
- ❌ Не містить бізнес-логіки
- ❌ Не імпортує SQLAlchemy моделі напряму

**Відповідальність:**
- HTTP роутинг та валідація запитів (через Pydantic)
- Форматування відповідей
- Аутентифікація (перевірка JWT токена)
- Swagger документація

---

### 2.2 Application Layer (Шар застосунку)

**Призначення:** Оркестрація бізнес-логіки, координація Use Cases.

**Склад:**
```
backend/app/application/
  ├── use_cases/
  │   ├── product_use_case.py       # CRUD + пошук товарів
  │   ├── invoice_use_case.py       # Створення/підтвердження накладних
  │   ├── auth_use_case.py          # Логін, реєстрація, JWT
  │   ├── stock_use_case.py         # Управління залишками
  │   ├── ledger_use_case.py        # Взаєморозрахунки
  │   ├── receipt_use_case.py       # Продажі (чеки)
  │   ├── transfer_use_case.py      # Переміщення
  │   ├── write_off_use_case.py     # Списання
  │   ├── return_invoice_use_case.py # Повернення
  │   └── document_use_case.py      # Узагальнені документи
  ├── dto/                          # Data Transfer Objects
  │   ├── product_dto.py
  │   ├── invoice_dto.py
  │   └── ...
  ├── mappers/                      # Маппери Entity ↔ DTO
  │   ├── product_mapper.py
  │   └── ...
  └── interfaces/                   # Інтерфейси (порти)
      ├── unit_of_work.py           # IUnitOfWork
      └── event_bus.py              # IEventBus
```

**Правила:**
- ✅ Залежить від Domain Layer (Entities, Repository Interfaces)
- ✅ Визначає Use Case інтерфейси
- ❌ Не залежить від Infrastructure
- ❌ Не знає про HTTP, БД, зовнішні сервіси

**Відповідальність:**
- Координація бізнес-операцій (транзакцій)
- Валідація бізнес-правил (через Domain)
- Виклик репозиторіїв (через інтерфейси)
- Публікація доменних подій
- Маппінг DTO ↔ Domain Entities

---

### 2.3 Domain Layer (Доменний шар)

**Призначення:** Чиста бізнес-логіка, незалежна від зовнішніх систем.

**Склад:**
```
backend/app/domain/
  ├── entities/
  │   ├── product.py                # Product entity
  │   ├── invoice.py                # Invoice aggregate root
  │   ├── invoice_item.py           # InvoiceItem entity
  │   ├── receipt.py                # Receipt aggregate root
  │   ├── user.py                   # User entity
  │   ├── supplier.py               # Supplier entity
  │   ├── category.py               # Category entity
  │   ├── transfer.py               # Transfer aggregate root
  │   ├── write_off.py              # WriteOff aggregate root
  │   └── return_invoice.py         # ReturnInvoice aggregate root
  ├── value_objects/
  │   ├── money.py                  # Гроші (валюта + сума)
  │   ├── barcode.py                # Штрих-код (з валідацією)
  │   ├── quantity.py               # Кількість (з одиницею виміру)
  │   ├── tax_rate.py               # Ставка ПДВ
  │   ├── ukr_tax_id.py             # УКТЗЕД код
  │   └── address.py                # Адреса
  ├── events/
  │   ├── stock_changed.py          # Зміна залишку
  │   ├── invoice_confirmed.py      # Накладну підтверджено
  │   ├── receipt_created.py        # Чек створено
  │   └── supplier_balance_changed.py # Зміна балансу
  ├── services/
  │   ├── pricing_service.py        # Розрахунок цін (ПДВ, націнка)
  │   ├── stock_service.py          # Логіка залишків (negative stock)
  │   ├── tax_service.py            # Податкові розрахунки
  │   └── document_numbering.py     # Генерація номерів документів
  └── repositories/                 # Інтерфейси (порти)
      ├── i_product_repository.py
      ├── i_invoice_repository.py
      ├── i_receipt_repository.py
      ├── i_user_repository.py
      ├── i_supplier_repository.py
      ├── i_category_repository.py
      ├── i_transfer_repository.py
      ├── i_write_off_repository.py
      ├── i_return_invoice_repository.py
      └── i_ledger_repository.py
```

**Правила:**
- ✅ Не має жодних залежностей від інших шарів
- ✅ Не імпортує SQLAlchemy, FastAPI, HTTP
- ✅ Всі інтерфейси репозиторіїв визначені тут
- ❌ Не містить реалізацій репозиторіїв
- ❌ Не містить DTO (тільки Domain Entities)

**Відповідальність:**
- Бізнес-правила та інваріанти
- Валідація стану entity
- Доменні події (Domain Events)
- Value Objects з вбудованою валідацією

---

### 2.4 Infrastructure Layer (Інфраструктурний шар)

**Призначення:** Реалізація технічних деталей (БД, зовнішні API, файлова система).

**Склад:**
```
backend/app/infrastructure/
  ├── persistence/
  │   ├── repositories/             # Реалізації репозиторіїв
  │   │   ├── product_repository.py  # SQLAlchemy реалізація
  │   │   ├── invoice_repository.py
  │   │   ├── receipt_repository.py
  │   │   ├── user_repository.py
  │   │   ├── supplier_repository.py
  │   │   ├── category_repository.py
  │   │   ├── transfer_repository.py
  │   │   ├── write_off_repository.py
  │   │   ├── return_invoice_repository.py
  │   │   └── ledger_repository.py
  │   ├── models/                   # SQLAlchemy ORM моделі
  │   │   ├── product_model.py
  │   │   ├── invoice_model.py
  │   │   └── ... (перейменовано з app/models/)
  │   ├── unit_of_work.py           # Реалізація UoW
  │   └── migrations/               # Alembic міграції
  ├── di/                           # Dependency Injection
  │   ├── container.py              # DI контейнер
  │   └── modules.py                # Модулі реєстрації
  ├── event_bus/                    # Шина подій
  │   ├── local_event_bus.py        # In-memory реалізація
  │   └── rabbitmq_event_bus.py     # RabbitMQ (для майбутнього)
  ├── external/                     # Зовнішні сервіси
  │   ├── prro/                     # ПРРО (фіскалізація)
  │   ├── nova_poshta/              # Нова Пошта API
  │   └── email/                    # Email сповіщення
  ├── cache/                        # Кешування
  │   └── redis_cache.py            # Redis (опціонально)
  └── config/                       # Конфігурація
      └── settings.py               # Pydantic Settings (переміщено з core/)
```

**Правила:**
- ✅ Реалізує інтерфейси з Domain Layer
- ✅ Залежить від Domain Layer (інтерфейси)
- ❌ Не залежить від Application Layer
- ❌ Не залежить від Presentation Layer

**Відповідальність:**
- Робота з БД (SQLAlchemy, asyncpg)
- Реалізація репозиторіїв
- Зовнішні API інтеграції
- Кешування
- Міграції БД
- DI контейнер

---

## 3️⃣ ДІАГРАМА ЗАЛЕЖНОСТЕЙ (Dependency Rule)

```
┌─────────────────────────────────────────────────────────────────────┐
│  PRESENTATION LAYER          ───→    APPLICATION LAYER              │
│  (api/v1/*.py)                       (use_cases/*.py)               │
│       │                                    │                        │
│       │                                    │                        │
│       ▼                                    ▼                        │
│  APPLICATION LAYER          ───→    DOMAIN LAYER                    │
│  (use_cases/*.py)                     (entities/, repositories/)    │
│       │                                    │                        │
│       │                                    │                        │
│       ▼                                    ▼                        │
│  INFRASTRUCTURE LAYER      ───→    DOMAIN LAYER                    │
│  (persistence/, external/)          (interfaces only)               │
└─────────────────────────────────────────────────────────────────────┘

НАПРЯМ ЗАЛЕЖНОСТЕЙ: Зовнішні шари → Внутрішні шари
НАПРЯМ ПОТОКУ ДАНИХ: Внутрішні шари → Зовнішні шари (через інтерфейси)
```

---

## 4️⃣ ПОРІВНЯННЯ: ПОТОЧНА vs ЦІЛЬОВА АРХІТЕКТУРА

| Аспект | Поточна (v1.x) | Цільова (v2.0) |
|--------|----------------|----------------|
| **Domain Entities** | Відсутні (SQLAlchemy моделі = Domain) | Окремі чисті entities в `domain/entities/` |
| **Repository Interfaces** | Відсутні (пряма робота з SQLAlchemy) | Визначені в `domain/repositories/` |
| **Repository Implementations** | Відсутні (логіка в services) | В `infrastructure/persistence/repositories/` |
| **Use Cases** | Відсутні (логіка в services) | В `application/use_cases/` |
| **Value Objects** | Відсутні (примітивні типи) | В `domain/value_objects/` (Money, Barcode, Quantity) |
| **Domain Events** | Відсутні | В `domain/events/` |
| **DI Container** | Відсутній (ручне створення) | В `infrastructure/di/container.py` |
| **Unit of Work** | Відсутній (ручне керування) | В `infrastructure/persistence/unit_of_work.py` |
| **DTO/Mappers** | Pydantic схеми в `schemas/` | DTO в `application/dto/`, маппери окремо |
| **Event Bus** | Відсутній | В `infrastructure/event_bus/` |
| **Тестування** | Ускладнене (залежність від БД) | Легке (інтерфейси + моки) |

---

## 5️⃣ МОДУЛЬНА КАРТА (Module Map)

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        KASA POS MODULE MAP                              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  МОДУЛЬ: PRODUCTS (Товари)                                       │   │
│  │  Шари: Presentation → Application → Domain → Infrastructure      │   │
│  │  Відповідальність: CRUD товарів, пошук, штрих-коди, залишки     │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  МОДУЛЬ: INVENTORY (Складський облік)                            │   │
│  │  Шари: Presentation → Application → Domain → Infrastructure      │   │
│  │  Підмодулі: Invoices, Transfers, WriteOffs, ReturnInvoices       │   │
│  │  Відповідальність: Документи, залишки, проведення               │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  МОДУЛЬ: SALES (Продажі)                                         │   │
│  │  Шари: Presentation → Application → Domain → Infrastructure      │   │
│  │  Відповідальність: POS, чеки, фіскалізація, повернення          │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  МОДУЛЬ: FINANCE (Фінанси)                                       │   │
│  │  Шари: Presentation → Application → Domain → Infrastructure      │   │
│  │  Відповідальність: Взаєморозрахунки, баланси, звіти             │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  МОДУЛЬ: AUTH (Авторизація)                                      │   │
│  │  Шари: Presentation → Application → Domain → Infrastructure      │   │
│  │  Відповідальність: Користувачі, ролі, JWT, PIN                  │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  МОДУЛЬ: CATALOG (Довідники)                                     │   │
│  │  Шари: Presentation → Application → Domain → Infrastructure      │   │
│  │  Підмодулі: Categories, Suppliers                                │   │
│  │  Відповідальність: Довідники, ієрархії                          │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │  МОДУЛЬ: REPORTS (Звіти)                                         │   │
│  │  Шари: Presentation → Application → Domain → Infrastructure      │   │
│  │  Відповідальність: Аналітика, дашборди, експорт                 │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 6️⃣ ПОТІК ДАНИХ (НА ПРИКЛАДІ СТВОРЕННЯ ТОВАРУ)

```
POST /api/v1/products
  │
  ▼
[Presentation] products.py (роутер)
  │  Валідація: ProductCreate (Pydantic)
  │  Аутентифікація: get_current_user
  │
  ▼
[Application] ProductUseCase.create_product(dto: ProductCreateDTO)
  │  Маппінг: DTO → Domain Entity
  │  Валідація бізнес-правил:
  │    - Унікальність barcode
  │    - Унікальність sku
  │    - Валідація ціни (≥ 0)
  │  Виклик: IProductRepository.save(product)
  │  Публікація: ProductCreated event
  │
  ▼
[Domain] IProductRepository (інтерфейс)
  │  Визначення: save(entity: Product) → Product
  │
  ▼
[Infrastructure] ProductRepository (SQLAlchemy)
  │  Маппінг: Product → ProductModel (ORM)
  │  Виконання: session.add(product_model)
  │  Коміт: unit_of_work.commit()
  │
  ▼
[Infrastructure] PostgreSQL (asyncpg)
  │  INSERT INTO products ...
  │
  ▼
[Presentation] ProductResponse (JSON)
```

---

## 7️⃣ КЛЮЧОВІ АРХІТЕКТУРНІ РІШЕННЯ

| Рішення | Обґрунтування | ADR |
|---------|---------------|-----|
| 4-шаровий поділ | Чітке розділення відповідальності | ADR-001 |
| Repository Pattern | Абстракція доступу до даних | ADR-002 |
| Domain Events | Слабка зв'язність між модулями | ADR-003 |
| DI Container | Централізоване управління залежностями | ADR-004 |
| Value Objects | Безпека типів, вбудована валідація | ADR-005 |
| CQRS (опціонально) | Розділення читання/запису для звітів | ADR-006 |

---

> **Документ створено:** System Architect Agent (AEGIS v3)  
> **Останнє оновлення:** 2026-07-20
