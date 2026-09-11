# =============================================================================
# Torgashka POS — Multi-stage Dockerfile (Backend)
# =============================================================================
# Використання:
#   docker build -t torgashka-backend .
#   docker run -p 8001:8001 torgashka-backend
# =============================================================================
#
# 🔧 Оновлення 2026-08-01 (контейнеризація):
#   - Python 3.11 → 3.12 (відповідає pyproject.toml та вимогам проєкту)
#   - Додано python3-dev у builder: потрібен для збірки twofish (ПРРО-крипто)
#   - Залежності встановлюються у /opt/venv (virtualenv):
#     * доступно non-root користувачу torgashka (на відміну від /root/.local)
#     * чистий ізольований site-packages, не засмічує системний Python
# =============================================================================

# ─── Stage 1: Builder ────────────────────────────────────────────────────────
# Встановлюємо залежності в окремому шарі для кешування
FROM python:3.12-slim AS builder

LABEL stage="builder"
LABEL description="Torgashka Backend — build stage"

WORKDIR /app

# Встановлюємо системні залежності:
#   gcc           — компілятор для C-екстеншнів (asyncpg, grpcio, twofish)
#   python3-dev   — заголовкові файли Python.h (обов'язково для twofish!)
#   libpq-dev     — заголовки libpq (для psycopg2/asyncpg)
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc \
    python3-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Створюємо віртуальне середовище та встановлюємо залежності
COPY requirements.txt .
RUN python -m venv /opt/venv && \
    /opt/venv/bin/pip install --no-cache-dir --upgrade pip && \
    /opt/venv/bin/pip install --no-cache-dir -r requirements.txt

# ─── Stage 2: Runtime ────────────────────────────────────────────────────────
# Мінімальний образ для запуску
FROM python:3.12-slim AS runtime

LABEL maintainer="Torgashka POS Team"
LABEL description="Torgashka POS — Enterprise Point of Sale System (Backend)"
LABEL version="1.0.0"

WORKDIR /app

# Встановлюємо тільки runtime-залежності (libpq для asyncpg/psycopg2)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libpq-dev \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Створюємо користувача для безпеки (non-root)
RUN groupadd -r torgashka && useradd -r -g torgashka -d /app -s /sbin/nologin torgashka

# Копіюємо віртуальне середовище з builder
COPY --from=builder /opt/venv /opt/venv

# Копіюємо код застосунку (секрети НЕ копіюються — див. .dockerignore)
COPY --chown=torgashka:torgashka . .

# Додаємо venv до PATH
ENV PATH=/opt/venv/bin:$PATH \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

# Переключаємося на non-root користувача
USER torgashka

# ⚠️ ЕТАП 8: Python sidecar дезактивовано — Rust-ядро torgashka-api :8000.
# Healthcheck перевіряє Rust-фасад (/api/v1/health). Контейнер збережено
# як legacy (docker-compose: profiles: legacy, не запускається за замовчуванням).
HEALTHCHECK --interval=30s --timeout=10s --retries=3 --start-period=15s \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://localhost:8000/api/v1/health')" || exit 1

# Відкриваємо порт
EXPOSE 8001

# Запускаємо FastAPI через uvicorn
CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8001"]
