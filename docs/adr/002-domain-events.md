# ADR 002: Domain Events & Event Sourcing

## Статус
✅ Прийнято

## Контекст
Потрібно:
- Логувати всі зміни в системі (аудит)
- Інвалідувати кеш при змінах
- Сповіщати інші компоненти про зміни
- Мати історію змін

## Рішення
Використовувати Domain Events + Event Bus:

1. **Domain Events**: dataclass-и в `app/domain/events/`
2. **Event Bus**: `app/infrastructure/event_bus/LocalEventBus`
3. **Handlers**: `app/application/event_handlers/`
4. **Публікація**: Use Cases публікують події після змін

### Події
- Product: Created, Updated, Deleted, StockChanged
- Invoice: Created, Updated, Deleted, Approved
- Receipt: Created, Refunded
- Ledger: EntryCreated
- User: LoggedIn, Created

## Наслідки
✅ + Аудит: всі зміни логуються
✅ + Кеш: автоматична інвалідація
✅ + Розширюваність: нові handler-и без зміни існуючого коду
⚠️ - Асинхронність: події обробляються в тому ж процесі
