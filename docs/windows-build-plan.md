# План збірки Windows-інсталяторів Torgashka (.exe NSIS, .msi WiX)

> Версія: 1.0.0
> Дата: 2026-08-21
> Статус: план до виконання
> Автор: PM_Agent (NIKO)

## 0. Фактична база (перевірено 2026-08-21)

| Параметр | Значення |
|---|---|
| Корінь проєкту | `/home/anastasia/Andriy/aegis_v3/Niko/Projects/kasa` |
| Frontend | `frontend/` — React 18 + Vite + TypeScript; `npm run build` = `tsc && vite build`; збірка → `frontend/dist/` |
| Tauri shell | `frontend/src-tauri/` — Tauri v2 (Cargo.toml: `tauri = "2"`), workspace `crates/*` |
| Конфіг Tauri | `frontend/src-tauri/tauri.conf.json` |
| Іконки | `frontend/src-tauri/icons/` — наявні: `icon.ico`, `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.png`, `Square30x30Logo.png`, `StoreLogo.png` |
| Backend | `backend/` — Python + FastAPI, API на `127.0.0.1:8000`; запускається разом із GUI як embedded process |
| Git remote | `git@github-my:moroznastya/KASA.git` (⚠️ НЕ збігається з updater endpoints) |
| Updater endpoints | `https://github.com/torgashka/torgashka/releases/latest/download/latest.json` (⚠️ аномалія, див. §6.1) |
| Наявні CI | `.github/workflows/ci.yml` (lint+test, ubuntu-latest), `.github/workflows/rust-core.yml` (fmt→clippy→test, working-dir `frontend/src-tauri`) |
| Локальна машина | Linux (rustc 1.97.1, target лише `x86_64-unknown-linux-gnu`; Windows target НЕ встановлено) |

---

## 1. Огляд варіантів збірки

### 1a. GitHub Actions, runner `windows-latest` — ⭐ РЕКОМЕНДОВАНИЙ

**Як працює:** окремий workflow запускається на віртуальній Windows-машині GitHub; там встановлюються Node, Rust (MSVC toolchain), збирається frontend і Tauri-бінарник, пакуються NSIS/MSI, артефакти завантажуються в GitHub Release.

| Переваги | Недоліки |
|---|---|
| Відтворюваність: кожна збірка = чиста Windows-машина | Ліміти хвилин GitHub Actions (безкоштовно ~2000 хв/міс) |
| Нульова підготовка локальної Windows | Артефакти не можна підписати власним HSM/токеном без налаштування секретів |
| Правильний MSVC toolchain + WebView2-оточення «з коробки» | Приватні репозиторії: діє ліміт хвилин |
| Інтеграція з GitHub Release та updater (той самий хост) | — |
| Не залежить від локальної машини розробника | — |

### 1b. Нативна Windows-машина (локальна або VM)

**Як працює:** на Windows-машині ставляться Node.js LTS, Rust (rustup + MSVC toolchain), Git; збірка запускається вручну або скриптом.

| Переваги | Недоліки |
|---|---|
| Повний контроль над середовищем | Ручна підготовка: Node, Rust MSVC, WebView2 SDK, WiX toolset |
| Можна підписувати код локальним сертифікатом | Невідтворюваність: стан машини змінюється з часом |
| Швидка ітерація під час розробки | «А в мене працює» — класична проблема |
| Зручно для фінального ручного тестування | Потрібна ліцензія Windows + апаратні ресурси |

### 1c. Cross-compile з Linux (cargo-xwin / NSIS)

**Як працює:** на Linux встановлюється `cargo-xwin` (обгортка над MSVC linker через xwin SDK) або mingw-w64; Tauri може збирати Windows-бінарники за допомогою `cargo tauri build --target x86_64-pc-windows-msvc` (через xwin) або `x86_64-pc-windows-gnu` (mingw).

| Переваги | Недоліки |
|---|---|
| Немає потреби у Windows-машині | Високий ризик зламатися: native-залежності Rust (wry, webview2-com) чутливі до лінкера |
| Можна тримати всю збірку в Linux-CI | NSIS можна зібрати (makensis через wine або nsis на linux), але WiX `.msi` cross-compile — практично неможливий (потрібен Windows) |
| — | WebView2 bootstrapper та код-сігнінг однаково потребують Windows-кроків |
| — | Підтримка tauri-cli для cross-збірки MSI відсутня |

### Висновок і рекомендація

**Використовувати GitHub Actions з `windows-latest`.** Причини:
1. Єдиний варіант, що дає **обидва** інсталятори (NSIS + MSI/WiX) у відтворюваному середовищі.
2. Updater працює через GitHub Release — той самий хост, що й збірка, мінімум ручної роботи.
3. Не потребує жодних змін на Linux-машині розробника.

Нативну Windows-машину тримати **як резервний/тестовий стенд** (для фінальної ручної верифікації, §5). Cross-compile з Linux — лише для експериментів, НЕ для релізів.

---

## 2. Покроковий план: GitHub Actions (`windows-latest`)

### 2.1. Цільовий workflow — `.github/workflows/windows-build.yml`

```yaml
# ============================================================================
# Torgashka — Windows release build (NSIS .exe + WiX .msi + updater artifacts)
# Стратегія: docs/windows-build-plan.md §2
# Тригер: ручний (workflow_dispatch) або tag v*.*.*
# ============================================================================
name: 🪟 Windows Build (NSIS + MSI)

on:
  workflow_dispatch:        # ручний запуск
  push:
    tags: ['v*.*.*']        # автоматично при пуші тегу релізу

permissions:
  contents: write           # потрібно для upload до GitHub Release

jobs:
  build-windows:
    name: Build NSIS + MSI
    runs-on: windows-latest
    defaults:
      run:
        working-directory: frontend          # всі npm/tauri команди — з frontend/

    steps:
      - name: 📥 Checkout
        uses: actions/checkout@v4

      - name: 🔧 Setup Node.js 20
        uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: npm
          cache-dependency-path: frontend/package-lock.json

      - name: 🦀 Setup Rust (MSVC toolchain)
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc

      - name: 💾 Cache cargo
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: frontend/src-tauri

      - name: 📦 npm ci
        run: npm ci

      - name: 🧱 Build frontend
        run: npm run build

      - name: 🏗️ Tauri build (NSIS + MSI + updater)
        run: npm run tauri:build -- --bundles nsis,msi
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}

      - name: 📤 Upload artifacts (installers + updater)
        uses: actions/upload-artifact@v4
        with:
          name: torgashka-windows
          path: |
            frontend/src-tauri/target/release/bundle/nsis/*.exe
            frontend/src-tauri/target/release/bundle/msi/*.msi
            frontend/src-tauri/target/release/bundle/nsis/*.sig
            frontend/src-tauri/target/release/bundle/msi/*.sig
            frontend/src-tauri/target/release/bundle/nsis/latest.json
            frontend/src-tauri/target/release/bundle/msi/latest.json
          if-no-files-found: error

      # ── Реліз: створюється тільки при push тегу v*.*.* ─────────────────
      - name: 🚀 Publish GitHub Release
        if: startsWith(github.ref, 'refs/tags/v')
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          files: |
            frontend/src-tauri/target/release/bundle/nsis/*.exe
            frontend/src-tauri/target/release/bundle/msi/*.msi
            frontend/src-tauri/target/release/bundle/nsis/*.sig
            frontend/src-tauri/target/release/bundle/msi/*.sig
            frontend/src-tauri/target/release/bundle/nsis/latest.json
            frontend/src-tauri/target/release/bundle/msi/latest.json
```

### 2.2. Пояснення ключових кроків

| Крок | Навіщо | Деталі |
|---|---|---|
| `setup-node@v4` + `cache: npm` | Відтворювані залежності frontend | Node 24 LTS; кеш пришвидшує збірку |
| `dtolnay/rust-toolchain@stable` + `targets: x86_64-pc-windows-msvc` | MSVC toolchain — єдиний офіційно підтримуваний Tauri на Windows | GNU (mingw) не рекомендований для wry/webview2 |
| `npm run tauri:build -- --bundles nsis,msi` | Збирає обидва інсталятори + updater-артефакти | Покладається на `beforeBuildCommand: npm run build` з tauri.conf.json; явний `npm run build` вище — для швидкого фідбеку при помилці |
| `TAURI_SIGNING_PRIVATE_KEY` secrets | Підпис updater-артефактів (latest.json + .sig) | Обов'язково: без ключа `createUpdaterArtifacts` не створить .sig |
| `softprops/action-gh-release@v2` | Завантажує інсталятори в GitHub Release | Спрацьовує тільки на тегах `v*` |

### 2.3. Налаштування GitHub secrets (один раз)

1. Згенерувати ключ підпису updater (локально, на Linux):
   ```bash
   cd /home/anastasia/Andriy/aegis_v3/Niko/Projects/kasa/frontend
   npx tauri signer generate -w ~/.tauri/torgashka.key
   # Виведе публічний ключ — він має ЗБІГАТИСЯ з plugins.updater.pubkey у tauri.conf.json
   ```
2. У GitHub: `Settings → Secrets and variables → Actions`:
   - `TAURI_SIGNING_PRIVATE_KEY` — вміст приватного ключа (base64 рядок з файлу)
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — пароль від ключа

> ⚠️ Якщо згенерувати НОВИЙ ключ — оновити `plugins.updater.pubkey` у `tauri.conf.json`, інакше клієнт відхилить оновлення.

### 2.4. Запуск збірки

```bash
# Ручний запуск: GitHub → Actions → "🪟 Windows Build (NSIS + MSI)" → Run workflow
# Або автоматично:
cd /home/anastasia/Andriy/aegis_v3/Niko/Projects/kasa
git tag v1.0.0
git push origin v1.0.0
```

### 2.5. Перевірка updater endpoints (критично перед релізом)

Після першого релізу переконатися, що:
```
https://github.com/<ВЛАСНИК>/<РЕПО>/releases/latest/download/latest.json
```
реально віддає JSON. `<ВЛАСНИК>/<РЕПО>` має збігатися з `plugins.updater.endpoints` у `tauri.conf.json` **і** з фактичним git remote. Зараз remote = `moroznastya/KASA`, а endpoint = `torgashka/torgashka` — див. §6.1.

---

## 3. Конфігурація `tauri.conf.json` для Windows

Файл: `frontend/src-tauri/tauri.conf.json`

### 3.1. Що вже налаштовано правильно

```jsonc
"bundle": {
  "active": true,
  "targets": "all",                       // = nsis + msi + app (на Windows)
  "createUpdaterArtifacts": true,         // генерує latest.json + .sig
  "windows": {
    "nsis": {
      "languages": ["Ukrainian", "English"],  // ✅ вже є
      "displayLanguageSelector": true         // ✅ вже є
    }
  }
}
```

### 3.2. Що змінити — NSIS (`.exe`)

```jsonc
"windows": {
  "nsis": {
    "languages": ["Ukrainian", "English"],
    "displayLanguageSelector": true,
    "installerIcon": "icons/icon.ico",        // ➕ іконка інсталятора (вже є файл)
    "headerImage": "icons/header.bmp",        // ➕ банер 150x57 px BMP (ФАЙЛУ НЕМАЄ — створити, див. нижче)
    "installMode": "currentUser",             // ➕ per-user: без UAC, без адмін-прав (POS-каса)
    "perMachine": false                        // ➕ явно: НЕ machine-wide
  }
}
```

**Створити `header.bmp`** (ImageMagick, на Linux):
```bash
cd /home/anastasia/Andriy/aegis_v3/Niko/Projects/kasa/frontend/src-tauri
convert icons/128x128.png -resize 150x57 -background '#1a1a2e' -gravity center -extent 150x57 BMP3:icons/header.bmp
file icons/header.bmp   # перевірка: має бути "PC bitmap, 150 x 57"
```
> Вимоги NSIS: BMP 150×57 px (24-біт). PNG НЕ підійде.

**Чому `currentUser`:** касовий застосунок не потребує прав адміністратора; per-user установка працює без UAC-промпту, оновлення updater'ом відбувається без підвищення прав. Якщо колись знадобиться встановлення «для всіх користувачів» — перемкнути на `perMachine: true` + `installMode: "perMachine"`.

### 3.3. Що додати — WiX (`.msi`)

```jsonc
"windows": {
  "wix": {
    "language": ["uk-UA", "en-US"],           // ➕ мови MSI (WiX language)
    "template": "wix/main.wxs"                // ➕ опційно: кастомний WiX-шаблон
  }
}
```

- `language: ["uk-UA", "en-US"]` — локалізація стандартних діалогів MSI.
- Кастомний `template` потрібен лише якщо знадобляться додаткові MSI-дії (наприклад, створення ярлика, реєстрація протоколу, запис у реєстр). **На перший реліз — НЕ додавати**, щоб не ускладнювати збірку. Створити `frontend/src-tauri/wix/main.wxs` пізніше, якщо з'явиться вимога.

### 3.4. Що виправити — updater endpoints

```jsonc
"plugins": {
  "updater": {
    "pubkey": "<згенерований публічний ключ>",
    "endpoints": [
      "https://github.com/<ВЛАСНИК>/<РЕПО>/releases/latest/download/latest.json"
      // ⚠️ ЗАРАЗ: torgashka/torgashka — не збігається з remote moroznastya/KASA
    ]
  }
}
```
Рішення аномалії — §6.1.

### 3.5. Дрібна аномалія

`"$schema"` вказує на nicegui-схему (не tauri). На збірку не впливає, але редактор може не давати підказок. Виправити на:
```jsonc
"$schema": "https://schema.tauri.app/config/2"
```

---

## 4. Особливості Windows для Torgashka

### 4.1. WebView2 runtime

- Tauri v2 інсталятори (NSIS/MSI) **вбудовують WebView2 bootstrapper** за замовчуванням: якщо WebView2 відсутній — інсталятор завантажить і встановить його автоматично.
- На Windows 10/11 (оновлених) WebView2 вже є — bootstrapper нічого не робить.
- **Перевірка на тестовій машині:** видалити WebView2 (або використати чисту VM) → встановити Torgashka → переконатися, що bootstrapper спрацював. Якщо потрібно жорстко контролювати поведінку — конфіг `app.windows.webviewInstallMode` (у v2: `bundle.windows.webviewInstallMode`), значення: `downloadBootstrapper` (за замовчуванням), `offlineInstaller`, `skip`.

### 4.2. Firewall і порт 127.0.0.1:8000

- Backend слухає **тільки loopback** (`127.0.0.1:8000`) — Windows Firewall **НЕ блокує loopback-трафік**, окреме firewall-правило **НЕ потрібне**.
- ⚠️ Якщо хтось колись змінить біндинг на `0.0.0.0` (доступ з мережі) — тоді Windows Firewall покаже діалог «Дозволити доступ» при першому запуску, і для тихих інсталяцій знадобиться правило:
  ```powershell
  New-NetFirewallRule -DisplayName "Torgashka API" -Direction Inbound -Protocol TCP -LocalPort 8000 -Action Allow -Profile Private
  ```
- **Рекомендація:** залишити `127.0.0.1` жорстко (у коді backend та у `VITE_API_BASE_URL`) — безпечніше і без firewall-кроків.

### 4.3. Автозапуск embedded API — Rust-фасад (Python sidecar ДЕЗАКТИВОВАНО)

> ⚠️ **АКТУАЛІЗАЦІЯ (2026-08-21):** Python sidecar дезактивовано (етап 8).
> API повністю перенесено в Rust-ядро: `crates/torgashka-api` (axum-фасад)
> біндить `127.0.0.1:8000` прямо з бінарника Tauri. **PyInstaller/backend.exe НЕ ПОТРІБНІ.**
> Секції нижче залишено як історичну довідку — не виконувати.

### 4.3. Автозапуск embedded API (Python backend)

На Windows Rust-шар (Tauri) має спавнити backend як child process:

```
Torgashka.exe (GUI, Rust)
└── python backend (child process, 127.0.0.1:8000)
    └── завершується разом із GUI (kill при exit)
```

**Два варіанти постачання backend:**

| Варіант | Як | Плюси | Мінуси |
|---|---|---|---|
| **A. PyInstaller onefile** (рекомендовано) | `pyinstaller --onefile --noconsole backend/main.py` → exe кладеться в `src-tauri/resources/` → бандлиться в інсталятор | Один exe, немає залежності від встановленого Python | Більший розмір інсталятора; треба тестувати антивіруси |
| **B. Системний Python** | Spawn `pythonw.exe backend/main.py` з venv | Менший розмір | Потрібен встановлений Python на машині клієнта — неприйнятно для продакшн-релізу |

**Реалізація (варіант A, Rust):**
```rust
// frontend/src-tauri/src/ — spawn при запуску GUI
use tauri_plugin_shell::ShellExt;
use std::path::PathBuf;

#[tauri::command]
fn start_backend(app: tauri::AppHandle) -> Result<u32, String> {
    // resources: backend.exe покладено через bundle.resources у tauri.conf.json
    let exe = app.path().resource_dir()?.join("backend.exe");
    let child = app.shell().sidecar("backend.exe")
        .map_err(|e| e.to_string())?
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(child.pid())
}
```
У `tauri.conf.json` додати:
```jsonc
"bundle": {
  "resources": ["resources/backend.exe"]   // PyInstaller onefile
}
```
На Linux цей же механізм уже працює через systemd user unit — на Windows він НЕ потрібен, spawn через Tauri process/plugin-shell.

**Завершення:** при виході GUI (`RunEvent::ExitRequested` / `WindowEvent::CloseRequested`) — `child.kill()`, щоб не лишати «висячий» python-процес, який займає порт 8000.

### 4.4. Порт-конфлікти (стратегія fallback)

`authStore.ts`: `API_BASE_URL = import.meta.env.DEV ? '/api/v1' : (VITE_API_BASE_URL || 'http://127.0.0.1:8000/api/v1')`.

**Проблема:** порт 8000 може бути зайнятий іншим застосунком → backend не стартує → GUI без даних.

**Стратегія (реалізувати в Rust-шарі):**
1. При старті перевірити, чи 8000 вільний: спроба TCP connect до `127.0.0.1:8000` з таймаутом 200 мс.
2. Якщо зайнятий — спробувати 8001, 8002, …, 8010 (перший вільний).
3. Передати вибраний порт у backend (аргумент CLI: `backend.exe --port 8003`) та у frontend.
4. Frontend має знати фактичний порт **до** завантаження застосунку. Варіанти:
   - **Статично (найпростіше для першого релізу):** `VITE_API_BASE_URL` встановлюється при build через env змінну workflow. Якщо порт завжди 8000 — fallback не потрібен.
   - **Динамічно (правильно):** Rust визначає порт до створення вікна, прокидає у frontend через `window.__TAURI__` / invoke / query-параметр, frontend формує base URL.
5. **Мінімальний прийнятний варіант на перший реліз:** зафіксувати 8000, при конфлікті показати діалог «Порт 8000 зайнятий — звільніть порт або завершіть інші застосунки» (tauri-plugin-dialog вже в залежностях).

### 4.5. Шляхи до даних (app_data_dir на Windows)

| API Tauri | Windows-шлях | Приклад |
|---|---|---|
| `app_data_dir()` | `%APPDATA%\{identifier}` | `C:\Users\<user>\AppData\Roaming\com.torgashka.pos` |
| `app_local_data_dir()` | `%LOCALAPPDATA%\{identifier}` | `C:\Users\<user>\AppData\Local\com.torgashka.pos` |
| `app_config_dir()` | `%APPDATA%\{identifier}` | як `app_data_dir` |
| `app_cache_dir()` | `%LOCALAPPDATA%\{identifier}\cache` | кеш друку/QR |

- `identifier = com.torgashka.pos` (з tauri.conf.json) — шляхи детерміновані.
- **Міграція з Linux:** дані, що зараз у `~/.config/...` (Linux), на Windows будуть в іншому місці. Якщо клієнт переходить з Linux на Windows — передбачити експорт/імпорт даних.
- Автозапуск: `tauri-plugin-autostart` (вже в залежностях) на Windows пише в реєстр `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` — для `currentUser`-інсталяції працює без прав адміністратора.

---

## 5. Перевірка та приймальні тести

**Стенд:** чиста Windows 10/11 VM (або фізична машина без встановленого Torgashka), користувач БЕЗ прав адміністратора.

| # | Чек | Очікуваний результат | Статус |
|---|---|---|---|
| 5.1 | Встановити `Torgashka_1.0.0_x64-setup.exe` (NSIS) | Інсталятор стартує, мова — Українська (вибір мов працює), встановлення без UAC-промпту, ярлики створені | ☐ |
| 5.2 | Встановити `Torgashka_1.0.0_x64_en-US.msi` (WiX) на іншій VM | MSI встановлюється через `msiexec /i ... /qn` (тиха) та через GUI; видно в «Установка та видалення програм» | ☐ |
| 5.3 | Перший запуск (обидва інсталятори) | GUI відкривається, WebView2 не потребує ручного встановлення, backend стартує як child process (перевірити `netstat -ano | findstr 8000`) | ☐ |
| 5.4 | Логін | Введення облікових даних → успішний вхід, дані підтягуються з API | ☐ |
| 5.5 | Перемикач точок (POS) | Перемикання торгової точки зберігається, інтерфейс оновлюється, зміни видно після перезапуску | ☐ |
| 5.6 | Повний цикл роботи | Створення чеку/продажу, друк (якщо є принтер), вихід із застосунку → `python`-процес завершився (порт 8000 звільнено) | ☐ |
| 5.7 | Оновлення через updater | Випустити тестовий `v1.0.1` (зменшена версія), натиснути «Перевірити оновлення» → застосунок скачує `.exe`/`.sig`/`latest.json` з GitHub Release, встановлює, перезапускається | ☐ |
| 5.8 | Порт 8000 зайнятий | Запустити dummy-сервер на 8000 → Torgashka коректно обробляє конфлікт (fallback або зрозумілий діалог) | ☐ |
| 5.9 | Офлайн-режим | Вимкнути мережу → застосунок працює з локальними даними, не «падає» | ☐ |
| 5.10 | Деінсталяція | Обидва інсталятори коректно видаляються, шляхи `%APPDATA%\com.torgashka.pos` очищені (або збережені за вимогою) | ☐ |

**Автоматизація (опційно, пізніше):** smoke-тест у workflow після збірки — встановити MSI тихо, запустити exe, перевірити процес і порт:
```powershell
msiexec /i "frontend\src-tauri\target\release\bundle\msi\Torgashka_1.0.0_x64_en-US.msi" /qn /norestart
Start-Process "$env:LOCALAPPDATA\com.torgashka.pos\Torgashka.exe"
Start-Sleep -Seconds 15
Get-NetTCPConnection -LocalPort 8000 -ErrorAction SilentlyContinue | Select LocalAddress,LocalPort,State
```

---

## 6. Ризики та мітігації

### 6.1. ⚠️ Невідповідність репозиторію updater (ВИСОКИЙ пріоритет)

**Факт:** `plugins.updater.endpoints` = `github.com/torgashka/torgashka/...`, а `git remote` = `moroznastya/KASA`. Updater-клієнт качатиме `latest.json` з `torgashka/torgashka` — якщо такого репозиторію немає або він порожній, оновлення НЕ працюватимуть.

**Рішення (вибрати ОДНЕ, до першого релізу):**

| Варіант | Дія | Коли обирати |
|---|---|---|
| **A. Виправити endpoints під фактичний remote (швидко, рекомендовано на зараз)** | У `tauri.conf.json` змінити на `https://github.com/moroznastya/KASA/releases/latest/download/latest.json` | Поки репозиторій не перейменовано |
| **B. Перейменувати репозиторій на `torgashka/torgashka`** | GitHub → Settings → Repository name; оновити remote: `git remote set-url origin git@github-my:torgashka/torgashka.git` | Коли бренд зафіксовано, публічний реліз |

**Критерій прийняття:** після релізу `curl -I https://github.com/<ВЛАСНИК>/<РЕПО>/releases/latest/download/latest.json` повертає `200 OK`, і значення в конфігу збігається з remote. **Це блокувальний чекліст-пункт перед релізом.**

### 6.2. Порт 8000 зайнятий (СЕРЕДНІЙ)

- **Мітігація:** стратегія fallback §4.4; мінімально — діалог з поясненням.
- **Додатково:** перевірити, чи немає в коді hardcode `8000` поза `authStore.ts` (пошук: `grep -rn "8000" frontend/src backend/`).

### 6.3. SmartScreen / код-сігнінг (СЕРЕДНІЙ)

**Сценарій 1 — сертифікат Є (рекомендований для продакшн):**
- Отримати Code Signing сертифікат (EV або OV) від Sectigo/DigiCert/GlobalSign.
- Підписувати на Windows-кроці workflow (потрібен секрет із PFX + пароль):
  ```powershell
  # Крок у windows-build.yml після tauri build:
  - name: 🔏 Sign installers
    shell: pwsh
    run: |
      $cert = "C:\cert.pfx"
      [IO.File]::WriteAllBytes($cert, [Convert]::FromBase64String($env:CERT_B64))
      & "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe" sign /fd SHA256 /f $cert /p $env:CERT_PASS /tr http://timestamp.digicert.com /td SHA256 `
        "frontend\src-tauri\target\release\bundle\nsis\*.exe" `
        "frontend\src-tauri\target\release\bundle\msi\*.msi"
    env:
      CERT_B64: ${{ secrets.WIN_CERT_BASE64 }}
      CERT_PASS: ${{ secrets.WIN_CERT_PASSWORD }}
  ```
- Після підпису — перевірка: `Get-AuthenticodeSignature *.exe` → `Status = Valid`.
- Інсталятор, підписаний довіреним ЦС, проходить SmartScreen без попереджень.

**Сценарій 2 — без сертифіката (перший реліз):**
- Користувачі побачать «Windows захистив ваш ПК» → «Докладніше» → «Виконати все одно».
- **Мітігація UX:** README/реліз-нотатки з інструкцією; згодом — обов'язково придбати сертифікат.
- **НЕ** використовувати самопідписані сертифікати для публічних релізів — гірше, ніж непідписаний (ще одне попередження).

### 6.4. NSIS vs MSI — коли що використовувати

| Критерій | NSIS (`.exe`) | WiX (`.msi`) |
|---|---|---|
| Цільова аудиторія | Кінцеві користувачі, малий бізнес | Корпоративний деплой (GPO/Intune/SCCM) |
| Досвід встановлення | Швидкий, звичний, кастомні сторінки | Стандартні MSI-діалоги, тиха установка |
| Оновлення через updater | ✅ основний шлях (Tauri updater працює з NSIS) | ✅ також підтримується |
| Права | `currentUser` — без UAC | Часто потребує admin (perMachine) |
| Деінсталяція | Власний uninstaller | Стандартна через «Установка та видалення програм» |
| Кастомізація | Багата (мови, банери, сторінки) | Обмежена без шаблону WiX |

**Рішення:** публікувати **обидва** (NSIS — основний для користувачів, MSI — для enterprise). Якщо ресурси обмежені — спочатку NSIS, MSI другим кроком.

### 6.5. Інші ризики

| Ризик | Мітігація |
|---|---|
| Rust-фічі `x11`, `dbus` у Cargo.toml — Linux-специфічні | На Windows вони умовно компілюються (cfg), збірці не заважають. Перевірити відсутність безумовних `use` Linux-крейтів у `src/commands` (grep `x11\|dbus\|systemd` у `frontend/src-tauri/src/`) |
| `crates/*` workspace — можливі native-залежності | `cargo build --release` на windows-latest покаже одразу; фіксувати до релізу |
| Антивіруси (PyInstaller-бандл часто фолз-позитив) | Підписати exe, надіслати на аналіз у Microsoft Defender/інші; мінімізувати розмір бандла |
| Секрет `TAURI_SIGNING_PRIVATE_KEY` втрачено | Зберігати копію в надійному місці (менеджер паролів); втрата = неможливість оновлень для вже встановлених клієнтів |
| `$schema` nicegui — косметика | Виправити на `https://schema.tauri.app/config/2` |
| Відсутність `header.bmp` для NSIS | Створити (команда в §3.2) ДО збірки, інакше tauri build впаде або згенерує дефолтний банер |

---

## 7. Чекліст перед релізом

### Блокувальні (обов'язково)

- [ ] **Updater endpoints збігаються з git remote** (§6.1): `torgashka/torgashka` **або** виправлено на `moroznastya/KASA` в `tauri.conf.json`
- [ ] `plugins.updater.pubkey` збігається з ключем, згенерованим `tauri signer generate` (секрети додано в GitHub)
- [ ] Версія збігається: `tauri.conf.json` version == `Cargo.toml` version == `frontend/package.json` version == git tag `vX.Y.Z`
- [ ] `icons/header.bmp` створено (150×57, BMP3)
- [ ] `npm run build` і `npm run tauri:build -- --bundles nsis,msi` проходять на `windows-latest` (зелений workflow)
- [ ] Інсталятори підписано (або свідомо прийнято сценарій SmartScreen §6.3)
- [ ] Приймальні тести §5: 5.1–5.10 пройдено на чистій Windows

### Рекомендовані

- [ ] Перевірено `grep -rn "8000" frontend/src backend/` — всі згадки порту узгоджені
- [ ] Поведінка при зайнятому порту 8000 визначена (fallback або діалог)
- [ ] Backend бандлиться (PyInstaller onefile) і спавниться як child process; при виході GUI процес завершується
- [ ] Реліз-нотатки українською (що нового, як встановити, відомі проблеми)
- [ ] README оновлено: розділ «Встановлення на Windows» з обома інсталяторами
- [ ] Тестовий `vX.Y.(Z+1)` випущено і перевірено оновлення через updater (§5.7)
- [ ] Дані Linux→Windows: визначено процедуру переносу (експорт/імпорт)

### Після релізу (ретроспектива)

- [ ] `curl -I https://github.com/<ВЛАСНИК>/<РЕПО>/releases/latest/download/latest.json` → 200
- [ ] Завантажити інсталятори з Release і встановити на 2-й чистій машині
- [ ] Зафіксувати в ROADMAP: наступний реліз, придбання code signing сертифіката, динамічний fallback порту

---

## Додаток A. Локальна Windows-збірка (варіант 1b) — стисло

Якщо GitHub Actions недоступний, на нативній Windows:

```powershell
# 1. Передумови (один раз):
#    - Node.js 24 LTS  → https://nodejs.org
#    - Rust MSVC       → https://rustup.rs (профіль default = msvc)
#    - Git for Windows
#    - Visual Studio Build Tools (C++ workload) — для MSVC linker

# 2. Збірка:
cd frontend
npm ci
npm run build
npm run tauri:build -- --bundles nsis,msi

# 3. Результат:
#    frontend\src-tauri\target\release\bundle\nsis\Torgashka_1.0.0_x64-setup.exe
#    frontend\src-tauri\target\release\bundle\msi\Torgashka_1.0.0_x64_en-US.msi
#    frontend\src-tauri\target\release\bundle\nsis\latest.json  (+ .sig)
```

## Додаток B. Cross-compile з Linux (варіант 1c) — попередження

```bash
# Експериментально, НЕ для релізів:
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin
cd frontend/src-tauri
cargo xwin build --release --target x86_64-pc-windows-msvc
```
- MSI (WiX) cross-compile неможливий — потрібен Windows.
- NSIS через Linux: `makensis` доступний, але Tauri CLI на Linux не генерує Windows-інсталятори повноцінно. Використовувати тільки для перевірки компіляції Rust-коду під Windows target.
