# 🏪 Kasa POS v2.0

**Enterprise-рівень POS-система (Каса + Склад/ERP)** для малого та середнього бізнесу.

| Компонент | Технологія |
|-----------|-----------|
| **Backend** | FastAPI + SQLAlchemy 2.0 (async) + Alembic + PostgreSQL |
| **Frontend** | React 18+ (Vite) + TypeScript + TailwindCSS v4 |
| **Desktop** | Tauri (Linux/Windows) |
| **Архітектура** | Clean Architecture / DDD |

---

## 📋 Вимоги

- **Python** 3.11+
- **Node.js** 18+
- **PostgreSQL** 15+
- **Rust** (для Tauri desktop збірки)

---

## 🚀 Швидкий старт

### 1. Клонування та налаштування

```bash
git clone <repository-url> kasa
cd kasa
```

### 2. Налаштування середовища

```bash
# Скопіюйте приклад .env та відредагуйте під своє оточення
cp backend/.env.example backend/.env
# Відредагуйте backend/.env — вкажіть свої дані для підключення до БД
```

### 3. Запуск Backend

```bash
cd backend

# Створіть віртуальне середовище
python -m venv venv
source venv/bin/activate  # Linux/Mac
# або: venv\Scripts\activate  # Windows

# Встановіть залежності
pip install -r requirements.txt

# Застосуйте міграції
alembic upgrade head

# Заповніть тестовими даними (опціонально)
python seed.py

# Запустіть сервер
uvicorn app.main:app --reload --host 0.0.0.0 --port 8000
```

Backend буде доступний за адресою: **http://localhost:8000**
Swagger документація: **http://localhost:8000/docs**

### 4. Запуск Frontend

```bash
cd frontend

# Встановіть залежності
npm install

# Запустіть в режимі розробки
npm run dev
```

Frontend буде доступний за адресою: **http://localhost:5173**

### 5. Запуск через Docker (альтернатива)

```bash
# Вся система (PostgreSQL + Backend + Frontend)
docker-compose up -d
```

---

## 📁 Структура проєкту

```
kasa/
├── backend/
│   ├── app/              # Основний код FastAPI
│   │   ├── api/          # Роутери (endpoints)
│   │   ├── core/         # Конфігурація, middleware
│   │   ├── models/       # SQLAlchemy ORM моделі
│   │   ├── schemas/      # Pydantic схеми
│   │   └── services/     # Бізнес-логіка
│   ├── alembic/          # Міграції БД
│   ├── .env              # Змінні середовища (не в git!)
│   └── requirements.txt  # Python залежності
├── frontend/
│   ├── src/              # React компоненти
│   ├── src-tauri/        # Tauri desktop обгортка
│   └── package.json      # Node.js залежності
├── docs/                 # Документація
│   ├── architecture/     # Архітектурні рішення
│   ├── adr/              # Architectural Decision Records
│   └── infrastructure/   # Інфраструктурна документація
├── .gitignore
└── README.md
```

---

## 🔧 Основні можливості

- ✅ **Товари** — каталог, штрих-коди, категорії, ціни, податки
- ✅ **Склад** — залишки, резервування, негативні залишки (конфігурується)
- ✅ **Документи** — прибуткові накладні, переміщення, списання, повернення
- ✅ **POS-каса** — продажі, сканер штрих-кодів, друк чеків
- ✅ **Звіти** — продажі, залишки, взаєморозрахунки
- ✅ **RBAC** — ролі та права доступу, PIN-авторизація
- ✅ **Desktop** — Tauri обгортка для Linux/Windows

---

## 🧪 Тестування

```bash
# Backend тести
cd backend
pytest -v

# Frontend тести
cd frontend
npm test
```

---

## 📄 Ліцензія

MIT License — для внутрішнього використання та комерційного впровадження.

---

*Kasa POS v2.0 — Clean Architecture | DDD | Enterprise Grade*
