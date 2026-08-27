# План відлагодження модуля ПРРО (torgashka-prro)

**Дата:** 2026-08-27
**Автор:** NIKO (координатор)
**Об'єкт:** `frontend/src-tauri/crates/torgashka-prro/` (Rust) + Python-еталон `backend/app/infrastructure/services/prro/` (1:1 parity)
**База:** аудит від 2026-08-27 (перевірено по коду, 7/10 підтверджено) + старий аудит 09.08
**Статус:** АКТИВНИЙ

---

## Пріоритети (блокери → високі → середні)

| # | Проблема | Серйозність | Файли |
|---|----------|-------------|-------|
| B1 | Відсутній hash попереднього Check (контроль послідовності) | 🔴 КРИТИЧНО | fiscalize.rs, xml.rs, shift.rs, sync.rs |
| B2 | Sync переформовує документ: новий NT/MAC/підпис при повторній відправці (порушення ідемпотентності) | 🔴 КРИТИЧНО | sync.rs, xml.rs (build_message), queue.rs |
| B3 | `rro_fn_sign` порожній у всіх RPC | 🔴 ВИСОКО | grpc.rs (5 місць) |
| B4 | Offline = retry queue, а не offline ПРРО: T=109/110/112 не використовуються, `id_offline` порожній | 🔴 ВИСОКО | xml.rs, fiscalize.rs, sync.rs, shift.rs |
| H1 | Timeout recovery: lastChk тільки при -3/-12, транспортний таймаут → сліпі ретраї | 🟠 ВИСОКО | fiscalize.rs (on_error), grpc.rs |
| M1 | Race на local_number (без атомарності/lock) | 🟡 СЕРЕДНЬО | fiscalize.rs, shift.rs |
| V1 | Валідація MAC (SHA-256 base64) та QR (mac=id_sign) проти офіційного семпла ДПС / test API | 🟠 ВЕРИФІКАЦІЯ | xml.rs, settings.rs, fiscalize.rs |

**Не чіпати:** date_time — вже виправлено (обидва yyyyMMddHHmmss, 1:1 Python).

---

## Фази

### Фаза 0 — BASELINE (контрольна точка)
- **Вхід:** поточний код.
- **Дія:** `cargo test` у `frontend/src-tauri/crates/torgashka-prro`; Python: `pytest backend/tests/unit/services/test_prro_*.py backend/tests/unit/use_cases/test_prro_*.py`.
- **Вихід:** звіт: скільки тестів зелених/червоних, які файли падають.
- **Критерій:** зафіксовано точку старту; жоден тест не змінено до старту.

### Фаза 1 — B1: Hash-chain попереднього Check (ВИКОНАНО)
- **Вхід:** XML builder, fiscalize, shift, sync.
- **Дія:**
  1. Визначити формат хеша попереднього Check згідно СЗЗД 2.1.7 (поле попереднього документу; звірити з наявним `docs/prro_spec_validation.md`).
  2. Зберігати hash останнього успішно відправленого Check (persist, не тільки в пам'яті).
  3. Вставляти hash у наступний Check (крім ping T=111 і службових 108/109/110/112 за протоколом).
  4. Те саме в Python-еталоні (1:1).
- **Вихід:** змінені fiscalize.rs/xml.rs/sync.rs/shift.rs + Python; юніт-тести ланцюжка.
- **Критерій:** 3 чеки поспіль: H(c1)→c2, H(c2)→c3; golden-тест XML містить коректне поле; Python і Rust генерують ідентичний XML.

### Фаза 2 — B2: Ідемпотентність sync (immutable фіскальний документ) (ВИКОНАНО)
- **Вхід:** sync.rs, xml.rs, queue.rs, offline_queue.py.
- **Дія:**
  1. При первинному формуванні зберігати ПОВНИЙ підписаний `check_sign` (XML RQ + MAC + підпис) у queue (поле xml_body/check_sign).
  2. При sync НЕ переформовувати: відправляти збережений `check_sign` as-is.
  3. NT/MAC не змінюються між спробами.
  4. Те саме в Python-еталоні.
- **Вихід:** sync.rs + queue моделі + Python; тест: 2 спроби sync з тим самим документом → ідентичний check_sign.
- **Критерій:** `build_message` викликається рівно 1 раз на документ; повторні sync не змінюють NT.

### Фаза 3 — B3: rro_fn_sign (ВИКОНАНО)
- **Вхід:** grpc.rs, keystore.rs, crypto/.
- **Дія:**
  1. Реалізувати підпис `rro_fn` (фіскальний номер ПРРО) тим самим КЕП-ключем.
  2. Заповнити `rro_fn_sign` у sendChkV2/statusRro/infoRro/lastChk/delLastChk/delLastChkId.
  3. Python-еталон: перевірити, чи заповнює; якщо ні — синхронно доповнити.
- **Вихід:** grpc.rs + Python grpc_client.py; юніт-тест: rro_fn_sign != empty, валідний підпис.
- **Критерій:** жоден `CheckRequest` не містить `Vec::new()` для rro_fn_sign.

### Фаза 4 — B4: Offline state machine (109/110/112 + id_offline) (ВИКОНАНО)
- **Вхід:** xml.rs (SERVICE_OFFLINE/ONLINE/RESERVE), fiscalize.rs, sync.rs, shift.rs, queue.rs.
- **Дія:**
  1. Реалізувати перехід ONLINE→OFFLINE: T=109.
  2. Реалізувати запит резервного діапазону номерів: T=112.
  3. Offline-чеки: генерувати з `id_offline` (не порожнім).
  4. Реалізувати перехід OFFLINE→ONLINE: T=110 + відправка offline-ланцюжка.
  5. Python-еталон синхронно.
- **Вихід:** модуль стану offline + використання id_offline + Python; тести переходів.
- **Критерій:** сценарій: online→(мережа впала)→109→112→offline-чеки з id_offline→(мережа є)→110→sync; усі документи пройшли.

### Фаза 5 — H1: Безпечний timeout recovery (ВИКОНАНО)
- **Вхід:** fiscalize.rs (on_error), grpc.rs.
- **Дія:**
  1. При транспортному таймауті: НЕ сліпий retry — спочатку `lastChk`.
  2. Порівняти local_number: якщо наш чек уже там → SENT; якщо ні → retry.
- **Вихід:** оновлений on_error/recovery; тест сценарію timeout.
- **Критерій:** після timeout немає дублікатів; документ не втрачається.

### Фаза 6 — M1: Атомарний local_number (ВИКОНАНО)
- **Вхід:** fiscalize.rs, shift.rs, repository.rs.
- **Дія:**
  1. Mutex/RwLock на рівні сервісу ПРРО (або SQL `SELECT ... FOR UPDATE`).
  2. Інкремент + збереження в одній атомарній операції.
- **Вихід:** lock у сервісі + тест конкурентного доступу.
- **Критерій:** N паралельних фіскалізацій → N унікальних послідовних local_number.

### Фаза 7 — V1: Валідація MAC/QR (ВИКОНАНО)
- **Вхід:** xml.rs (compute_mac), settings.rs (build_fiscal_check_url), docs/prro_spec_validation.md, семпли ДПС.
- **Дія:**
  1. Звірити алгоритм MAC з офіційним семплом (Sender.java / приклади в документації).
  2. Перевірити, чи має QR `mac` бути MAC чека замість id_sign; виправити в Rust + Python.
  3. Якщо є доступ до test API — smoke-тест реальним сервером (ping/чек).
- **Вихід:** висновок + виправлення (якщо потрібно) + документація.
- **Критерій:** MAC і QR відповідають офіційному опису; результати задокументовані.

### Фаза 8 — ФІНАЛЬНА ВЕРИФІКАЦІЯ
- **Дія:** повний `cargo test` + `pytest` (prro-набір) + `cargo clippy`.
- **Вихід:** звіт з результатами, список відкритих питань.
- **Критерій:** 100% тестів зелених (або задокументовані відхилення); жоден новий тест не пропущено.

---

### Фаза 2 — КОРІНЬ OOM: мок-крипта замість FFI у юніт-тестах (ВИКОНАНО 27.08.2026)
- **Вхід:** src/crypto/ (IitSigner/iit.rs), tests/cades_iit.rs, tests/sdk_subprocess.rs,
  tests/common/mod.rs (MockSigner), test_prro_jks_dstu.py, pytest.ini.
- **Дія:**
  1. Юніт-тести логіки (xml, fiscalize, sync, shift, queue, settings) і так
     використовували MockSigner (src + tests/common) — FFI-викликів не було.
  2. Єдині реальні FFI-точки: `tests/cades_iit.rs` (4 тести, прямий FFI +
     субпроцес) та `tests/sdk_subprocess.rs` (2 тести, субпроцес) — позначено
     `#[ignore = "integration: ..."]`.
  3. Python: `test_dstu_jks_signs_and_verifies_via_iit` → `@pytest.mark.integration`;
     `pytest.ini: addopts = -m "not integration"` (integration — за явним флагом).
  4. Фікс `cades_iit.rs`: `TORGASHKA_PRRO_SDK_HELPER_BIN` (раніше current_exe =
     тестовий бінарник → порожній stdout хелпера).
- **Як запускати інтеграційні (реальний SDK):**
  - Rust: `cargo test --test cades_iit -- --ignored`
    `cargo test --test sdk_subprocess -- --ignored`
  - Python: `pytest -m integration` (з backend/)
- **Вихід:** звичайний `cargo test` → 75 passed + 6 ignored, ЖОДНОГО FFI;
  `pytest` → 602 passed + 83 deselected; 0 процесів cades_iit.
- **Критерій:** після `cargo test` `ps aux | grep cades_iit` → 0 рядків. ✅

---

## Правила виконання
1. Кожна фаза — окремий контракт виконавцю (вхід/вихід/критерій).
2. Rust і Python змінюються СИНХРОННО (1:1 parity — закон проєкту).
3. Не чіпати: .env, ключі, сертифікати, GUI, дата-базу production.
4. Після кожної фази — `cargo test` + `pytest` (prro-набір), звіт координатору.
5. Аномалії — вгору по ієрархії негайно.
6. Жоден запуск тестів/збірки — без run_limited.sh (обгортка лімітів).
