# 📐 ДЕТАЛІЗОВАНА СТРУКТУРА ПРОЄКТУ — Kasa POS

> **Версія:** 2.0.0
> **Архітектура:** Clean Architecture / DDD
> **Стек (продакшн):** Rust (axum-фасад) + React + PostgreSQL + Tauri — 100% Rust backend
> **Legacy:** Python/FastAPI — дезактивований, еталон для differential-тестів

---

## 1️⃣ ЗАГАЛЬНА СТРУКТУРА ПРОЄКТУ

```
kasa/
├── backend/                          # 🧪 LEGACY — Python-бекенд (FastAPI)
│   │                                 # ДЕЗАКТИВОВАНИЙ: не runtime, лише еталон
│   │                                 # для differential-тестів та історія міграцій
│   ├── alembic/                      # Історичні міграції БД
│   ├── app/                          # Код FastAPI (api, core, models, schemas, services)
│   ├── migrations/                   # Альтернативні міграції
│   ├── .env                          # Змінні оточення
│   ├── alembic.ini                   # Конфіг Alembic
│   └── requirements.txt              # Залежності Python
│
├── frontend/                         # 🎨 Клієнтська частина (React + Vite)
│   ├── public/                       # Статичні файли
│   ├── src/                          # Вихідний код React
│   │   ├── components/               # UI компоненти
│   │   │   ├── layout/               # Компоненти макету
│   │   │   └── ui/                   # Базові UI компоненти
│   │   ├── hooks/                    # React хуки
│   │   ├── pages/                    # Сторінки застосунку
│   │   ├── services/                 # API сервіси (axios → :8000)
│   │   ├── store/                    # Глобальний стан (Zustand)
│   │   ├── types/                    # TypeScript типи
│   │   └── utils/                    # Утиліти
│   ├── src-tauri/                    # 🦀 Tauri оболонка + Rust-фасад (100% Rust)
│   │   ├── crates/                   # Робочі крейти Rust
│   │   │   ├── kasa-api/             # HTTP-фасад (axum), роути v1, 410-fallback
│   │   │   ├── kasa-application/     # Застосунковий шар (use cases)
│   │   │   ├── kasa-domain/          # Доменні сутності та правила
│   │   │   ├── kasa-infrastructure/  # PostgreSQL, репозиторії, міграції
│   │   │   ├── kasa-ocr/             # OCR (розпізнавання документів)
│   │   │   └── kasa-prro/            # ПРРО/фіскалізація
│   │   ├── src/                      # main.rs / lib.rs (Tauri + axum)
│   │   ├── migrations/               # Міграції БД (Rust)
│   │   ├── Cargo.toml                # Rust workspace
│   │   ├── tauri.conf.json           # Конфігурація Tauri
│   │   └── target/debug/kasa-pos     # Зібраний бінарник (слухає 127.0.0.1:8000)
│   ├── index.html                    # Вхідна HTML точка
│   ├── vite.config.ts                # Конфіг Vite
│   ├── tsconfig.json                 # Конфіг TypeScript
│   ├── tailwind.config.ts            # Конфіг TailwindCSS
│   └── package.json                  # Залежності Node.js
│
├── docs/                             # 📄 Документація
├── docker-compose.yml                # PostgreSQL + backend (backend — профіль legacy)
├── ROADMAP.md                        # Дорожня карта розробки
└── STRUCTURE.md                      # Цей файл
```

---

## 2️⃣ RUST-ФАСАД — ПРОДАКШН BACKEND (frontend/src-tauri/crates)

**Роль:** єдиний backend системи. Вбудований у Tauri-бінарник, слухає `127.0.0.1:8000`.

### 2.1 🧱 Крейти

```
frontend/src-tauri/crates/
│
├── kasa-api/                          # 🎯 ШАР ПРЕЗЕНТАЦІЇ (HTTP-фасад, axum)
│   ├── src/
│   │   ├── lib.rs                     # Збірка роутів, fallback → 410 Gone
│   │   ├── auth.rs                    # Авторизація (login, login-pin, refresh)
│   │   ├── products.rs                # Товари (CRUD + пошук за ШК)
│   │   ├── categories.rs              # Категорії (CRUD + дерево)
│   │   ├── suppliers.rs               # Постачальники (CRUD)
│   │   ├── invoices.rs                # Прибуткові накладні
│   │   ├── transfers.rs               # Переміщення між складами
│   │   ├── write_offs.rs              # Списання
│   │   ├── return_invoices.rs         # Повернення постачальнику
│   │   ├── receipts.rs                # Чеки продажу
│   │   ├── ledger.rs                  # Взаєморозрахунки
│   │   ├── documents.rs               # Узагальнений перегляд документів
│   │   ├── users.rs                   # Користувачі + ролі
│   │   ├── purchase_orders.rs         # Замовлення постачальнику
│   │   └── ocr.rs / prro.rs           # OCR та ПРРО-інтеграції
│   └── tests/                         # Rust-тести фасаду
│
├── kasa-application/                  # 🧠 ЗАСТОСУНКОВИЙ ШАР
│   └── src/                           # Use cases, DTO, mappers, інтерфейси
│
├── kasa-domain/                       # 📦 ДОМЕННИЙ ШАР
│   └── src/                           # Entities, value objects, репозиторії (traits)
│
├── kasa-infrastructure/               # 💾 ШАР ІНФРАСТРУКТУРИ
│   ├── src/                           # PostgreSQL (sqlx/diesel), репозиторії, UoW
│   ├── migrations/                    # Міграції БД
│   └── tests/                         # Інтеграційні тести
│
├── kasa-ocr/                          # 🔍 OCR
│   └── src/                           # Розпізнавання документів/зображень
│
└── kasa-prro/                         # 🧾 ПРРО/ФІСКАЛІЗАЦІЯ
    ├── src/                           # Інтеграція з ПРРО
    ├── ffi/                           # FFI-обгортки
    └── tests/
```

### 2.2 🌐 Покриття роутів

| Показник | Значення |
|----------|----------|
| Покрито роутів | **157 / 164** |
| Деприкейтнуті v2-аліаси auth | **7 → 410 Gone** |
| Fallback для legacy-шляхів | **410 Gone** (Python-бекенд не runtime) |

---

## 3️⃣ BACKEND (PYTHON/FastAPI) — LEGACY

> **Статус:** ❌ ДЕЗАКТИВОВАНИЙ. Профіль `legacy` у docker-compose, Python-процеси
> не запускаються. Код збережено як **еталон для differential-тестів** та історичну довідку.

### 3.1 🏗️ Шари архітектури (історична довідка)

```
backend/app/
│
├── api/                              # 🎯 ШАР ПРЕЗЕНТАЦІЇ (API)
│   ├── __init__.py
│   └── v1/                           # Версія API v1
│       ├── __init__.py               # Агрегація всіх роутерів
│       ├── products.py               # Товари (CRUD + пошук за ШК)
│       ├── categories.py             # Категорії (CRUD + дерево)
│       ├── suppliers.py              # Постачальники (CRUD)
│       ├── users.py                  # Користувачі + Авторизація
│       ├── invoices.py               # Прибуткові накладні
│       ├── transfers.py              # Переміщення між складами
│       ├── write_offs.py             # Списання
│       ├── return_invoices.py        # Повернення постачальнику
│       ├── receipts.py               # Чеки продажу
│       ├── ledger.py                 # Взаєморозрахунки
│       └── documents.py              # Узагальнений перегляд документів
│
├── middleware/                        # 🛡️ ПРОМІЖНИЙ ШАР
│   └── auth_middleware.py            # JWT авторизація
│
├── services/                          # 🧠 ШАР БІЗНЕС-ЛОГІКИ
│   ├── auth_service.py               # Авторизація (JWT, bcrypt)
│   ├── product_service.py            # Товари (логіка пошуку, фільтрації)
│   ├── document_service.py           # Документи (створення, проведення)
│   └── ledger_service.py             # Взаєморозрахунки
│
├── models/                            # 💾 ШАР ДАНИХ (SQLAlchemy)
│   ├── product.py                    # Product (товар)
│   ├── barcode.py                    # Barcode (штрих-коди)
│   ├── category.py                   # Category (категорія)
│   ├── product_image.py              # ProductImage (зображення)
│   ├── supplier.py                   # Supplier (постачальник)
│   ├── user.py                       # User (користувач)
│   ├── invoice.py                    # Invoice (прибуткова накладна)
│   ├── transfer.py                   # Transfer (переміщення)
│   ├── write_off.py                  # WriteOff (списання)
│   ├── return_invoice.py             # ReturnInvoice (повернення)
│   ├── receipt.py                    # Receipt (чек продажу)
│   └── supplier_ledger.py            # SupplierLedger (взаєморозрахунки)
│
├── schemas/                           # 📦 ШАР DTO (Pydantic)
│   ├── product.py                    # ProductCreate/Update/Response
│   ├── category.py                   # CategoryCreate/Update/Response
│   ├── supplier.py                   # SupplierCreate/Update/Response
│   ├── user.py                       # UserCreate/Update/Response
│   ├── invoice.py                    # InvoiceCreate/Update/Response
│   ├── transfer.py                   # TransferCreate/Update/Response
│   ├── write_off.py                  # WriteOffCreate/Update/Response
│   ├── return_invoice.py             # ReturnInvoiceCreate/Update/Response
│   ├── receipt.py                    # ReceiptCreate/Response
│   └── ledger.py                     # LedgerEntry/Response
│
├── core/                              # ⚙️ ЯДРО
│   ├── config.py                     # Налаштування (pydantic-settings)
│   ├── security.py                   # JWT, хешування паролів
│   └── exceptions.py                 # Кастомні винятки
│
├── main.py                           # 🚀 Точка входу FastAPI (історична)
├── database.py                       # 🔗 Підключення до БД (async)
└── config.py                         # ⚙️ Конфігурація застосунку
```

### 3.2 📊 Моделі БД (SQLAlchemy — еталон схеми)

| Модель | Таблиця | Призначення |
|--------|---------|-------------|
| `Product` | `products` | Товар (назва, ціна, ПДВ, одиниця виміру) |
| `Barcode` | `barcodes` | Додаткові штрих-коди товару |
| `Category` | `categories` | Категорія (ієрархічне дерево) |
| `ProductImage` | `product_images` | Зображення товару |
| `Supplier` | `suppliers` | Постачальник (контакти, баланс) |
| `User` | `users` | Користувач (логін, PIN, роль) |
| `Invoice` | `invoices` | Прибуткова накладна |
| `InvoiceItem` | `invoice_items` | Позиція прибуткової накладної |
| `Transfer` | `transfers` | Переміщення між складами |
| `TransferItem` | `transfer_items` | Позиція переміщення |
| `WriteOff` | `write_offs` | Списання товару |
| `WriteOffItem` | `write_off_items` | Позиція списання |
| `ReturnInvoice` | `return_invoices` | Повернення постачальнику |
| `ReturnInvoiceItem` | `return_invoice_items` | Позиція повернення |
| `Receipt` | `receipts` | Чек продажу |
| `ReceiptItem` | `receipt_items` | Позиція чеку |
| `SupplierLedger` | `supplier_ledger` | Журнал взаєморозрахунків |

> Схема БД ідентична для Rust-фасаду: `kasa-infrastructure` реалізує ті самі
> таблиці та міграції (PostgreSQL). Python-моделі — еталон для differential-тестів.

### 3.3 🔌 API Ендпоінти (v1 — реалізовані в Rust-фасаді)

| Метод | Шлях | Опис |
|-------|------|------|
| **Авторизація** | | |
| POST | `/api/v1/auth/login` | Вхід за логіном/паролем |
| POST | `/api/v1/auth/login-pin` | Вхід за PIN-кодом |
| POST | `/api/v1/auth/refresh` | Оновлення токена |
| **Товари** | | |
| GET | `/api/v1/products` | Список товарів (пошук, фільтри, пагінація) |
| GET | `/api/v1/products/{id}` | Товар за ID |
| GET | `/api/v1/products/barcode/{barcode}` | Товар за штрих-кодом |
| POST | `/api/v1/products` | Створити товар |
| PUT | `/api/v1/products/{id}` | Оновити товар |
| DELETE | `/api/v1/products/{id}` | Видалити товар |
| **Категорії** | | |
| GET | `/api/v1/categories` | Список категорій (дерево) |
| GET | `/api/v1/categories/{id}` | Категорія за ID |
| POST | `/api/v1/categories` | Створити категорію |
| PUT | `/api/v1/categories/{id}` | Оновити категорію |
| DELETE | `/api/v1/categories/{id}` | Видалити категорію |
| **Постачальники** | | |
| GET | `/api/v1/suppliers` | Список постачальників |
| GET | `/api/v1/suppliers/{id}` | Постачальник за ID |
| POST | `/api/v1/suppliers` | Створити постачальника |
| PUT | `/api/v1/suppliers/{id}` | Оновити постачальника |
| DELETE | `/api/v1/suppliers/{id}` | Видалити постачальника |
| **Прибуткові накладні** | | |
| GET | `/api/v1/invoices` | Список накладних |
| GET | `/api/v1/invoices/{id}` | Накладна за ID |
| POST | `/api/v1/invoices` | Створити накладну |
| PUT | `/api/v1/invoices/{id}` | Оновити накладну |
| DELETE | `/api/v1/invoices/{id}` | Видалити накладну |
| POST | `/api/v1/invoices/{id}/confirm` | Підтвердити накладну |
| **Переміщення** | | |
| GET | `/api/v1/transfers` | Список переміщень |
| GET | `/api/v1/transfers/{id}` | Переміщення за ID |
| POST | `/api/v1/transfers` | Створити переміщення |
| PUT | `/api/v1/transfers/{id}` | Оновити переміщення |
| DELETE | `/api/v1/transfers/{id}` | Видалити переміщення |
| POST | `/api/v1/transfers/{id}/confirm` | Підтвердити переміщення |
| **Списання** | | |
| GET | `/api/v1/write-offs` | Список списань |
| GET | `/api/v1/write-offs/{id}` | Списання за ID |
| POST | `/api/v1/write-offs` | Створити списання |
| PUT | `/api/v1/write-offs/{id}` | Оновити списання |
| DELETE | `/api/v1/write-offs/{id}` | Видалити списання |
| **Повернення** | | |
| GET | `/api/v1/return-invoices` | Список повернень |
| GET | `/api/v1/return-invoices/{id}` | Повернення за ID |
| POST | `/api/v1/return-invoices` | Створити повернення |
| PUT | `/api/v1/return-invoices/{id}` | Оновити повернення |
| DELETE | `/api/v1/return-invoices/{id}` | Видалити повернення |
| POST | `/api/v1/return-invoices/{id}/confirm` | Підтвердити повернення |
| **Чеки** | | |
| GET | `/api/v1/receipts` | Список чеків |
| GET | `/api/v1/receipts/{id}` | Чек за ID |
| POST | `/api/v1/receipts` | Створити чек |
| **Взаєморозрахунки** | | |
| GET | `/api/v1/ledger` | Журнал взаєморозрахунків |
| GET | `/api/v1/ledger/suppliers/{id}` | Баланс постачальника |
| POST | `/api/v1/ledger/payment` | Зареєструвати платіж |
| **Документи (узагальнені)** | | |
| GET | `/api/v1/documents` | Список всіх документів |
| **Користувачі** | | |
| GET | `/api/v1/users` | Список користувачів |
| GET | `/api/v1/users/{id}` | Користувач за ID |
| POST | `/api/v1/users` | Створити користувача |
| PUT | `/api/v1/users/{id}` | Оновити користувача |
| DELETE | `/api/v1/users/{id}` | Видалити користувача |
| **Системні** | | |
| GET | `/health` | Перевірка стану сервера |
| GET | `/` | Кореневий ендпоінт |

> Деприкейтнуті v2-аліаси auth (7 шт.) повертають **410 Gone**.
> Невідомі/legacy-шляхи — fallback **410 Gone**.

---

## 4️⃣ FRONTEND — ДЕТАЛІЗОВАНА СТРУКТУРА

### 4.1 🧩 Компонентна архітектура

```
frontend/src/
│
├── components/                        # 🧱 ПЕРЕВИКОРИСТОВУВАНІ КОМПОНЕНТИ
│   ├── layout/                        # 📐 Компоненти макету
│   │   ├── AppLayout.tsx              # Головний макет (Sidebar + Header + Content)
│   │   ├── Sidebar.tsx                # Бокова панель навігації
│   │   ├── Header.tsx                 # Верхня панель (користувач, годинник)
│   │   └── ProtectedRoute.tsx         # Захищений маршрут (перевірка авторизації)
│   │
│   └── ui/                            # 🎨 Базові UI компоненти
│       ├── Button.tsx                 # Кнопка (варіанти: primary, secondary, danger)
│       ├── Input.tsx                  # Поле вводу (з іконкою, лейблом, помилкою)
│       ├── Select.tsx                 # Випадаючий список
│       ├── Modal.tsx                  # Модальне вікно
│       ├── Table.tsx                  # Таблиця (з пагінацією, сортуванням)
│       ├── Spinner.tsx                # Індикатор завантаження
│       ├── Badge.tsx                  # Бейдж (статус, тип)
│       ├── Card.tsx                   # Картка
│       ├── SearchInput.tsx            # Поле пошуку
│       └── Pagination.tsx             # Пагінація
│
├── hooks/                             # 🪝 React ХУКИ
│   ├── useAuth.ts                     # Авторизація (логін, логаут, токен)
│   ├── useProducts.ts                 # Товари (CRUD, пошук)
│   ├── useCategories.ts               # Категорії (CRUD, дерево)
│   ├── useSuppliers.ts                # Постачальники (CRUD)
│   ├── useDocuments.ts                # Документи (CRUD, фільтрація)
│   ├── useBarcodeSearch.ts            # Пошук за штрих-кодом (debounce)
│   └── usePagination.ts               # Пагінація (сторінки, ліміти)
│
├── pages/                             # 📄 СТОРІНКИ
│   ├── auth/                          # 🔐 Авторизація
│   │   └── LoginPage.tsx              # Сторінка входу (логін/PIN)
│   │
│   ├── dashboard/                     # 📊 Дашборд
│   │   └── DashboardPage.tsx          # Головна панель (статистика, графіки)
│   │
│   ├── pos/                           # 🛒 POS-каса
│   │   └── PosPage.tsx                # Екран продажу (товари, кошик, оплата)
│   │
│   ├── products/                      # 📦 Товари
│   │   ├── ProductListPage.tsx        # Список товарів (таблиця, фільтри)
│   │   └── ProductFormPage.tsx        # Форма товару (створення/редагування)
│   │
│   ├── categories/                    # 🏷️ Категорії
│   │   └── CategoryListPage.tsx       # Список категорій (дерево)
│   │
│   ├── suppliers/                     # 🤝 Постачальники
│   │   ├── SupplierListPage.tsx       # Список постачальників
│   │   └── SupplierFormPage.tsx       # Форма постачальника
│   │
│   ├── documents/                     # 📄 Документи
│   │   ├── DocumentListPage.tsx       # Список всіх документів
│   │   ├── InvoiceFormPage.tsx        # Форма прибуткової накладної
│   │   ├── TransferFormPage.tsx       # Форма переміщення
│   │   ├── WriteOffFormPage.tsx       # Форма списання
│   │   └── ReturnInvoiceFormPage.tsx  # Форма повернення
│   │
│   ├── ledger/                        # 💰 Взаєморозрахунки
│   │   └── LedgerPage.tsx             # Журнал взаєморозрахунків
│   │
│   └── reports/                       # 📈 Звіти
│       └── ReportsPage.tsx            # Сторінка звітів
│
├── services/                          # 🌐 API СЕРВІСИ (axios → 127.0.0.1:8000)
│   ├── api.ts                         # Базовий клієнт (інтерцептори, токен)
│   ├── authService.ts                 # Авторизація (логін, PIN, refresh)
│   ├── productService.ts              # Товари (CRUD, пошук за ШК)
│   ├── categoryService.ts             # Категорії (CRUD)
│   ├── supplierService.ts             # Постачальники (CRUD)
│   ├── documentService.ts             # Документи (CRUD, підтвердження)
│   ├── receiptService.ts              # Чеки (створення, історія)
│   └── ledgerService.ts               # Взаєморозрахунки (баланс, платежі)
│
├── store/                             # 🗄️ ГЛОБАЛЬНИЙ СТАН (Zustand)
│   ├── authStore.ts                   # Стан авторизації (токен, користувач)
│   └── uiStore.ts                     # Стан UI (тема, бічне меню)
│
├── types/                             # 📝 TypeScript ТИПИ
│   ├── api.ts                         # API типи (PaginatedResponse, SearchParams)
│   ├── auth.ts                        # Auth типи (User, LoginRequest, Tokens)
│   ├── product.ts                     # Product, Category, BarcodeSearchResult
│   ├── supplier.ts                    # Supplier
│   ├── document.ts                    # Invoice, Transfer, WriteOff, ReturnInvoice
│   ├── receipt.ts                     # Receipt, ReceiptItem, PaymentMethod
│   └── ledger.ts                      # LedgerEntry, Payment
│
├── utils/                             # 🔧 УТИЛІТИ
│   ├── format.ts                      # Форматування (валюта, дата, одиниці)
│   └── validation.ts                  # Валідація (штрих-код, ЄДРПОУ, телефон)
│
├── main.tsx                           # 🚀 Точка входу React
├── App.tsx                            # 📱 Головний компонент (роутинг)
├── index.css                          # 🎨 Глобальні стилі (TailwindCSS)
└── vite-env.d.ts                      # 📌 Типи Vite
```

### 4.2 🗺️ Маршрутизація (React Router)

| Шлях | Сторінка | Доступ |
|------|----------|--------|
| `/login` | LoginPage | Публічний |
| `/` | DashboardPage | Захищений |
| `/pos` | PosPage | Захищений |
| `/products` | ProductListPage | Захищений |
| `/products/new` | ProductFormPage | Захищений |
| `/products/:id/edit` | ProductFormPage | Захищений |
| `/categories` | CategoryListPage | Захищений |
| `/suppliers` | SupplierListPage | Захищений |
| `/suppliers/new` | SupplierFormPage | Захищений |
| `/suppliers/:id/edit` | SupplierFormPage | Захищений |
| `/documents` | DocumentListPage | Захищений |
| `/documents/invoices/new` | InvoiceFormPage | Захищений |
| `/documents/transfers/new` | TransferFormPage | Захищений |
| `/documents/write-offs/new` | WriteOffFormPage | Захищений |
| `/documents/returns/new` | ReturnInvoiceFormPage | Захищений |
| `/ledger` | LedgerPage | Захищений |
| `/reports` | ReportsPage | Захищений |

### 4.3 🎨 UI Компоненти (TailwindCSS v4)

| Компонент | Пропси | Призначення |
|-----------|--------|-------------|
| `Button` | variant, size, isLoading, disabled | Кнопки дій |
| `Input` | label, error, icon, type | Поля вводу |
| `Select` | label, options, value | Випадаючі списки |
| `Modal` | isOpen, title, size, onClose | Модальні вікна |
| `Table` | columns, data, isLoading | Таблиці даних |
| `Spinner` | size | Індикатори завантаження |
| `Badge` | variant | Статуси (активний/архівний) |
| `Card` | children | Контейнери |
| `SearchInput` | value, onChange, placeholder | Пошук |
| `Pagination` | page, total, size, onChange | Пагінація |

---

## 5️⃣ Tauri DESKTOP ОБГОРТКА + RUST-ФАСАД

```
frontend/src-tauri/
├── src/
│   ├── main.rs                        # Точка входу Tauri (бінарник kasa-pos)
│   └── lib.rs                         # kasa_pos_lib (інтеграція Tauri + axum)
├── crates/                            # Робочі крейти (kasa-api, kasa-domain, ...)
├── migrations/                        # Міграції БД
├── Cargo.toml                         # Rust workspace (бінарник: kasa-pos)
├── Cargo.lock
├── tauri.conf.json                    # Конфігурація Tauri
├── build.rs
├── capabilities/                      # Дозволи Tauri
└── icons/                             # Іконки застосовунку
```

> Бінарник `kasa-pos` (target/debug або target/release) запускає axum-фасад
> на `127.0.0.1:8000` — єдиний backend для React-фронтенду.

---

## 6️⃣ БАЗА ДАНИХ (PostgreSQL)

### 6.1 Схема даних

```sql
-- Ключові таблиці (ідентичні для Rust-фасаду та legacy-еталона):
products          -- Товари (Super-Product Model)
barcodes          -- Додаткові штрих-коди
categories        -- Категорії (ієрархічне дерево)
product_images    -- Зображення товарів
suppliers         -- Постачальники
users             -- Користувачі
invoices          -- Прибуткові накладні
invoice_items     -- Позиції накладних
transfers         -- Переміщення
transfer_items    -- Позиції переміщень
write_offs        -- Списання
write_off_items   -- Позиції списань
return_invoices   -- Повернення постачальнику
return_invoice_items -- Позиції повернень
receipts          -- Чеки продажу
receipt_items     -- Позиції чеків
supplier_ledger   -- Журнал взаєморозрахунків
```

### 6.2 Materialized Views (для звітів)

```sql
sales_report_view      -- Продажі за період (групування)
stock_report_view      -- Залишки товарів
supplier_ledger_view   -- Взаєморозрахунки з постачальниками
```

---

## 7️⃣ 🔄 ПОТОКИ ДАНИХ (Data Flow)

### 7.1 Створення прибуткової накладної
```
Frontend (InvoiceFormPage)
  → POST /api/v1/invoices (Rust-фасад :8000)
    → kasa-api → kasa-application (use case)
      → kasa-infrastructure: Invoice + InvoiceItems
      → Оновлення залишків (Stock)
      → Оновлення SupplierLedger
  ← Відповідь: InvoiceResponse
```

### 7.2 Продаж товару (POS)
```
Frontend (PosPage)
  → Сканування ШК → GET /api/v1/products/barcode/{barcode} (Rust-фасад :8000)
    → kasa-api → kasa-application → kasa-infrastructure
  ← ProductResponse
  → Додавання в кошик (локальний стан)
  → POST /api/v1/receipts
    → Створення Receipt + ReceiptItems
    → Зменшення залишків
  ← ReceiptResponse
```

### 7.3 Авторизація
```
Frontend (LoginPage)
  → POST /api/v1/auth/login-pin (Rust-фасад :8000)
    → Пошук користувача за PIN
    → Генерація JWT токена
  ← { access_token, user }
  → Збереження токена в authStore (Zustand)
  → Додавання токена до заголовків axios (interceptor)
```

### 7.4 Fallback legacy-шляхів
```
Будь-який запит до деприкейтнутого/невідомого шляху
  → Rust-фасад: fallback → HTTP 410 Gone
  (Python-бекенд не запускається, docker-compose backend — профіль legacy)
```

---

## 8️⃣ 🧪 ТЕСТУВАННЯ (QA)

| Рівень | Інструмент | Що тестуємо |
|--------|-----------|-------------|
| Rust unit | cargo test | Крейти kasa-api, kasa-domain, kasa-infrastructure, kasa-ocr, kasa-prro |
| Rust integration | cargo test (tests/) | API ендпоінти, БД |
| Differential | pytest (backend/) | Звірка Rust-фасаду з legacy-еталоном (Python — референс) |
| Frontend | npm test | Компоненти, хуки |
| E2E | Playwright | Користувацькі сценарії |

---

## 9️⃣ 📦 ЗАЛЕЖНОСТІ

### Rust (продакшн-стек)
```
axum               — HTTP-фасад (127.0.0.1:8000)
tokio              — async runtime
sqlx / diesel      — PostgreSQL
serde              — серіалізація
tauri              — desktop обгортка
kasa-* (crates)    — api, application, domain, infrastructure, ocr, prro
```

### Frontend (Node.js)
```
react              — UI бібліотека
react-router-dom   — Маршрутизація
axios              — HTTP клієнт
zustand            — Управління станом
lucide-react       — Іконки
react-hot-toast    — Сповіщення
tailwindcss        — CSS фреймворк
typescript         — Типізація
vite               — Збірка
@tauri-apps/api    — Tauri API
```

### Backend (Python) — LEGACY, лише для differential-тестів
```
fastapi            — Веб-фреймворк (історичний)
sqlalchemy[asyncio] — ORM (async)
alembic            — Міграції БД (історичні)
asyncpg            — Драйвер PostgreSQL
pydantic           — Валідація даних
uvicorn            — ASGI сервер (НЕ запускається)
```

---

## 🔟 📋 СТАТУС РОЗРОБКИ

| Модуль | Статус | Примітки |
|--------|--------|----------|
| Rust: Фасад kasa-api | ✅ Готово | 157/164 роути, 7 v2-аліасів → 410 |
| Rust: Домен kasa-domain | ✅ Готово | Сутності, правила |
| Rust: Application | ✅ Готово | Use cases |
| Rust: Infrastructure | ✅ Готово | PostgreSQL, міграції |
| Rust: OCR kasa-ocr | ✅ Готово | Розпізнавання документів |
| Rust: ПРРО kasa-prro | ✅ Готово | Фіскалізація |
| Python-бекенд (legacy) | ❌ Дезактивовано | Еталон differential-тестів, профіль legacy |
| Frontend: Layout | ✅ Готово | Sidebar, Header |
| Frontend: Login | ✅ Готово | PIN-авторизація |
| Frontend: POS | ✅ Готово | Кошик, оплата |
| Frontend: Товари | ✅ Готово | Список, форма |
| Frontend: Категорії | ✅ Готово | Список |
| Frontend: Постачальники | ✅ Готово | Список, форма |
| Frontend: Документи | ✅ Готово | Всі форми |
| Frontend: Взаєморозрахунки | ✅ Готово | Ledger |
| Frontend: Звіти | ⏳ В розробці | ReportsPage |
| Tauri Desktop | ✅ Готово | Бінарник kasa-pos :8000 |
| Differential-тести | ⏳ В процесі | Rust vs legacy-еталон |

---

> **Документ оновлено:** Dev_Agent після етапу 8 міграції Python → Rust  
> **Архітектура:** 100% Rust backend (axum :8000) + React + Tauri + PostgreSQL
