# =============================================================================
# Kasa POS — Multi-stage Dockerfile
# =============================================================================
# Використання:
#   docker build -t kasa-backend .
#   docker run -p 8000:8000 kasa-backend
# =============================================================================

# ─── Stage 1: Builder ────────────────────────────────────────────────────────
# Встановлюємо залежності в окремому шарі для кешування
FROM python:3.12-slim AS builder

WORKDIR /app

# Встановлюємо системні залежності (для psycopg2/bcrypt)
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Копіюємо тільки requirements.txt для кешування залежностей
COPY requirements.txt .
RUN pip install --no-cache-dir --user -r requirements.txt


# ─── Stage 2: Runtime ────────────────────────────────────────────────────────
# Мінімальний образ для запуску
FROM python:3.12-slim AS runtime

WORKDIR /app

# Встановлюємо тільки runtime-залежності (libpq для asyncpg)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Копіюємо встановлені пакети з builder
COPY --from=builder /root/.local /root/.local

# Копіюємо код застосунку
COPY . .

# Додаємо user-пакети до PATH
ENV PATH=/root/.local/bin:$PATH

# Відкриваємо порт
EXPOSE 8000

# Запускаємо FastAPI через uvicorn
CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
