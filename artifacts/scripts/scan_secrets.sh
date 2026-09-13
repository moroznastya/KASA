#!/usr/bin/env bash
# Пошук секретів у файлах, які потрапляють у git.
#
# Навмисно шукає ФОРМИ, а не конкретні значення — щоб сам сканер
# не став новим джерелом витоку.
#
# Використання:
#   artifacts/scripts/scan_secrets.sh              # сканує artifacts/ (жорсткий гейт)
#   artifacts/scripts/scan_secrets.sh --all        # сканує весь репозиторій (звіт)
#   artifacts/scripts/scan_secrets.sh <path>...    # конкретні шляхи
#
# Код виходу: 0 — чисто, 1 — знайдено.
set -uo pipefail

SCOPE=("artifacts")
MODE="strict"
case "${1:-}" in
  --all) SCOPE=("."); shift || true ;;
  "")    ;;
  *)     SCOPE=("$@") ;;
esac

# Файли, де такі рядки — легітимні шаблони/сам сканер.
ALLOW='(\.example$|scan_secrets\.sh$|_lib\.sh$|_env\.py$)'

# Форми секретів. Роздільники [=:] з лапками ураховують YAML/TOML/Python.
PATTERNS=(
  'PGPASSWORD=[^"'"'"'$[:space:]]{3,}'
  '(password|passwd|pwd)[[:space:]]*[=:][[:space:]]*["'"'"'][^"'"'"']{3,}'
  'eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.'
  '-----BEGIN [A-Z ]*PRIVATE KEY-----'
  'gh[pousr]_[A-Za-z0-9]{20,}'
  'github_pat_[A-Za-z0-9_]{20,}'
  'AKIA[0-9A-Z]{16}'
  'AIza[0-9A-Za-z_-]{35}'
  'xox[baprs]-[A-Za-z0-9-]{10,}'
  '(host|HOST)[[:space:]]*[=:][[:space:]]*["'"'"']?10\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}'
)

found=0
for pat in "${PATTERNS[@]}"; do
  hits=$(git grep -nIE --no-color -e "$pat" -- "${SCOPE[@]}" 2>/dev/null | grep -vE "$ALLOW" || true)
  if [ -n "$hits" ]; then
    found=1
    echo "  ✗ форма секрету: $pat"
    echo "$hits" | sed 's/^/      /' | cut -c1-160
  fi
done

if [ "$found" = "0" ]; then
  echo "  ✓ секретів не знайдено (область: ${SCOPE[*]})"
  exit 0
fi

echo
echo "  Знайдено збіги. Якщо це хибне спрацювання — додайте шлях у ALLOW."
if [ "$MODE" = "strict" ] && [[ " ${SCOPE[*]} " == *" . "* ]]; then
  exit 1
fi
[ "${1:-}" = "--all" ] && exit 0
exit 1
