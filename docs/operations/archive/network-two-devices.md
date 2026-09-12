> [!WARNING]
> **⛔ ЗАСТАРІЛО — АРХІВ (2026-09-12).** Документ описує модель **read-only вузла /
> фізичної реплікації PostgreSQL** (`primary` → hot-standby, `pg_basebackup`,
> WAL-стрімінг, promote). Цю модель **демонтовано** рішенням Творця,
> зафіксованим у [ADR-0008 «Рівноправні read-write вузли + центральний хаб»](../../adr/ADR-0008-peer-nodes-sync-hub.md)
> і виконаним кроком **E7** плану [`plan-adr0008-peer-nodes.md`](../../architecture/plan-adr0008-peer-nodes.md)
> (коміт `50eb8ec`).
>
> **Актуальна модель: [docs/operations/hub-and-nodes.md](../hub-and-nodes.md).**
>
> Збережено як **історичний запис** (рішення, процедури, факти реальних прогонів) — не видаляти.
> **НЕ керуватися цим документом.** Згадані тут шляхи та роути у коді БІЛЬШЕ НЕ ІСНУЮТЬ:
> `/api/v1/network-nodes/join`, `/api/v1/network-nodes/:id/heartbeat`,
> `/api/v1/admin/network-nodes/:id/force-resync`, `/api/v1/local/promote`,
> `/api/v1/local/repoint-primary`, локальна embedded-PG репліка на порту 5433,
> `pg_basebackup`-провіжн, ґейт запису `write_gate`/`readonly_net`.

---
# Мережа на 2 окремі пристрої — LAN-тест (PG-реплікація)

> Статус: код готовий (етапи 15–18: join/heartbeat, pg_basebackup-провіжн,
> hot_standby на 5433, локальний API, promote/repoint, SQLite-черга).
> Цей документ — процедура **першого реального прогону на 2 пристроях**.
> Еталон деталей: `docs/system_documentation/sources/network-replication-etap15-20.md`.
> DR/promote: `docs/operations/disaster-recovery-network.md`.

## 0. Архітектура і ролі

```
[Пристрій A = PRIMARY]  ──LAN──▶  [Пристрій B = STANDBY (каса)]
  • embedded PG :5433              • embedded PG :5433 (replica, hot standby)
  • фасад :8000 (join/heartbeat)   • фасад :8000 (локальний API)
  • єдиний автор запису            • read-only копія + WAL streaming
```

- **Запис у БД — лише через primary.** Standby — точна копія (pg_basebackup +
  стрімінг WAL), read-only.
- **SQLite offline-черга** — окремий шар на вузлі: якщо зв'язок з primary
  втрачено, каса далі читає каталог/залишки з локальної репліки (5433),
  а чеки пише в SQLite-чергу (`queue_pending` > 0, `effective: offline`).
  Після відновлення зв'язку або promote черга не губиться.
- Обидва пристрої — **Windows-збірки** (десктоп). Primary може стояти на
  Zotac ZBOX Windows (завжди ввімкнений).

## 1. Збірка інсталяторів (Windows)

CI готовий: `.github/workflows/windows-build.yml` (windows-latest →
NSIS `.exe` + WiX `.msi` + `latest.json` для автоапдейту).

### 1.1 Через GitHub Actions (рекомендовано)
1. Закомітити поточний стан (зміни реплікаційного треку) у гілку.
2. Запуск вручну: Actions → «🪟 Windows Build» → Run workflow (гілка main);
   або автоматично: `git tag v1.0.1 && git push origin v1.0.1`.
3. З GitHub Release забрати `.msi` (або NSIS `.exe`) → на USB → на пристрої.

### 1.2 Локально на Windows-машині
```powershell
# Потрібно: Node 24, Rust MSVC (x86_64-pc-windows-msvc), WebView2 (є в Win10/11)
cd frontend
npm ci
npm run tauri:build
# → frontend/src-tauri/target/release/bundle/msi/*.msi
# → frontend/src-tauri/target/release/bundle/nsis/*.exe
```

> З Linux-машини Windows-інсталятор не збирається (MSVC/WebView2) —
> тільки CI або Windows-хост.

## 2. Сценарій A — одна локальна мережа (обов'язковий тест перед WAN)

### A1. IP і firewall
- Обидва пристрої в одній підмережі (DHCP-reservation у роутері):
  A = `192.168.1.50`, B = `192.168.1.60`.
- Windows Firewall на **A** (PowerShell від адміністратора):
```powershell
New-NetFirewallRule -DisplayName "Torgashka-facade" -Direction Inbound -LocalPort 8000 -Protocol TCP -Action Allow -RemoteAddress 192.168.1.60
New-NetFirewallRule -DisplayName "Torgashka-pg"     -Direction Inbound -LocalPort 5433 -Protocol TCP -Action Allow -RemoteAddress 192.168.1.60
```

### A2. Env PRIMARY (пристрій A)
```powershell
setx TORGASHKA_LISTEN_ADDR "0.0.0.0:8000"        # фасад для join/heartbeat з B
setx TORGASHKA_PG_LISTEN_ADDRESSES "192.168.1.60" # PG приймає pg_basebackup/WAL
# (після setx — перезапустити Torgashka)
```
- Дефолт без env: фасад `127.0.0.1:8000`, PG `127.0.0.1:5433` — **недостатньо**
  для LAN: B не достукається.
- pg_hba для реплікації (`host replication <role> <LAN> scram-sha-256`)
  налаштовується/перевіряється на A (авто-додавання при join або через
  HBA_EXTRA — див. етап 15–20, §провіжн).

### A3. Перевірка на A, що слухає мережу
```powershell
netstat -ano | findstr ":8000 :5433"   # очікуємо 0.0.0.0:8000 і 0.0.0.0:5433
```

### A4. Приєднати STANDBY (пристрій B)
1. На A: адмінка → **Мережа** → «Додати вузол» → join-код (TTL 30 хв).
2. На B: перший запуск → join-екран:
   - **Server URL** = `http://192.168.1.50:8000`
   - join-код, назва вузла.
3. Після join автоматично: pg_basebackup з A → прогрес → успіх →
   «Перезапустити зараз».
4. Після рестарту B — standby: локальна репліка на `127.0.0.1:5433`,
   heartbeat кожні 60 с на A, в адмінці A статус вузла `active`.

## 3. Критерії прийняття LAN-тесту (ПІД РЕПЛІКАЦІЮ)

### 3.1 З'єднання і реєстрація
- [ ] B `curl http://192.168.1.50:8000/api/v1/health` → 200 (з B)
- [ ] join успішний; на A в списку вузлів B: `active` (heartbeat доходить)
- [ ] B: `/api/v1/local/status` → `mode: standby`, `primary_reachable: true`

### 3.2 Повна копія (basebackup + WAL catch-up)
- [ ] На B: `SELECT pg_is_in_recovery();` → `true` (standby-режим)
- [ ] Розмір/вміст БД збігається: кількість рядків каталогу, залишків,
      контрагентів на B == на A (після провіжна — звірити кілька ключових
      таблиць, напр. `products`, `stock`, `stores`).
- [ ] **WAL catch-up (live):** на A додати товар / провести чек →
      на B рядок з'являється **сам**, без ручних дій, протягом секунд.
- [ ] Лаг у нормі: на A `SELECT replay_lag FROM pg_stat_replication;`
      → 0 або < 2 с; при просторію — лаг зростає, але наздоганяється
      (standby «наздоганяє повну копію»).
- [ ] Standby read-only: прямий запис у БД B (`INSERT` на 5433) → помилка
      `cannot execute INSERT in a read-only transaction` (очікувано).

### 3.3 SQLite offline-черга (окремий шар, на розриві)
- [ ] Вимкнути мережу на B (або firewall на A) → B: `effective: offline`,
      каталог/залишки **читаються** з локальної репліки.
- [ ] Пробити чек на B під час розриву → чек збережено в SQLite-черзі
      (`queue_pending` зростає), дані не втрачені, касир працює далі.
- [ ] Увімкнути мережу → зв'язок відновлено; черга не зникає/не губиться
      (розгрібання черги — за процедурою етапу: push після promote або
      реплікація знову актуальна).

### 3.4 Promote на відключеному primary (DR)
- [ ] Повністю вимкнути A (primary).
- [ ] На B (тепер офлайн, але з повною копією): JWT owner →
      `POST /api/v1/local/promote` → B стає **новим primary**:
      `mode: primary`, пише напряму, локальна БД 5433 відкрита на запис.
- [ ] На B після promote: каталог/чеки доступні, нові чеки проходять у БД
      (не лише в чергу).
- [ ] **Захист від split-brain:** увімкнути A знову → A **НЕ** підключається
      автоматично як primary (не створює двох авторів).
- [ ] Повернення старого primary у мережу — вручну як **новий standby**
      (join-код/repoint; процедура — `disaster-recovery-network.md`).

## 4. Часті помилки

| Симптом | Причина → рішення |
|---|---|
| `curl` з B timeout | Firewall A не пускає 8000; фасад на 127.0.0.1 (env не задано/не перезапущено) |
| Join ок, pg_basebackup падає | HBA на A без `host replication` для IP B; пароль ролі реплікації |
| Heartbeat є, реплікація не йде | `TORGASHKA_PG_LISTEN_ADDRESSES` не включає B; PG слухає лише 127.0.0.1 |
| Лаг зростає постійно | Слабка Wi-Fi/навантаження; перевірити `replay_lag`; у нормі — наздоганяє |
| Join-код не приймається | TTL 30 хв минув → створити новий |
| Помилка TLS | У LAN без TLS: Server URL = `http://`, не `https://` |
| Promote на живому primary | Заборонено: спершу переконатись `primary_reachable: false` |

Логи: `torgashka.log` поряд з data_dir; події мережі —
`GET /api/v1/admin/network-events`.

## 5. Чек-лист «2 пристрої працюють»
- [ ] B бачить A по HTTP (3.1)
- [ ] Вузол B `active` в адмінці A
- [ ] Зміна на A сама з'являється на B (3.2: live WAL)
- [ ] Standby read-only (3.2)
- [ ] Розрив: локальна робота + SQLite-черга без втрат (3.3)
- [ ] Promote на вимкненому A працює; старий A не стає другим primary (3.4)
- [ ] Після тесту — відновити конфігурацію (або лишити B primary і
      ре-приєднати A як standby через join)
