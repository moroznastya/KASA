# 📐 ДЕТАЛІЗОВАНА СТРУКТУРА ПРОЄКТУ — Kasa POS

> **Версія:** 1.0.0  
> **Архітектура:** Clean Architecture / DDD  
> **Стек:** FastAPI + React + PostgreSQL + Tauri

---

## 1️⃣ ЗАГАЛЬНА СТРУКТУРА ПРОЄКТУ

```
kasa/
├── backend/                          # 🖥️ Серверна частина (FastAPI)
│   ├── alembic/                      # Міграції БД
│   ├── app/                          # Основний код застосунку
│   │   ├── api/                      # API шар (роутери)
│   │   ├── core/                     # Ядро (конфіг, безпека)
│   │   ├── middleware/               # Middleware (авторизація)
│   │   ├── models/                   # Моделі БД (SQLAlchemy)
│   │   ├── schemas/                  # Pydantic схеми (DTO)
│   │   └── services/                 # Бізнес-логіка (сервіси)
│   ├── migrations/                   # Альтернативні міграції
│   ├── .env                          # Змінні оточення
│   ├── alembic.ini                   # Конфіг Alembic
│   └── requirements.txt              # Залежності Python
│
├── frontend/                         # 🎨 Клієнтська частина (React + Vite)
│   ├── public/                       # Статичні файли
│   ├── src/                          # Вихідний код
│   │   ├── components/               # UI компоненти
│   │   │   ├── layout/               # Компоненти макету
│   │   │   └── ui/                   # Базові UI компоненти
│   │   ├── hooks/                    # React хуки
│   │   ├── pages/                    # Сторінки застосунку
│   │   ├── services/                 # API сервіси (axios)
│   │   ├── store/                    # Глобальний стан (Zustand)
│   │   ├── types/                    # TypeScript типи
│   │   └── utils/                    # Утиліти
│   ├── src-tauri/                    # Tauri desktop обгортка
│   ├── index.html                    # Вхідна HTML точка
│   ├── vite.config.ts                # Конфіг Vite
│   ├── tsconfig.json                 # Конфіг TypeScript
│   ├── tailwind.config.ts            # Конфіг TailwindCSS
│   └── package.json                  # Залежності Node.js
│
├── docs/                             # 📄 Документація
├── ROADMAP.md                        # Дорожня карта розробки
└── STRUCTURE.md                      # Цей файл
```

---

## 2️⃣ BACKEND — ДЕТАЛІЗОВАНА СТРУКТУРА

### 2.1 🏗️ Шари архітектури (Clean Architecture)

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
│   ├── __init__.py
│   └── auth_middleware.py            # JWT авторизація
│
├── services/                          # 🧠 ШАР БІЗНЕС-ЛОГІКИ
│   ├── __init__.py
│   ├── auth_service.py               # Авторизація (JWT, bcrypt)
│   ├── product_service.py            # Товари (логіка пошуку, фільтрації)
│   ├── document_service.py           # Документи (створення, проведення)
│   └── ledger_service.py             # Взаєморозрахунки
│
├── models/                            # 💾 ШАР ДАНИХ (SQLAlchemy)
│   ├── __init__.py
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
│   ├── __init__.py
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
│   ├── __init__.py
│   ├── config.py                     # Налаштування (pydantic-settings)
│   ├── security.py                   # JWT, хешування паролів
│   └── exceptions.py                 # Кастомні винятки
│
├── main.py                           # 🚀 Точка входу FastAPI
├── database.py                       # 🔗 Підключення до БД (async)
└── config.py                         # ⚙️ Конфігурація застосунку
```

### 2.2 📊 Моделі БД (SQLAlchemy)

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

### 2.3 🔌 API Ендпоінти (v1)

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

---

## 3️⃣ FRONTEND — ДЕТАЛІЗОВАНА СТРУКТУРА

### 3.1 🧩 Компонентна архітектура

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
│   ├── useCategories.ts              # Категорії (CRUD, дерево)
│   ├── useSuppliers.ts               # Постачальники (CRUD)
│   ├── useDocuments.ts               # Документи (CRUD, фільтрація)
│   ├── useBarcodeSearch.ts           # Пошук за штрих-кодом (debounce)
│   └── usePagination.ts              # Пагінація (сторінки, ліміти)
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
├── services/                          # 🌐 API СЕРВІСИ (axios)
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

### 3.2 🗺️ Маршрутизація (React Router)

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

### 3.3 🎨 UI Компоненти (TailwindCSS v4)

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

## 4️⃣ Tauri DESKTOP ОБГОРТКА

```
frontend/src-tauri/
├── src/
│   └── main.rs                        # Точка входу Tauri
├── Cargo.toml                         # Rust залежності
├── tauri.conf.json                    # Конфігурація Tauri
└── icons/                             # Іконки застосунку
```

---

## 5️⃣ БАЗА ДАНИХ (PostgreSQL)

### 5.1 Схема даних

```sql
-- Ключові таблиці:
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

### 5.2 Materialized Views (для звітів)

```sql
sales_report_view      -- Продажі за період (групування)
stock_report_view      -- Залишки товарів
supplier_ledger_view   -- Взаєморозрахунки з постачальниками
```

---

## 6️⃣ 🔄 ПОТОКИ ДАНИХ (Data Flow)

### 6.1 Створення прибуткової накладної
```
Frontend (InvoiceFormPage)
  → POST /api/v1/invoices
    → InvoiceService.create_invoice()
      → Створення Invoice + InvoiceItems
      → Оновлення залишків (Stock)
      → Оновлення SupplierLedger
  ← Відповідь: InvoiceResponse
```

### 6.2 Продаж товару (POS)
```
Frontend (PosPage)
  → Сканування ШК → GET /api/v1/products/barcode/{barcode}
    → ProductService.get_product_by_barcode()
  ← ProductResponse
  → Додавання в кошик (локальний стан)
  → POST /api/v1/receipts
    → ReceiptService.create_receipt()
      → Створення Receipt + ReceiptItems
      → Зменшення залишків
  ← ReceiptResponse
```

### 6.3 Авторизація
```
Frontend (LoginPage)
  → POST /api/v1/auth/login-pin
    → AuthService.authenticate_by_pin()
      → Пошук користувача за PIN
      → Генерація JWT токена
  ← { access_token, user }
  → Збереження токена в authStore (Zustand)
  → Додавання токена до заголовків axios (interceptor)
```

---

## 7️⃣ 🧪 ТЕСТУВАННЯ (QA)

| Рівень | Інструмент | Що тестуємо |
|--------|-----------|-------------|
| Unit | pytest | Сервіси, валідація, утиліти |
| Integration | pytest + httpx | API ендпоінти |
| E2E | Playwright | Користувацькі сценарії |
| UI | Storybook | Компоненти |

---

## 8️⃣ 📦 ЗАЛЕЖНОСТІ

### Backend (Python)
```
fastapi            — Веб-фреймворк
sqlalchemy[asyncio] — ORM (async)
alembic            — Міграції БД
asyncpg            — Драйвер PostgreSQL
pydantic           — Валідація даних
pydantic-settings  — Конфігурація
python-jose[cryptography] — JWT токени
passlib[bcrypt]    — Хешування паролів
python-multipart   — Form data
uvicorn            — ASGI сервер
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

---

## 9️⃣ 📋 СТАТУС РОЗРОБКИ

| Модуль | Статус | Примітки |
|--------|--------|----------|
| Backend: Моделі БД | ✅ Готово | Всі моделі створено |
| Backend: Міграції | ✅ Готово | Початкова міграція |
| Backend: API (CRUD) | ✅ Готово | Всі ендпоінти |
| Backend: Бізнес-логіка | ✅ Готово | Документи, проведення |
| Backend: Авторизація | ✅ Готово | JWT + PIN |
| Backend: Взаєморозрахунки | ✅ Готово | Ledger |
| Frontend: Layout | ✅ Готово | Sidebar, Header |
| Frontend: Login | ✅ Готово | PIN-авторизація |
| Frontend: POS | ✅ Готово | Кошик, оплата |
| Frontend: Товари | ✅ Готово | Список, форма |
| Frontend: Категорії | ✅ Готово | Список |
| Frontend: Постачальники | ✅ Готово | Список, форма |
| Frontend: Документи | ✅ Готово | Всі форми |
| Frontend: Взаєморозрахунки | ✅ Готово | Ledger |
| Frontend: Звіти | ⏳ В розробці | ReportsPage |
| Tauri Desktop | ⏳ В розробці | Обгортка |
| Фіскалізація (ПРРО) | ⏳ План | Спринт 5 |
| Negative Stock Logic | ⏳ План | Спринт 2 |
| Авто-замовлення | ⏳ План | Спринт 3 |

---

> **Документ створено:** PM Agent v1.0 (ALPHA_PM)  
> **Останнє оновлення:** 2026
