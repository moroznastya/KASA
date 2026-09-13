#!/usr/bin/env bash
# ── Перезапуск каси + приймальний замір. З автовідкатом бінарника. ──
#
# Каса керується systemd-юнітом torgashka.service (Restart=on-failure).
# НЕ запускати застосунок через nohup: поза cgroup systemd він помирає
# разом із сесією і лишає :8000 зайнятим — саме так 13.09.2026 каса
# «зникла» після ручного перезапуску.
#
# Робочий каталог юніта = тека проєкту (див. override.conf), тому
# перейменування теки проєкту ламає запуск (status=200/CHDIR).
#
# Використання:
#   bash artifacts/scripts/restart_and_verify.sh            # рестарт + замір
#   bash artifacts/scripts/restart_and_verify.sh --install  # спочатку покласти свіжий бінарник
set -uo pipefail

. "$(dirname "$0")/_lib.sh"

PROJECT=/home/anastasia/Andriy/aegis_v3/Niko/Projects/Torgashka
APP=/home/anastasia/.local/bin/torgashka
UNIT=torgashka.service
BACKUP_DIR="$PROJECT/artifacts/backups"
BUILT="$PROJECT/frontend/src-tauri/target/release/torgashka"

if [ "${1:-}" = "--install" ]; then
  if [ ! -f "$BUILT" ]; then
    echo "  ✗ немає зібраного бінарника: $BUILT"
    exit 1
  fi
  mkdir -p "$BACKUP_DIR"
  if [ -f "$APP" ]; then
    cp -f "$APP" "$BACKUP_DIR/torgashka.$(date +%Y%m%d_%H%M%S)"
    echo "  бекап поточного бінарника → $BACKUP_DIR"
  fi
  install -m 755 "$BUILT" "$APP"
  echo "  встановлено свіжий бінарник ($(stat -c%s "$APP") байт)"
fi

echo "=== 1. рестарт через systemd ==="
systemctl --user is-active "$UNIT" >/dev/null 2>&1 && echo "  був активний → рестарт" || echo "  був неактивний → старт"
systemctl --user restart "$UNIT"
echo "  стан: $(systemctl --user is-active "$UNIT")"

echo
echo "=== 2. чекаю готовності API ==="
ok=0
for i in $(seq 1 60); do
  code=$(curl -s -m 3 -o /dev/null -w "%{http_code}" \
    "${TORGASHKA_API_BASE:-http://127.0.0.1:8000}/api/v1/health" 2>/dev/null)
  if [ "$code" = "200" ]; then echo "  API готовий за ${i}с"; ok=1; break; fi
  sleep 1
done

if [ "$ok" != "1" ]; then
  echo "  ✗ API не піднявся. Останні рядки журналу:"
  journalctl --user -u "$UNIT" -n 15 --no-pager | sed 's/^/      /'
  echo
  echo "  Найчастіші причини:"
  echo "    • status=200/CHDIR — WorkingDirectory у юніті вказує в неіснуючу теку"
  echo "    • «адреса вже зайнята» — лишився процес, запущений повз systemd:"
  echo "        pgrep -af 'bin/torgashka'   (убити зайвий вручну)"
  exit 1
fi

echo
echo "=== 3. який бінарник обслуговує ==="
pid=$(systemctl --user show -p MainPID --value "$UNIT")
echo "  PID $pid → $(readlink -f "/proc/$pid/exe" 2>/dev/null)"
echo "  зібрано: $(stat -c%y "$APP" | cut -c1-19)"

echo
echo "=== 4. приймальний замір round-trips ==="
python3 "$PROJECT/artifacts/scripts/acceptance_roundtrip.py"
rc=$?

if [ "$rc" != "0" ]; then
  echo
  echo "  ✗ приймальний тест НЕ пройдено (rc=$rc)"
  echo "  відкат: покласти останній бекап із $BACKUP_DIR і повторити рестарт"
fi
exit "$rc"
