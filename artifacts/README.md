# artifacts/ — робочі зрізи досліджень

Тут лежать заміри, діагностичні скрипти, SQL-плани та звіти, зроблені під час
роботи над продуктивністю каталогу (13.09.2026). Це не частина застосунку —
це слід дослідження, щоб рішення можна було перевірити, а не вірити на слово.

## Що де

| Тека | Що всередині |
|------|--------------|
| `scripts/` | діагностичні скрипти + приймальний тест |
| `index_optimization/` | звіт і план щодо індексу пошуку товарів |
| `db_cleanup/` | звіт і SQL-кроки чищення БД |
| `hub_snapshot/` | верифікований знімок БД хаба + MANIFEST |
| `backups/` | локальні бекапи бінарника/БД — **у git не потрапляє** |
| `screenshots/` | локальні скріншоти — **у git не потрапляє** |

## Налаштування (обов'язково перед запуском)

Скрипти не містять секретів — вони читають їх із `artifacts/.env.local`:

```bash
cp artifacts/.env.local.example artifacts/.env.local
$EDITOR artifacts/.env.local     # вписати хаб і пароль
chmod 600 artifacts/.env.local
```

- bash-скрипти підключають `scripts/_lib.sh` (дає `$HUB_HOST`, `$PGPASSWORD`, `hub_psql`)
- python-скрипти імпортують `scripts/_env.py` (дає `HUB_*`, `hub_connect()`, `api_token()`)

## Ключові скрипти

```bash
# приймальний тест: 6 ендпоінтів каталогу проти цілей, PASS/FAIL
python3 artifacts/scripts/acceptance_roundtrip.py

# таймлайн SQL-подій за час HTTP-запиту (рахує мережеві круги)
python3 artifacts/scripts/sql_timeline2.py

# перезапуск каси через systemd + замір (з відкатом)
bash artifacts/scripts/restart_and_verify.sh

# гейт: секрети у файлах, які йдуть у git
bash artifacts/scripts/scan_secrets.sh          # artifacts/ (жорстко)
bash artifacts/scripts/scan_secrets.sh --all    # весь репо (звіт)
```

## Правила теки

1. **Ніяких секретів.** Навіть у тимчасових скриптах. Перевіряється в CI
   (`artifacts.yml`, job `secret-scan`) і локально `scan_secrets.sh`.
2. **Дампи/бекапи не комітяться** (`.gitignore`): це десятки мегабайт і реальні
   дані підприємства. Тримайте локально.
3. **Скріншоти не комітяться** без перевірки вмісту — у кадр може потрапити токен.
