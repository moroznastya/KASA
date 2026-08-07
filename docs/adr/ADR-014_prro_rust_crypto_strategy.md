# ADR 014: Крипто-стратегія ПРРО в Rust (етап 7 міграції Kasa → Rust)

## Статус
✅ Прийнято (2026-08-07)

## Контекст

Етап 7 міграції Kasa POS → Rust переносить ПРРО-модуль (фіскалізація чеків через
ДПС України) з Python-еталона (`backend/app/infrastructure/services/prro/`) у
Rust-крейт. Ключове питання — **крипто-стратегія**: як підписувати фіскальні
документи СЗЗД 2.1.7 кваліфікованим електронним підписом (КЕП) у Rust.

### Факти з Python-еталона (перевірено на реальному ключі)

1. **Тестовий ключ** `certs/prro-test/pb_3791505547 (2).jks` (пароль `test2003`)
   містить **ДСТУ 4145-2002** приватний ключ: PKCS#8 OID
   `1.2.804.2.1.1.1.1.3.1.1` (little-endian) або `1.2.804.2.1.1.1.1.3.1.2`
   (big-endian). `PrroCryptoSigner._load_from_jks()` детектує цей OID і
   перемикає бекенд на ІІТ SDK.
2. **Python не підписує ДСТУ 4145 самостійно**: для цього використовується
   крипто-ядро ІІТ — `backend/vendor/iit-sdk/opt/iit/eu/sw/euscp.so`
   (SDK EUSignCP), завантажене через `ctypes` (`iit_sdk.py`). Функції:
   `EUGetJKSPrivateKeyFile` → `EUSaveCertificate` → `EUReadPrivateKeyBinary`
   → `EUSignDataInternal` (CAdES-BES, `ee.SignInternal(true, data)` з
   офіційного семпла ДПС programika/prro_sample).
3. **Формат підпису для ДПС**: бінарний **CAdES-BES** (ContentInfo/signedData,
   ДСТУ 4145-2002 + Стрибог-256) — саме ці байти ДПС очікує в
   `Check.check_sign`. Це НЕ XAdES і НЕ простий DER-підпис.
4. Для **RSA/ECDSA ключів** (PKCS#12/PEM) Python використовує `signxml`
   (XAdES-BES enveloped, RSA-SHA256) — чистий Python, без SDK.

### Обмеження плану (docs/RUST_MIGRATION_PLAN.md, етап 7)

> **КРИТИЧНЕ ОБМЕЖЕННЯ:** НЕ переписувати ДСТУ 4145 на чистий Rust.
> IIT SDK — через FFI (bindgen) АБО через gRPC (tonic поверх наявного prro.proto).

## Рішення

**Стратегія (a) — FFI до IIT SDK EUSignCP (euscp.so) через `libloading`**
**з ручними `extern "C"` сигнатурами** (без bindgen) для ДСТУ 4145-2002
(JKS/.dat). Для RSA/ECDSA (PKCS#12/PEM) — чистий Rust (XAdES-BES, RSA-SHA256).

### Чому не bindgen

bindgen — лише генератор обгорток з C-заголовків. Він вимагає `libclang`
у середовищі збірки (CI, машина розробника) і генерує великий обсяг коду
для API, якого нам потрібно лише 9 функцій. Python-еталон (`iit_sdk.py`)
оголошує ті самі функції **вручну через ctypes** — це працює в продукти
місяцями. Rust `extern "C"` з `libloading` — прямий аналог ctypes:
оголошуємо сигнатури вручну (9 функцій, стабільний C ABI), завантажуємо
`.so` динамічно. Нуль залежностей від toolchain-інструментів, той самий
рівень контролю, що в еталона.

### Архітектура крипто-шару (Rust)

```
kasa-prro/src/
  crypto/
    mod.rs          — PrroSigner trait (sign/verify/serial/name)
    iit.rs          — IitSdk: libloading → euscp.so, 9 extern "C" функцій
                      (EUInitialize, EUSetSettingsFilePath, EUSetFileStoreSettings,
                       EUGetJKSPrivateKeyFile, EUSaveCertificate, EUReadPrivateKeyBinary,
                       EUSignDataInternal, EUVerifyDataInternal, EUFreeMemory, EUGetErrorDesc)
    xades.rs        — XAdES-BES enveloped RSA-SHA256 для PKCS#12/PEM (7.2)
    factory.rs      — вибір бекенда за OID PKCS#8 (1.2.804.2.1.1.1.1.3.1.{1,2} → iit;
                      інакше → xades)
  keystore.rs       — читання JKS/PKCS#12/PEM: приватний ключ (DER) + сертифікат(и)
                      + OID алгоритму (авто-визначення формату за магією/розширенням)
```

### Потік підпису (7.2, закладено в 7.1)

1. `keystore` читає JKS (власний парсер + PBEWithMD5AndTripleDES) → ключ DER
   + сертифікати DER + OID алгоритму.
2. OID = ДСТУ 4145 → `IitSdk.load_jks_key(path, password)` (ті самі виклики,
   що в Python: EUGetJKSPrivateKeyFile → EUSaveCertificate →
   EUReadPrivateKeyBinary) → `sign_data_internal(xml)` → CAdES-BES bytes.
3. OID = RSA/ECDSA → `xades.sign` (чистий Rust, RSA-SHA256, enveloped).

### Ризики та пом'якшення

| Ризик | Вплив | Пом'якшення |
|---|---|---|
| `.so` відсутній на машині | ПРРО не може підписати ДСТУ 4145 | Той самий стан, що в Python: `vendor/iit-sdk` поза git, ставиться `setup_iit_sdk.sh`. Rust падає з чіткою помилкою "встановіть SDK" |
| C ABI зміниться в новій версії SDK | Збій підпису | SDK заморожений у vendor; сигнатури ідентичні Python-еталону; smoke-тест FFI у CI (якщо .so є) |
| libloading + багатопотоковість | Гонки в SDK | Той самий singleton-підхід, що в Python; Mutex навколо SDK-викликів |
| ДСТУ 4145 підпис не валідний для ДПС | Фіскалізація відхилена | Golden parity: Rust-підпис через той самий SDK == Python-підпис на однакових векторах (7.2) |
| `pkcs12`/`pem` crates не розуміють екзотику | Не завантажиться ключ | Падіння з чіткою помилкою, аналог Python `PrroCryptoError`; fallback на JKS/ІІТ шлях |

## Альтернативи, що розглядалися

| Альтернатива | Чому відхилена |
|---|---|
| **(b) Rust делегує крипто-підпис Python sidecar** | Sidecar — це еталон, який буде **деактивовано** в етапі 8 (дезактивація Python). ПРРО став би довічною залежністю від Python-процесу — суперечить цілі міграції (єдиний бінарник) |
| **(c) gRPC/HTTP від Rust до Python-криптосервісу** | Те саме: Python залишається в проді; додає мережевий hop, стан, обробку помилок — без жодної вигоди |
| **(d) Чистий Rust ДСТУ 4145** (наприклад, `dstu4145` crates) | **Прямо заборонено** планом: ДСТУ 4145-2002 — державний стандарт із закритими векторами; ризик несумісності з сервером ДПС і з бойовими ключами ІІТ «ЦСК-1» (Key-6.dat) невиправданий |
| **(e) bindgen замість libloading** | Потребує libclang у CI і на машинах розробників; для 9 стабільних функцій — надмірність. Ручні сигнатури = той самий підхід, що Python ctypes |

## Наслідки

✅ **Переваги:**
- ДСТУ 4145 підписується тим самим перевіреним крипто-ядром ІІТ, що й Python —
  нульовий ризик несумісності з ДПС.
- Після етапу 8 (дезактивація Python) ПРРО працює в єдиному Rust-бінарнику.
- `libloading` без bindgen — жодних нових вимог до CI/toolchain.
- RSA/ECDSA (PKCS#12/PEM) — чистий Rust, без SDK (як Python signxml).

⚠️ **Ризики:**
- Залежність від наявності `euscp.so` на цільовій машині (той самий стан, що в Python).
- Підпис ДСТУ 4145 неможливо перевірити без SDK (тести — golden проти Python на тих
  самих векторах, 7.2).
- XAdES-BES (RSA) у чистому Rust — значна робота (канонізація C14N, ds:Signature,
  enveloped) — виконується в 7.2 з golden parity проти signxml.

## Пов'язані документи
- ADR-013 (архітектура ПРРО-модуля, Clean Architecture)
- docs/RUST_MIGRATION_PLAN.md (етап 7, критичне обмеження)
- docs/PRRO_IMPLEMENTATION_PLAN.md
- docs/prro_phase0_ping.md (smoke TLS, Python)

> **Документ створено:** Rust_Agent (NIKO) — етап 7.1, 2026-08-07
