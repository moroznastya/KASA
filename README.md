# 🏪 Kasa POS v3.0

**Enterprise-рівень POS-система (Каса + Склад/ERP)** для малого та середнього бізнесу.

| Компонент | Технологія |
|-----------|-----------|
| **Backend (продакшн)** | Rust — вбудований фасад (axum) на `127.0.0.1:8000` |
| **Backend (legacy)** | FastAPI + SQLAlchemy 2.0 — дезактивований, лише еталон для differential-тестів |
| **Frontend** | React 18+ (Vite) + TypeScript + TailwindCSS v4 |
| **Desktop** | Tauri (Linux/Windows) |
| **База даних** | PostgreSQL 15+ |
| **Архітектура** | Clean Architecture / DDD |

> **Міграція завершена (етап 8).** Python-бекенд ПОВНІСТЮ дезактивований.
> Продакшн-шлях: **React (`frontend/`) → Tauri-оболонка → вбудований Rust-фасад (axum) на `127.0.0.1:8000` → PostgreSQL. 100% Rust.**

---

## 📋 Вимоги

- **Node.js** 18+
- **PostgreSQL** 15+
- **Rust** (stable, для збірки Tauri/бінарника)
- *Python 3.11+ — ТІЛЬКИ для differential-тестів проти legacy-еталона (не runtime)*

---

## 🚀 Швидкий старт

### 1. Клонування та налаштування

```bash
git clone <repository-url> kasa
cd kasa
```

### 2. Запуск PostgreSQL

```bash
# PostgreSQL 15+ має бути запущений. Створіть БД та користувача:
#   CREATE USER kasa WITH PASSWORD '<пароль>';
#   CREATE DATABASE kasa OWNER kasa;
# Параметри підключення — у frontend/src-tauri/.env (або оточенні процесу).
```

### 3. Запуск Rust-фасаду (основний шлях)

```bash
cd frontend/src-tauri

# Debug-збірка
cargo build
./target/debug/kasa-pos

# або release:
cargo build --release
./target/release/kasa-pos
```

Бінарник слухає **http://127.0.0.1:8000** — це і є backend системи (Rust-фасад, axum).

### 4. Запуск Frontend (розробка)

```bash
cd frontend

npm install
npm run dev
```

Frontend доступний за адресою: **http://localhost:5173** (proxy на Rust-фасад `:8000`).

### 5. Запуск Desktop (Tauri)

```bash
cd frontend
npm run tauri dev        # розробка: збирає Rust-фасад + React
npm run tauri build      # production-збірка
```

> ⚠️ **Python-бекенд (`backend/`) НЕ запускається** — він дезактивований і не є runtime.
> `docker-compose up -d` більше НЕ піднімає всю систему: сервіс `backend` винесено
> у профіль `legacy` (лише для відкату/налагодження). Звернення до деприкейтнутих
> v2-аліасів та legacy-роутів повертає **410 Gone**.

---

## 📁 Структура проєкту

```
kasa/
├── frontend/
│   ├── src/                  # React компоненти
│   ├── src-tauri/            # Tauri оболонка + Rust-фасад (100% Rust)
│   │   ├── crates/
│   │   │   ├── kasa-api/             # HTTP-фасад (axum), роути v1
│   │   │   ├── kasa-application/     # Застосунковий шар (use cases)
│   │   │   ├── kasa-domain/          # Доменні сутності та правила
│   │   │   ├── kasa-infrastructure/  # PostgreSQL, репозиторії, міграції
│   │   │   ├── kasa-ocr/             # OCR (розпізнавання документів)
│   │   │   └── kasa-prro/            # ПРРО/фіскалізація
│   │   └── target/debug/kasa-pos     # Зібраний бінарник (слухає :8000)
│   └── package.json          # Node.js залежності
├── backend/                  # 🧪 LEGACY Python-бекенд (FastAPI) — дезактивований
│   ├── app/                  #   Еталон для differential-тестів, НЕ runtime
│   ├── alembic/              #   Історичні міграції
│   └── requirements.txt
├── docs/                     # Документація
│   ├── architecture/         # Архітектурні рішення
│   ├── adr/                  # Architectural Decision Records
│   └── infrastructure/       # Інфраструктурна документація
├── docker-compose.yml        # PostgreSQL + backend (профіль legacy)
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
- ✅ **Rust-фасад** — 157/164 роути покрито, 7 деприкейтнутих v2-аліасів → 410
- ✅ **ПРРО/OCR** — крейти `kasa-prro`, `kasa-ocr`

---

## 🧪 Тестування

```bash
# Rust (основний стек)
cd frontend/src-tauri
cargo test

# Frontend тести
cd frontend
npm test

# Differential-тести проти legacy-еталона (Python — лише як референс, не runtime)
cd backend
pytest -v
```

---

## 📄 Ліцензія

MIT License — для внутрішнього використання та комерційного впровадження.

---

*Kasa POS v3.0 — Rust-фасад (axum) | React | Tauri | PostgreSQL | Clean Architecture / DDD*
