# ADR 001: Clean Architecture

## Статус
✅ Прийнято

## Контекст
Torgashka — Enterprise-рівня система з тривалим життєвим циклом. 
Потрібна архітектура, яка:
- Дозволяє змінювати зовнішні залежності (БД, UI) без зміни бізнес-логіки
- Підтримує тестування
- Масштабується

## Рішення
Використовувати Clean Architecture (Robert C. Martin) з 4 шарами:

```
┌─────────────────────────────────────┐
│         Presentation (API)          │
│   app/api/v1/ (legacy)             │
│   app/api/v2/ (use cases based)    │
├─────────────────────────────────────┤
│         Application                 │
│   app/application/use_cases/        │
│   app/application/event_handlers/   │
├─────────────────────────────────────┤
│         Domain                      │
│   app/domain/entities/              │
│   app/domain/services/              │
│   app/domain/events/                │
│   app/domain/repositories/ (ports)  │
├─────────────────────────────────────┤
│         Infrastructure              │
│   app/infrastructure/persistence/   │
│   app/infrastructure/cache/         │
│   app/infrastructure/event_bus/     │
│   app/infrastructure/di/            │
└─────────────────────────────────────┘
```

## Наслідки
✅ + Тестування: можна mock-нути будь-який шар
✅ + Гнучкість: заміна БД не впливає на домен
✅ + Підтримка: чіткі межі відповідальності
⚠️ - Більше коду (interfaces, DI)
⚠️ - Складніше для junior розробників
