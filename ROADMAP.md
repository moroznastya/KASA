# 🗺️ Дорожня карта розробки POS-системи "Torgashka"

## 📋 Загальний опис
Enterprise-рівень POS-система (Каса + Склад/ERP) для малого та середнього бізнесу.
- **Backend:** FastAPI + SQLAlchemy 2.0 (async) + Alembic + PostgreSQL
- **Frontend:** React 18+ (Vite) + TypeScript + TailwindCSS v4
- **Desktop:** Tauri (Linux/Windows)
- **Архітектура:** Clean Architecture / DDD

---

## 🔷 Спринт 1: Фундамент (Моделі даних + CRUD + Міграції)
**Ролі:** DB_Admin_Agent, Python_Backend_Agent

### Етап 1.1 — Моделі даних (DB)
- [ ] Product (Super-Product Model: name, barcode, category, price, tax, images)
- [ ] Barcode (one-to-many: product_id, barcode, is_primary)
- [ ] Category (tree: id, name, parent_id)
- [ ] Supplier (id, name, contact_info, balance)
- [ ] Warehouse (id, name, address)
- [ ] Stock (product_id, warehouse_id, quantity, reserved_qty)
- [ ] User (id, username, pin_hash, role_id)
- [ ] Role (id, name, permissions JSON)
- [ ] Customer (id, name, phone, discount, bonus_balance)

### Етап 1.2 — Міграції (Alembic)
- [ ] Ініціалізація Alembic
- [ ] Створення міграцій для всіх моделей
- [ ] Seed-дані (admin user, базові ролі, категорії)

### Етап 1.3 — CRUD API (Backend)
- [ ] Products CRUD
- [ ] Categories CRUD
- [ ] Suppliers CRUD
- [ ] Warehouses CRUD
- [ ] Users CRUD + PIN-авторизація (bcrypt)
- [ ] Customers CRUD

---

## 🔷 Спринт 2: Бізнес-логіка (Документообіг + Склад)
**Ролі:** Python_Backend_Agent, DB_Admin_Agent

### Етап 2.1 — Документи
- [ ] Invoice (Прибуткова накладна)
- [ ] Transfer (Переміщення між складами)
- [ ] WriteOff (Списання)
- [ ] ReturnInvoice (Повернення постачальнику)
- [ ] SaleReceipt (Чек продажу)

### Етап 2.2 — Negative Stock Logic
- [ ] Конфігуратор дозволу/заборони негативних залишків
- [ ] Ієрархічний підхід (глобально → по складу → по товару)

### Етап 2.3 — RBAC (Ролі та права)
- [ ] Role-based permissions
- [ ] PIN-авторизація для дій (продаж, списання, звіти)

---

## 🔷 Спринт 3: Звіти + Авто-замовлення + Взаєморозрахунки
**Ролі:** Python_Backend_Agent, DB_Admin_Agent, QA_Agent

### Етап 3.1 — Materialized Views
- [ ] SalesReportView (продажі за період)
- [ ] StockReportView (залишки)
- [ ] SupplierLedgerView (взаєморозрахунки)

### Етап 3.2 — Авто-замовлення
- [ ] Алгоритм minimum_stock → recommended_qty
- [ ] Генерація AutoOrder документів

### Етап 3.3 — SupplierLedger
- [ ] Часткові оплати постачальникам
- [ ] Історія взаєморозрахунків

---

## 🔷 Спринт 4: Frontend (POS + UI/UX)
**Ролі:** React_UI_UX_Agent, Tauri_Agent

### Етап 4.1 — Основний UI
- [ ] Layout (Sidebar + Header + Content)
- [ ] Сторінка входу (PIN-клавіатура)
- [ ] Dashboard (головна панель)

### Етап 4.2 — POS-каса
- [ ] Екран продажу (товари, кошик, оплата)
- [ ] Сканер штрих-кодів (keyboard wedge)
- [ ] Друк чеків

### Етап 4.3 — Складські сторінки
- [ ] Товари (список + фільтри + редагування)
- [ ] Документи (створення/перегляд)
- [ ] Звіти (таблиці + графіки)

### Етап 4.4 — Tauri Desktop
- [ ] Обгортка Tauri
- [ ] Налаштування друку
- [ ] Офлайн-режим (localStorage)

---

## 🔷 Спринт 5: Фіскалізація + Фінальне тестування
**Ролі:** Python_Backend_Agent, QA_Agent, React_UI_UX_Agent

### Етап 5.1 — ПРРО (програмний РРО)
- [ ] Інтеграція з фіскальним сервером
- [ ] FiscalSplitter (розподіл ПДВ/акцизу)
- [ ] Підакцизні товари (маркування)

### Етап 5.2 — QA та оптимізація
- [ ] Тестування всіх сценаріїв
- [ ] Оптимізація запитів
- [ ] Документація API (Swagger)

---

## 📅 Орієнтовні терміни
| Спринт | Тривалість | Статус |
|--------|-----------|--------|
| Спринт 1 | 5 днів | ⏳ Очікує |
| Спринт 2 | 5 днів | ⏳ |
| Спринт 3 | 4 дні | ⏳ |
| Спринт 4 | 7 днів | ⏳ |
| Спринт 5 | 4 дні | ⏳ |
| **Всього** | **25 днів** | |

---

*Створено: PM Agent v1.0 (ALPHA_PM)*
