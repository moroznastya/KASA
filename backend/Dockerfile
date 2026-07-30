# =============================================================================
# Kasa POS — Multi-stage Dockerfile (Backend)
# =============================================================================
# Використання:
#   docker build -t kasa-backend .
#   docker run -p 8000:8000 kasa-backend
# =============================================================================

# ─── Stage 1: Builder ────────────────────────────────────────────────────────
# Встановлюємо залежності в окремому шарі для кешування
FROM python:3.11-slim AS builder

LABEL stage="builder"
LABEL description="Kasa Backend — build stage"

WORKDIR /app

# Встановлюємо системні залежності (для psycopg2/bcrypt/asyncpg)
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Копіюємо тільки requirements.txt для кешування залежностей
COPY requirements.txt .
RUN pip install --no-cache-dir --upgrade pip && \
    pip install --no-cache-dir --user -r requirements.txt

# ─── Stage 2: Runtime ────────────────────────────────────────────────────────
# Мінімальний образ для запуску
FROM python:3.11-slim AS runtime

LABEL maintainer="Kasa POS Team"
LABEL description="Kasa POS — Enterprise Point of Sale System (Backend)"
LABEL version="1.0.0"

WORKDIR /app

# Встановлюємо тільки runtime-залежності (libpq для asyncpg)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Створюємо користувача для безпеки (non-root)
RUN groupadd -r kasa && useradd -r -g kasa -d /app -s /sbin/nologin kasa

# Копіюємо встановлені пакети з builder
COPY --from=builder /root/.local /root/.local

# Копіюємо код застосунку
COPY --chown=kasa:kasa . .

# Додаємо user-пакети до PATH
ENV PATH=/root/.local/bin:$PATH \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

# Переключаємося на non-root користувача
USER kasa

# Health check
HEALTHCHECK --interval=30s --timeout=10s --retries=3 \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://localhost:8000/health')" || exit 1

# Відкриваємо порт
EXPOSE 8000

# Запускаємо FastAPI через uvicorn
CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
