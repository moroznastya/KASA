#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# Kasa POS — Tauri Dependencies Setup (Ubuntu/Debian)
# ─────────────────────────────────────────────────────────────────────────────
# Запустити: bash setup.sh
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

echo "🔧 Встановлення залежностей для Tauri v2..."

# Системні пакети для Tauri на Linux
sudo apt-get update
sudo apt-get install -y \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    patchelf \
    build-essential \
    pkg-config \
    libssl-dev

echo ""
echo "✅ Системні залежності встановлено!"
echo ""
echo "🚀 Для запуску Tauri в режимі розробки:"
echo "   cd frontend && npm run tauri dev"
echo ""
echo "📦 Для збірки production-версії:"
echo "   cd frontend && npm run tauri build"
