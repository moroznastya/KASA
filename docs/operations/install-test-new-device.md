# Встановлення Torgashka на інший пристрій і тестування — покроково

> Гілка: `feat/pg-replication` (PG-реплікація + SQLite offline-черга).
> Цільовий пристрій: **Windows** (Zotac ZBOX — primary; каса/інший ПК — standby).
> Якщо інший пристрій — Linux, див. §6.
> LAN-тест на 2 пристроях: `docs/operations/network-two-devices.md`.
> Розгортання на ZBOX: `docs/operations/setup-two-devices.md`.

---

## ЧАСТИНА 1. Отримати Windows-інсталятор (на цьому ПК)

### Крок 1.1 — Переконатись, що гілка запушена
```bash
git ls-remote --heads origin feat/pg-replication
# → d77d61351769065351df16745ebd47980b1a8a6b  (збігається з локальним HEAD)
```
Гілка вже запушена. Якщо sha не збігається — `git push origin feat/pg-replication`.

### Крок 1.2 — Запустити Windows-збірку в GitHub Actions (ручний запуск)
> Автоматично workflow НЕ запуститься: гілка `feat/pg-replication` не входить
> у тригерні branches (main, feat/rust-migration) — лише ручний dispatch
> або тег `v*.*.*`.

1. Відкрити репо на GitHub: `moroznastya/KASA`
2. Вкладка **Actions** → ліворуч **«🪟 Windows Build (NSIS + MSI)»**
3. Праворуч кнопка **«Run workflow»** → у випадаючому списку **Branch** обрати
   **`feat/pg-replication`** → зелена кнопка **Run workflow**
4. Дочекатись завершення (~15–25 хв: windows-latest ставить Node/Rust,
   збирає Rust-core + Tauri, робить NSIS + MSI + updater-артефакти)
5. У вкладці workflow-рану (список кроків) внизу — секція **Artifacts**:
   - `torgashka_*.msi` (WiX, рекомендується для встановлення)
   - `torgashka_*.exe` (NSIS-інсталятор, альтернатива)
   - `torgashka_*.sig` + `latest.json` (для автоапдейту)
6. Завантажити **`.msi`** (або `.exe`) на цей ПК, покласти на USB-флешку.

> Альтернатива для автозапуску при релізі (не обов'язково):
> `git tag v1.0.1 && git push origin v1.0.1` — тег збере release-артефакти
> у GitHub Release з `latest.json` (автоапдейт кас).

---

## ЧАСТИНА 2. Встановлення на цільовому пристрої (Windows)

### Крок 2.1 — Налаштування Windows (один раз)
- Windows 10/11 64-bit, актуальні оновлення.
- **WebView2 Runtime** — вбудований у Win11; для Win10 — за потреби
  `https://developer.microsoft.com/microsoft-edge/webview2/` (per-machine).
- Для ZBOX (сервер 24/7): BIOS → `Power On AC Restore` = On;
  Windows: автологін (netplwiz), вимкнути sleep/hibernate,
  живлення через ДБЖ.

### Крок 2.2 — Встановити Torgashka
1. Вставити USB → запустити `torgashka_*.msi` → Install (шлях за замовчуванням:
   `C:\Program Files\Torgashka\`)
2. Запустити **Torgashka** (ярлик на робочому столі / зі Старт-меню).
3. Перший запуск: пройти створення власника (owner) і магазину (store).
   Це звичайна локальна каса (embedded PostgreSQL на `127.0.0.1:5433`).

---

## ЧАСТИНА 3. Тестування на іншому пристрої (варіанти)

### Варіант A — просто перевірити, що працює (один пристрій, без мережі)
1. Відкрити каталог → додати товар → продати чек.
2. Перевірити, що дані збереглись: перезапустити Torgashka → товар і чеки на місці.
3. Адмінка (власник): розділи «Мережа», «Налаштування» відкриваються без помилок.
   → Версія актуальна, якщо є сторінки «Мережа»/«Топологія вузлів»
   (ознака збірки з гілки feat/pg-replication).

### Варіант B — LAN-тест з 2 пристроїв (PG-реплікація) — повний сценарій
Виконувати за чек-листом: `docs/operations/network-two-devices.md` §3.
Коротко:
1. Обидва пристрої в одній мережі. **Пристрій A (primary)**: налаштувати
   Windows Firewall (дозволити 8000/5433 для IP пристрою B) + env:
   ```powershell
   setx TORGASHKA_LISTEN_ADDR "0.0.0.0:8000"
   setx TORGASHKA_PG_LISTEN_ADDRESSES "<IP-пристрою-B>"
   ```
   Перезапустити Torgashka (env читаються при старті).
2. На A: Адмінка → **Мережа** → «Додати вузол» → отримати **join-код** (TTL 30 хв).
3. На **пристрої B (standby/каса)**: перший запуск → join-екран →
   **Server URL** = `http://<IP-A>:8000` + join-код → «Приєднати».
4. Автоматично йде pg_basebackup з A на B (прогрес) → «Перезапустити зараз».
5. B після рестарту — standby (репліка `127.0.0.1:5433`, heartbeat 60 с).
   На A в списку вузлів B: `active`.

**Що перевірити (критерії):**
- [ ] B: `curl http://<IP-A>:8000/api/v1/health` → 200
- [ ] B у статусі `active` в адмінці A
- [ ] **Повна копія + WAL:** на A додати товар/провести чек → на B з'являється
      саме за секунди (без ручних дій)
- [ ] **Read-only standby:** прямий запис у БД B (5433) → помилка
      `read-only transaction` (очікувано)
- [ ] **Розрив:** вимкнути мережу на B → каса читає каталог з локальної репліки,
      чеки йдуть у SQLite-чергу (`queue_pending` > 0), дані не губляться
- [ ] **Promote:** вимкнути A повністю → на B: «Мережа» → «Зробити цим
      комп'ютером головний» → B пише напряму; увімкнути A → A НЕ стає другим
      primary автоматично (захист від split-brain; повернення A — вручну
      як новий standby через join)

### Варіант C — WAN через Tailscale (ZBOX як primary)
Виконувати за `docs/operations/setup-two-devices.md` §3–§4:
1. Tailscale на ZBOX і касах (один tailnet).
2. Env на ZBOX: `TORGASHKA_LISTEN_ADDR=0.0.0.0:8000`,
   `TORGASHKA_PG_LISTEN_ADDRESSES=<tailnet-підмережа>`, firewall лише з VPN.
3. Server URL кас = `http://<zbox-tailscale-ip>:8000`; join → провіжн →
   рестарт → `active`.

---

## ЧАСТИНА 4. Як оновити (новий інсталятор пізніше)
- Автоапдейт: встановлена версія перевіряє `latest.json` з GitHub Release
  (потрібен тег `v*.*.*` на GitHub).
- Вручну: повторити Частину 1 (Run workflow на новій гілці) → встановити `.msi`
  поверх (Torgashka зберігає дані; після апдейту standby наздоганяє
  реплікацію автоматично).

---

## §6. Якщо інший пристрій — Linux (цей ПК, Ubuntu)
Поточна локальна збірка (виконана на цьому ПК) дає:
```
frontend/src-tauri/target/release/bundle/deb/torgashka_*.deb
frontend/src-tauri/target/release/bundle/appimage/torgashka_*.AppImage
```
Встановлення:
```bash
sudo apt install -y ./torgashka_*.deb        # або
chmod +x torgashka_*.AppImage && ./torgashka_*.AppImage
```

---

## Часті помилки (швидко)
| Симптом | Рішення |
|---|---|
| У Actions немає workflow | Гілка має бути запушена (Крок 1.1), запуск — Run workflow → feat/pg-replication |
| `.msi` не ставиться | Win10 без WebView2 → встановити WebView2 per-machine |
| B не бачить A (timeout) | Firewall A: 8000/5433; env задано ДО запуску Torgashka; IP збігаються |
| join-код не приймається | TTL 30 хв минув → створити новий |
| pg_basebackup падає | `TORGASHKA_PG_LISTEN_ADDRESSES` на A не включає IP B |
| Немає «Мережа» в меню | Встановлено стару збірку (не feat/pg-replication) |
