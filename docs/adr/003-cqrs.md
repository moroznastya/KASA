# ADR 003: CQRS Pattern (Use Cases)

## Статус
✅ Прийнято

## Контекст
API v1 використовує прямі SQLAlchemy виклики в роутерах.
Потрібен чистий інтерфейс між API та бізнес-логікою.

## Рішення
CQRS (Command Query Responsibility Segregation) через Use Cases:

- **Commands**: createProduct, updateProduct, deleteProduct, createInvoice, ...
- **Queries**: getProduct, listProducts, getInvoice, ...
- Використовувати DI Container для ін'єкції залежностей

## Наслідки
✅ + Чистий інтерфейс: API не залежить від БД
✅ + Тестування: Use Cases тестуються з mock-ами
✅ + API v1 і v2 можуть співіснувати
⚠️ - Більше коду (DTO, Use Cases, DI)
