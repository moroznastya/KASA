#!/usr/bin/env bash
# ============================================================================
# firewall.sh — мережевий firewall для сервера Torgashka (ЕТАП 0, sync-offline)
# ============================================================================
# Призначення: на фізичному VPS/VPN-сервері дозволити порти фасаду (8000/tcp)
# і PostgreSQL (5432/tcp) ЛИШЕ з VPN-підмережі; решту — deny.
#
# ⚠️ Скрипт ПЕРЕДБАЧАЄ, що VPN вже піднято (див. vpn-setup.sh) і ви знаєте
#    свою VPN-підмережу. За замовчуванням підставлена Tailscale CGNAT
#    100.64.0.0/10 — відкоригуйте VPN_SUBNET під свій варіант
#    (ZeroTier: 10.147.17.0/24 або 100.x.y.0/24).
#
# Ідемпотентність: скрипт можна запускати багаторазово — повторні правила
#    ufw/nft не дублюються (ufw сам дедуплікує; nft-гілка перезаписує набір).
#
# Використання:
#   sudo ./firewall.sh            # застосувати (ufw, якщо є; інакше nftables)
#   sudo ./firewall.sh status     # показати поточний стан
# ============================================================================
set -euo pipefail

# ── Налаштування (змініть під середовище!) ────────────────────────────────
VPN_SUBNET="${VPN_SUBNET:-100.64.0.0/10}"   # Tailscale CGNAT за замовчуванням
FACADE_PORT=8000                            # Rust-фасад torgashka-api
PG_PORT=5432                                # PostgreSQL
SSH_PORT=22                                 # SSH лишити відкритим (керування)
# ───────────────────────────────────────────────────────────────────────────

log() { echo "[firewall] $*"; }
die() { echo "[firewall] ПОМИЛКА: $*" >&2; exit 1; }

if [[ "${1:-}" == "status" ]]; then
    if command -v ufw >/dev/null 2>&1 && ufw status | grep -q "Status: active"; then
        ufw status verbose
    elif command -v nft >/dev/null 2>&1; then
        nft list ruleset
    else
        die "жоден активний firewall (ufw/nftables) не знайдено"
    fi
    exit 0
fi

[[ $EUID -eq 0 ]] || die "запускайте від root (sudo)"

# ── Гілка 1: ufw (Ubuntu/Debian, найпоширеніший варіант) ──────────────────
if command -v ufw >/dev/null 2>&1; then
    log "ufw знайдено — застосовую через ufw"
    ufw default deny incoming
    ufw default allow outgoing
    # SSH — звідусіль (керування сервером); за бажання звузьте до вашого IP
    ufw allow "${SSH_PORT}/tcp" comment 'SSH admin'
    # Фасад + PostgreSQL — ЛИШЕ з VPN-підмережі
    ufw allow from "${VPN_SUBNET}" to any port "${FACADE_PORT}" proto tcp \
        comment 'Torgashka facade (VPN only)'
    ufw allow from "${VPN_SUBNET}" to any port "${PG_PORT}" proto tcp \
        comment 'Torgashka PostgreSQL (VPN only)'
    # Явна відмова решти (вже покрита default deny, але для наочності):
    # ufw deny 8000/tcp  # не обов'язково — default deny incoming це робить
    ufw --force enable
    ufw status verbose
    log "ufw застосовано. Перевірка ззовні: curl має таймаутити (див. README)."
    exit 0
fi

# ── Гілка 2: nftables (сервери без ufw) ───────────────────────────────────
if command -v nft >/dev/null 2>&1; then
    log "nftables знайдено — застосовую через nft"
    # Ідемпотентність: перезаписуємо набір torgashka_allow, а не правила.
    nft list table inet torgashka_fw >/dev/null 2>&1 || \
        nft add table inet torgashka_fw
    nft flush table inet torgashka_fw
    nft -f - <<EOF
table inet torgashka_fw {
    set vpn_subnet {
        type ipv4_addr
        flags interval
        elements = { ${VPN_SUBNET} }
    }
    chain input {
        type filter hook input priority filter; policy drop;
        ct state established,related accept
        iif lo accept
        ip saddr @vpn_subnet tcp dport { ${FACADE_PORT}, ${PG_PORT} } accept
        tcp dport ${SSH_PORT} accept
        ct state invalid drop
    }
    chain forward { type filter hook forward priority filter; policy drop; }
    chain output { type filter hook output priority filter; policy accept; }
}
EOF
    log "nftables застосовано:"
    nft list ruleset
    log "Підказка: щоб зробити правила постійними — "
    log "  sudo nft list ruleset > /etc/nftables.conf && systemctl enable nftables"
    exit 0
fi

die "не знайдено ufw або nftables — встановіть один із них (apt install ufw | nftables)"
