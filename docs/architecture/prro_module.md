# 🧩 Модуль: PRRO (Фіскалізація чеків через ДПС України)

> **Статус:** Проєктування (Фаза 0.4) — ADR-013  
> **Дата:** 2026-08-01  
> **Принцип:** Clean Architecture, Dependency Rule, ізольований функціональний модуль

---

## 1️⃣ МІСЦЕ В АРХІТЕКТУРІ

```
┌─────────────────────────────────────────────────────────────────────────┐
│  PRESENTATION                                                           │
│  └── api/v2/prro.py              # HTTP-роутери ПРРО                   │
├─────────────────────────────────────────────────────────────────────────┤
│  APPLICATION                                                           │
│  └── application/use_cases/prro/  # OpenShift, FiscalizeReceipt,        │
│                                   # CloseShift, SyncOfflineQueue,       │
│                                   # PrroSettings                        │
├─────────────────────────────────────────────────────────────────────────┤
│  DOMAIN                                                                │
│  └── domain/entities/             # Product, Receipt, ReceiptItem,      │
│                                   # Invoice, ReturnInvoice (фіск. поля) │
│  └── domain/repositories/         # IPrroSettingRepository,            │
│                                   # IPrroShiftRepository,              │
│                                   # IPrroQueueRepository               │
├─────────────────────────────────────────────────────────────────────────┤
│  INFRASTRUCTURE                                                        │
│  ├── infrastructure/services/prro/ # gRPC-клієнт, XML-білдер,           │
│  │                                 # криптосервіс, key-store,          │
│  │                                 # офлайн-черга, prro.proto         │
│  └── infrastructure/persistence/   # models/prro.py,                   │
│                                    # repositories/prro_repository.py   │
└─────────────────────────────────────────────────────────────────────────┘
```

Модуль ПРРО — **горизонтальний зріз** через усі 4 шари, як і модулі Products, Receipts.
Не створює циклічних залежностей: Presentation → Application → Domain ← Infrastructure.

## 2️⃣ ВІДПОВІДАЛЬНІСТЬ

- Формування фіскальних чеків (СЗЗД 2.1.7) для продажів/повернень
- Підпис XAdES (КЕП) та передача на фіскальний сервер ДПС (gRPC, `sendChkV2`)
- Часткова фіскалізація: спліт чеків на фіскальну/нефіскальну частину (`split_group_id`)
- Управління касовими змінами (відкриття Т=108, закриття Z-звіт)
- Офлайн-черга з резервними номерами (Т=112) та синхронізацією (ліміт 168 год)
- Безпечне зберігання ключа КЕП та налаштувань (шифрування at-rest, master-key)

## 3️⃣ КЛЮЧОВІ РІШЕННЯ

| Рішення | Обґрунтування | ADR |
|---------|---------------|-----|
| gRPC `sendChkV2` (не `sendChk`) | Метод `sendChk` діє лише до 01.10.2021 | ADR-013 |
| Часткова фіскалізація (спліт) | Продаж нефіскального товару не блокується нестачею fiscal_stock | ADR-013 |
| Офлайн-черга + резервні номери | Законодавчий ліміт 168 год, безперервність каси | ADR-013 |
| Шифрування ключа at-rest (AES-GCM + master-key) | Вимоги безпеки КЕП | ADR-013 |
| Один підписант на зміну | Вимога протоколу ДПС | ADR-013 |

## 4️⃣ СТАТУС РЕАЛІЗАЦІЇ

- ✅ Міграції `f89706f0cc25`, `f89706f0cc26` — фіскальні поля Product/Receipt/ReceiptItem
- ✅ `models/prro.py` — PrroSetting, PrroShift, PrroQueueItem (чернетка)
- ✅ `services/prro/` — prro.proto, grpc_client.py, xml_builder.py (початок)
- ⬜ application/use_cases/prro/* — планується
- ⬜ api/v2/prro.py — планується
- ⬜ crypto_signer.py, key_store.py, offline_queue.py — планується

---

> **Документ створено:** System Architect Agent (AEGIS v3) — Фаза 0.4  
> **Останнє оновлення:** 2026-08-01
