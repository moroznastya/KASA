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
# Розгортання мережі: Zotac ZBOX Windows (primary) + каси (standby)

> Архітектура: **PG-реплікація** (primary — єдиний автор; standby — повна
> копія hot standby + WAL streaming; SQLite offline-черга — окремий шар на
> розрив). Рішення Творця: сервер глобальної БД — **Zotac ZBOX під Windows**,
> інсталятори — **Windows** (NSIS/MSI).
> LAN-тест на 2 пристроях: `docs/operations/network-two-devices.md`.
> DR/promote: `docs/operations/disaster-recovery-network.md`.

## 1. Цільова топологія (WAN)

```
[Каса Windows: standby]  ──Tailscale (VPN)──▶  [Zotac ZBOX: Windows primary]
   embedded PG :5433 (replica)                    embedded PG :5433
   фасад :8000                                    фасад :8000 (0.0.0.0)
   SQLite offline-черга (шар)                     єдиний автор запису
```

- Каси ходять до ZBOX через **VPN (Tailscale/ZeroTier)** — PG і фасад НЕ
  виставляються в публічний інтернет (принцип з `deploy/network/README.md`,
  адаптований під Windows).
- Один комп'ютер магазину = один вузол-standby; інші каси магазину — тонкі
  клієнти до локального фасаду цього вузла (по LAN), а не через WAN.

## 2. Zotac ZBOX — вимоги

- ZBOX з Intel N100/N305 (або новіший), 16+ GB RAM, 512 GB+ SSD (NVMe).
- Windows 11 Pro (Pro — для Group Policy/автологіну; Home теж підійде).
- Завжди ввімкнений: у BIOS — `Power On AC Restore`, автологін +
  автозапуск Torgashka (Startup), відключити sleep.
- Живлення через ДБЖ (UPS) — щоб переживав короткі збої.
- Резервування: щоденний бекап БД (розділ 6) + можливість promote standby
  на іншій машині.

## 3. Встановлення ZBOX (primary)

### 3.1 Збірка Windows-інсталятора
CI: `.github/workflows/windows-build.yml` →
1. Закомітити зміни (реплікаційний трек) у гілку.
2. Actions → «🪟 Windows Build» → Run workflow **або**
   `git tag v1.0.1 && git push origin v1.0.1` → GitHub Release:
   `.msi` (WiX) + `.exe` (NSIS) + `latest.json` (автоапдейт).
3. На ZBOX і на кожну касу встановити `.msi`.

Локальна збірка на Windows: `cd frontend && npm ci && npm run tauri:build`
→ `frontend/src-tauri/target/release/bundle/`.

### 3.2 Env-змінні на ZBOX (системні, PowerShell адмін)
```powershell
# Фасад слухає всі інтерфейси (Tailscale-IP та LAN) — join/heartbeat з кас
setx TORGASHKA_LISTEN_ADDR "0.0.0.0:8000"

# PG приймає реплікацію лише з Tailscale-підмережі кас (100.x.y.z)
setx TORGASHKA_PG_LISTEN_ADDRESSES "100.0.0.0/8"
# (для LAN-тесту: конкретна IP каси, напр. "192.168.1.60")

# HBA: hostssl для реплікації/додатку поза localhost — за потреби
setx TORGASHKA_PG_HBA_EXTRA "hostssl replication all 100.0.0.0/8 scram-sha-256"
```
Після `setx` — перезапуск Torgashka (env читаються при старті).

> Безпека: якщо ZBOX дивиться в інтернет напряму (не лише Tailscale) —
> Windows Firewall: дозволити 8000/5433 **тільки** з Tailscale-підмережі
> кас; решту — deny. Найкраще — взагалі не відкривати порти назовні,
> а ходити виключно через Tailscale.

### 3.3 Tailscale на ZBOX і касах
1. Встановити Tailscale (tailscale.com/download/windows) на ZBOX і каси.
2. `tailscale up` на всіх (один tailnet, MagicDNS).
3. Перевірка: з каси `ping <zbox-tailscale-ip>`; ZBOX має стабільну
   Tailscale-IP (краще — `tailscale set --nickname` + резервування IP).
4. Server URL для кас = `http://<zbox-tailscale-ip>:8000` (або MagicDNS-ім'я).

## 4. Первинне розгортання (перший запуск)

1. На ZBOX запустити Torgashka → створити власника (owner) → налаштувати
   магазин (store) як звичайно.
2. Адмінка ZBOX → **Мережа** → «Додати вузол» → join-код (TTL 30 хв).
3. На касі: перший запуск → join-екран → Server URL `http://<zbox>:8000`,
   join-код, назва вузла.
4. Автоматично: pg_basebackup з ZBOX на касу (прогрес на екрані) →
   «Перезапустити зараз».
5. Каса після рестарту: standby (репліка 5433, heartbeat 60 с). У адмінці
   ZBOX вузол `active`.

Повтор для кожної каси: кроки 2–5 (кожна каса — окремий вузол-standby).

## 5. Щоденна робота і відмови

- **Норма:** heartbeat 60 с; в адмінці ZBOX усі вузли `active`; лаг ~0.
- **Інтернет/ZBOX недоступний з каси:** каса лишається живою: читає
  каталог/залишки з локальної репліки (5433), чеки → SQLite-черга
  (`effective: offline`, `queue_pending` > 0). Дані не губляться.
- **ZBOX вийшов з ладу надовго → promote каси:** на касі
  `POST /api/v1/local/promote` (JWT owner, працює офлайн) → каса стає новим
  primary. Детально: `disaster-recovery-network.md`.
- **Повернення старого ZBOX:** НЕ підключається автоматично (захист від
  split-brain). Вручну — як новий standby через join/repoint
  (див. disaster-recovery-network.md §3–§5).
- **Каса офлайн довше WAL retention:** сервер віддає 410 `force-resync
  required` → кнопка force-resync в адмінці або повний повторний join.

## 6. Бекапи (ZBOX, primary)

Щоденно (Task Scheduler):
```powershell
# pg_dump стиснутий (файл даних + схема)
& "C:\Program Files\Torgashka\pg\bin\pg_dump.exe" -h 127.0.0.1 -p 5433 -U torgashka_app -d torgashka -Fc -f "D:\backup\torgashka_$(Get-Date -Format yyyyMMdd).dump"
# + WAL-архівація (якщо retention критичний) — або покладатись на standby-копії
```
- Зберігати на іншому диску/пристрої (зовнішній, хмара).
- Додатковий рівень резервування — самі standby-вузли (повні копії даних).
- Регулярно тестувати відновлення з бекапу.

## 7. Оновлення версій (Windows)

- Tauri-збірка підтримує автоапдейт (`latest.json` з GitHub Release):
  нова версія підхоплюється касами автоматично (або вручну — інсталятор).
- Порядок: спершу оновити ZBOX (primary), потім каси (standby) — standby
  наздоганяє після рестарту через pg_basebackup/реплікацію.

## 8. Чек-лист продакшн-розгортання
- [ ] Windows-інсталятор зібрано (CI/локально) і стоїть на ZBOX + касах
- [ ] ZBOX: env задано, фасад слухає 0.0.0.0:8000, PG — Tailscale-підмережу
- [ ] Tailscale: ZBOX і каси в одному tailnet, ping проходить
- [ ] Windows Firewall на ZBOX: 8000/5433 лише з Tailscale-підмережі
- [ ] Каса: join → провіжн → рестарт → `active` в адмінці ZBOX
- [ ] LAN/WAN-критерії реплікації пройдено (network-two-devices.md §3)
- [ ] Promote-сценарій відпрацьовано (на тестовій касі)
- [ ] Бекапи налаштовано і протестовано відновлення
