#!/usr/bin/env bash
# ============================================================================
# vpn-setup.sh — інтерактивне підняття VPN для сервера Torgashka (ЕТАП 0)
# ============================================================================
# Що робить: дає вибір Tailscale АБО ZeroTier, встановлює (якщо треба),
# запускає сервіс і показує VPN-IP сервера.
#
# ⚠️ Для роботи потрібен АКАУНТ і АВТОРИЗАЦІЯ:
#   - Tailscale:  tailscale up            → дасть URL для логіну в браузері
#                 (або tailscale up --authkey=<key> для безголового сервера)
#   - ZeroTier:   нужден network ID вашої мережі; після joіn — авторизуйте
#                 сервер в адмін-панелі my.zerotier.com
#
# Кроки (коментарі-інструкції для реального сервера):
#   1) sudo ./vpn-setup.sh
#   2) обрати 1 (Tailscale) або 2 (ZeroTier)
#   3) виконати інтерактивну авторизацію
#   4) скрипт покаже VPN-IP — його треба підставити в:
#        - deploy/network/postgresql.conf.d/torgashka-network.conf
#          (listen_addresses = '<VPN-IP>')
#        - torgashka-facade.service (TORGASHKA_FACADE_ADDR=<VPN-IP>:8000)
#        - README.md → curl-перевірка
#   5) firewall.sh дозволить порти ЛИШЕ з VPN-підмережі
# ============================================================================
set -euo pipefail

log() { echo "[vpn-setup] $*"; }
die() { echo "[vpn-setup] ПОМИЛКА: $*" >&2; exit 1; }

[[ $EUID -eq 0 ]] || die "запускайте від root (sudo)"

echo "════════════════════════════════════════════════════════════════════"
echo "  Torgashka — підняття VPN (Tailscale або ZeroTier)"
echo "════════════════════════════════════════════════════════════════════"
echo "  1) Tailscale  (рекомендовано: CGNAT 100.64.0.0/10, простий)"
echo "  2) ZeroTier   (власна підмережа 10.x/100.x, self-hosted можливо)"
read -rp "Ваш вибір [1/2]: " CHOICE

# ───────────────────────────────────────────────────────────────────────────
# Tailscale
# ───────────────────────────────────────────────────────────────────────────
if [[ "${CHOICE}" == "1" ]]; then
    if ! command -v tailscale >/dev/null 2>&1; then
        log "Tailscale не знайдено — встановлюю (офіційний скрипт)..."
        curl -fsSL https://tailscale.com/install.sh | sh
    fi
    log "Запускаю tailscale up — відкриється URL для авторизації..."
    log "  (на безголовому сервері: tailscale up --authkey=<TS_AUTH_KEY>)"
    tailscale up || true
    # Перевірка: чекаємо появи IP (до 30 сек)
    for i in $(seq 1 30); do
        IP="$(tailscale ip -4 2>/dev/null | head -1 || true)"
        [[ -n "${IP}" ]] && break
        sleep 1
    done
    [[ -n "${IP:-}" ]] || die "tailscale ip -4 не дав адресу — перевірте авторизацію"
    systemctl enable --now tailscaled >/dev/null 2>&1 || true
    log "Tailscale активний. VPN-IP сервера: ${IP}"
    log "Статус: $(tailscale status | head -1)"
    echo "▶ Далі: підставте ${IP} у конфіги (див. шапку скрипта)."

# ───────────────────────────────────────────────────────────────────────────
# ZeroTier
# ───────────────────────────────────────────────────────────────────────────
elif [[ "${CHOICE}" == "2" ]]; then
    if ! command -v zerotier-cli >/dev/null 2>&1; then
        log "ZeroTier не знайдено — встановлюю..."
        if command -v apt-get >/dev/null 2>&1; then
            curl -s https://install.zerotier.com | bash
        else
            die "підтримується Debian/Ubuntu (apt). Встановіть ZeroTier вручну: https://www.zerotier.com/download/"
        fi
    fi
    read -rp "Введіть Network ID ZeroTier (16 hex-символів): " NETID
    [[ "${NETID}" =~ ^[0-9a-fA-F]{16}$ ]] || die "невірний Network ID"
    zerotier-cli join "${NETID}" || true
    log "joined ${NETID}. Авторизуйте цей сервер в my.zerotier.com → Members → Auth"
    log "Чекаю авторизації (до 60 сек)..."
    IP=""
    for i in $(seq 1 60); do
        IP="$(zerotier-cli listnetworks 2>/dev/null | awk -v nid="${NETID}" '$2==nid {print $NF; exit}' | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
        [[ -n "${IP}" ]] && break
        sleep 1
    done
    [[ -n "${IP:-}" ]] || die "IP не з'явився — перевірте авторизацію в my.zerotier.com"
    systemctl enable --now zerotier-one >/dev/null 2>&1 || true
    log "ZeroTier активний. VPN-IP сервера: ${IP}"
    echo "▶ Далі: підставте ${IP} у конфіги (див. шапку скрипта)."

else
    die "невірний вибір: ${CHOICE} (очікувалось 1 або 2)"
fi

echo "────────────────────────────────────────────────────────────────────"
log "ГОТОВО. Перевірка з іншої машини у VPN: ping ${IP}"
