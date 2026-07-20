# ADR-003: Domain Events (Події домену)

| Метадані | Значення |
|----------|----------|
| **Статус** | ✅ Прийнято |
| **Дата** | 2026-07-20 |
| **Автор** | System Architect Agent (AEGIS v3) |
| **Версія** | 1.0.0 |

---

## Контекст

В поточній архітектурі модулі комунікують напряму через виклики сервісів:

```python
# Поточний підхід (жорстка зв'язність)
class DocumentService:
    def __init__(self, session):
        self.product_service = ProductService(session)  # Пряма залежність
        self.ledger_service = LedgerService(session)     # Пряма залежність

    async def confirm_invoice(self, invoice_id):
        # ...
        await self.product_service.update_stock(...)  # Жорсткий виклик
        await self.ledger_service.create_entry(...)    # Жорсткий виклик
```

**Проблеми:**
1. **Жорстка зв'язність:** DocumentService знає про ProductService та LedgerService
2. **Порушення SRP:** DocumentService робить занадто багато
3. **Важке розширення:** Щоб додати нову дію при підтвердженні накладної, потрібно змінювати DocumentService
4. **Важке тестування:** Потрібно мокати багато залежностей

## Рішення

Впровадити Domain Events для слабкої зв'язності між модулями:

```python
# 1. Базовий клас події
class DomainEvent:
    event_id: UUID
    occurred_at: datetime
    aggregate_id: UUID

# 2. Конкретна подія
class InvoiceConfirmed(DomainEvent):
    invoice_id: UUID
    supplier_id: UUID
    total_amount: Money
    items: list[InvoiceItem]

# 3. Публікація події в Use Case
class InvoiceUseCase:
    def __init__(self, invoice_repo, unit_of_work, event_bus):
        self._invoice_repo = invoice_repo
        self._uow = unit_of_work
        self._event_bus = event_bus

    async def confirm_invoice(self, invoice_id: UUID) -> Invoice:
        async with self._uow:
            invoice = await self._invoice_repo.find_by_id(invoice_id)
            invoice.confirm()  # Зміна статусу + генерація події
            await self._invoice_repo.save(invoice)
            # Події публікуються після коміту
            await self._uow.commit()
            for event in invoice.events:
                await self._event_bus.publish(event)

# 4. Обробник події (в іншому модулі)
class StockEventHandler:
    def __init__(self, stock_service):
        self._stock_service = stock_service

    async def handle_invoice_confirmed(self, event: InvoiceConfirmed):
        await self._stock_service.apply_invoice(event.items)
```

## Event Bus

```python
# Інтерфейс (Domain)
class IEventBus(ABC):
    @abstractmethod
    async def publish(self, event: DomainEvent): ...
    @abstractmethod
    def subscribe(self, event_type: type, handler: Callable): ...

# In-Memory реалізація (Infrastructure)
class LocalEventBus(IEventBus):
    def __init__(self):
        self._handlers: dict[type, list[Callable]] = {}

    async def publish(self, event: DomainEvent):
        handlers = self._handlers.get(type(event), [])
        for handler in handlers:
            await handler(event)

    def subscribe(self, event_type: type, handler: Callable):
        self._handlers.setdefault(event_type, []).append(handler)
```

## Список подій

| Подія | Джерело | Слухачі |
|-------|---------|---------|
| `ProductCreated` | Products Module | SearchIndex, Reports |
| `StockChanged` | Inventory/Sales | Reports, Alerts |
| `InvoiceConfirmed` | Inventory Module | StockService, LedgerService |
| `InvoiceCancelled` | Inventory Module | StockService, LedgerService |
| `ReceiptCreated` | Sales Module | StockService, Reports |
| `SupplierBalanceChanged` | Finance Module | Reports, Alerts |
| `UserLoggedIn` | Auth Module | AuditLog |

## Обґрунтування

1. **Слабка зв'язність:** Модулі не знають один про одного
2. **Розширюваність:** Нова функціональність додається через нові обробники
3. **Асинхронність (опціонально):** Події можна обробляти асинхронно через RabbitMQ
4. **Аудит:** Легко логувати всі події для аудиту

## Альтернативи

| Альтернатива | За | Проти |
|-------------|-----|-------|
| **Прямі виклики** | Простота | Жорстка зв'язність |
| **Observer Pattern** | Синхронний, простий | Важко масштабувати |
| **Message Queue (RabbitMQ)** | Асинхронний, надійний | Overhead для MVP |

## Наслідки

**Позитивні:**
- ✅ Слабка зв'язність між модулями
- ✅ Легке додавання нової функціональності
- ✅ Природній аудит
- ✅ Можливість асинхронної обробки

**Негативні:**
- ❌ Складніше відстежувати потік виконання
- ❌ Потрібна обережність з транзакціями (події після коміту)
- ❌ Можливі race conditions при синхронній обробці

---

> **Пов'язані ADR:** ADR-001 (4-шарова архітектура), ADR-002 (Repository Pattern)
