#!/usr/bin/env bash
# ============================================================================
# Встановлення крипто-ядра ІІТ (SDK EUSignCP) для ДСТУ 4145-2002 підпису ПРРО
# ============================================================================
# Завантажує офіційний пакет ІІТ (euswi.64.deb) з iit.com.ua та розпаковує
# його БЕЗ root у backend/vendor/iit-sdk/. Потрібна бібліотека euscp.so —
# повноцінне крипто-ядро (ДСТУ 4145-2002 + ДСТУ 7564:2014 / Стрибог-256),
# яке читає JKS-ключі та формує CAdES-підпис, що розуміє сервер ДПС.
#
# Використання:
#   bash backend/scripts/setup_iit_sdk.sh
#
# Джерело: https://iit.com.ua/downloads → «EUSign для Linux (64-bit)»
# (пряме посилання /download/productfiles/euswi.64.deb).
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$(dirname "$SCRIPT_DIR")"
VENDOR_DIR="$BACKEND_DIR/vendor"
SDK_DIR="$VENDOR_DIR/iit-sdk"
URL="https://iit.com.ua/download/productfiles/euswi.64.deb"
DEB="/tmp/euswi.64.deb"

echo "→ Каталог SDK: $SDK_DIR"

if [ -f "$SDK_DIR/opt/iit/eu/sw/euscp.so" ]; then
    echo "✅ SDK вже встановлено ($SDK_DIR/opt/iit/eu/sw/euscp.so)"
    exit 0
fi

mkdir -p "$VENDOR_DIR"

echo "→ Завантаження $URL ..."
if command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "$DEB" "$URL"
elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$DEB" "$URL"
else
    echo "❌ Потрібен curl або wget" >&2
    exit 1
fi

if [ ! -s "$DEB" ]; then
    echo "❌ Не вдалося завантажити SDK (може бути капча/авторизація на iit.com.ua)." >&2
    echo "   Завантажте вручну: $URL" >&2
    echo "   і розпакуйте: mkdir -p $SDK_DIR && dpkg-deb -x $DEB $SDK_DIR" >&2
    exit 1
fi

echo "→ Розпакування ..."
mkdir -p "$SDK_DIR"
dpkg-deb -x "$DEB" "$SDK_DIR"

LIB="$SDK_DIR/opt/iit/eu/sw/euscp.so"
if [ -f "$LIB" ]; then
    echo "✅ SDK встановлено: $LIB"
    echo "   Перевірка: ldd $LIB"
    ldd "$LIB" | head -5 || true
else
    echo "❌ Бібліотеку euscp.so не знайдено після розпакування" >&2
    exit 1
fi
