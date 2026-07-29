# ============================================================================
# Kasa POS Frontend — Multi-stage Dockerfile
# ============================================================================
# Багатостадійна збірка: node build → nginx production
# ============================================================================
# Використання:
#   docker build -t kasa-frontend -f kasa/frontend/Dockerfile .
#   docker run -p 3000:80 kasa-frontend
# ============================================================================

# ─── Stage 1: Node Builder ───────────────────────────────────────────────────
FROM node:20-alpine AS builder

LABEL stage="builder"
LABEL description="Kasa Frontend — build stage"

WORKDIR /build

# Копіюємо package.json та package-lock.json
COPY package.json package-lock.json ./

# Встановлюємо залежності
RUN npm ci

# Копіюємо весь код
COPY . .

# Збираємо production build
RUN npm run build

# ─── Stage 2: Nginx Production ──────────────────────────────────────────────
FROM nginx:1.27-alpine AS production

LABEL maintainer="Kasa POS Team"
LABEL description="Kasa POS Frontend — React + TypeScript"
LABEL version="1.0.0"
LABEL org.opencontainers.image.source="https://github.com/niko-pos/niko"
LABEL org.opencontainers.image.description="Kasa POS — Point of Sale System Frontend"

# Видаляємо дефолтну конфігурацію Nginx
RUN rm -rf /usr/share/nginx/html/* && \
    rm -rf /etc/nginx/conf.d/default.conf

# Копіюємо зібраний фронтенд
COPY --from=builder /build/dist/ /usr/share/nginx/html/

# Копіюємо кастомну конфігурацію Nginx
COPY nginx.conf /etc/nginx/conf.d/default.conf

# Health check
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD wget -qO- http://localhost:80/health || exit 1

# Порти
EXPOSE 80

# Запуск Nginx
CMD ["nginx", "-g", "daemon off;"]
