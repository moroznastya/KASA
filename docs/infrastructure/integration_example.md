# Приклад інтеграції: ProductModule ↔ StockModule через EventBus

**Версія:** 1.0.0  
**Дата:** 2025-01-20  
**Статус:** Проєкт (Contract First)

---

## 1. Сценарій: Підтвердження прибуткової накладної

Розглянемо, як три модулі (DocumentModule, StockModule, LedgerModule) взаємодіють через Event Bus при підтвердженні прибуткової накладної.

### 1.1 Поточна архітектура (проблема)

```python
# document_service.py — ПОТОЧНИЙ КОД (жорстке зв'язування)
class DocumentService:
    def __init__(self, session):
        self.session = session
        # Пряма залежність від конкретних сервісів!
        self.product_service = ProductService(session)
        self.ledger_service = LedgerService(session)
    
    async def confirm_invoice(self, invoice_id):
        invoice = await self._get_invoice(invoice_id)
        
        # Прямий виклик ProductService
        for item in invoice.items:
            await self.product_service.update_stock(
                product_id=item.product_id,
                quantity_change=item.quantity,
            )
        
        # Прямий виклик LedgerService
        await self.ledger_service.create_ledger_entry(
            supplier_id=invoice.supplier_id,
            operation_type="invoice",
            amount=invoice.total_amount,
            ...
        )
        
        invoice.status = CONFIRMED
        return invoice
```

**Проблеми:**
1. `DocumentService` жорстко залежить від `ProductService` та `LedgerService`
2. Не можна додати новий модуль (наприклад, `NotificationService`) без зміни `DocumentService`
3. Важко тестувати — потрібні реальні екземпляри всіх сервісів

### 1.2 Цільова архітектура (через Event Bus)

```python
# document_service.py — НОВИЙ КОД (через Event Bus)
class DocumentService:
    def __init__(self, session, event_bus: EventBus):
        self.session = session
        self.event_bus = event_bus  # Залежність тільки від EventBus!
    
    async def confirm_invoice(self, invoice_id):
        invoice = await self._get_invoice(invoice_id)
        
        # Змінюємо статус
        invoice.status = CONFIRMED
        await self.session.flush()
        
        # Публікуємо подію — всі, хто підписаний, отримають сповіщення
        await self.event_bus.publish(Event(
            event_type="invoice.confirmed",
            source_module="document",
            payload={
                "invoice_id": str(invoice.id),
                "invoice_number": invoice.number,
                "supplier_id": str(invoice.supplier_id),
                "total_amount": float(invoice.total_amount),
                "invoice_date": invoice.invoice_date.isoformat(),
                "items": [
                    {
                        "product_id": str(item.product_id),
                        "product_title": item.product_title,
                        "quantity": float(item.quantity),
                        "price": float(item.price),
                    }
                    for item in invoice.items
                ],
                "confirmed_by": "system",
                "confirmed_at": datetime.utcnow().isoformat(),
            }
        ))
        
        return invoice
```

---

## 2. Обробники подій

### 2.1 StockModule — обробник `invoice.confirmed`

```python
# stock_module.py
class StockModule:
    """
    Модуль складу.
    Підписується на події документів для оновлення залишків.
    """
    
    def __init__(self, session, event_bus: EventBus):
        self.session = session
        self.event_bus = event_bus
        
        # Реєструємо обробники подій
        self.event_bus.subscribe("invoice.confirmed", self.handle_invoice_confirmed)
        self.event_bus.subscribe("invoice.cancelled", self.handle_invoice_cancelled)
        self.event_bus.subscribe("receipt.created", self.handle_receipt_created)
        self.event_bus.subscribe("return.confirmed", self.handle_return_confirmed)
    
    async def handle_invoice_confirmed(self, event: Event) -> None:
        """
        Обробник події підтвердження накладної.
        
        Збільшує залишки товарів на складі.
        """
        payload = event.payload
        
        for item in payload["items"]:
            product_id = UUID(item["product_id"])
            quantity = Decimal(str(item["quantity"]))
            
            # Оновлюємо залишок
            product = await self._get_product(product_id)
            old_stock = product.stock or Decimal("0")
            product.stock = old_stock + quantity
            
            # Публікуємо подію про зміну залишку
            await self.event_bus.publish(Event(
                event_type="stock.changed",
                source_module="stock",
                payload={
                    "product_id": str(product_id),
                    "product_title": item["product_title"],
                    "old_quantity": float(old_stock),
                    "change": float(quantity),
                    "new_quantity": float(product.stock),
                    "reason": "invoice_confirmed",
                    "document_id": payload["invoice_id"],
                    "document_type": "invoice",
                    "document_number": payload["invoice_number"],
                }
            ))
            
            # Перевіряємо мінімальний рівень
            if product.min_stock and product.stock < product.min_stock:
                await self.event_bus.publish(Event(
                    event_type="stock.low",
                    source_module="stock",
                    payload={
                        "product_id": str(product_id),
                        "product_title": item["product_title"],
                        "current_stock": float(product.stock),
                        "min_stock": float(product.min_stock),
                    }
                ))
        
        await self.session.flush()
        print(f"[StockModule] Оновлено залишки для накладної {payload['invoice_number']}")
    
    async def _get_product(self, product_id: UUID):
        """Отримує товар з БД."""
        from sqlalchemy import select
        from app.models.product import Product
        result = await self.session.execute(
            select(Product).where(Product.id == product_id)
        )
        product = result.scalar_one_or_none()
        if not product:
            raise Exception(f"Product {product_id} not found")
        return product
```

### 2.2 LedgerModule — обробник `invoice.confirmed`

```python
# ledger_module.py
class LedgerModule:
    """
    Модуль взаєморозрахунків.
    Підписується на події документів для ведення журналу.
    """
    
    def __init__(self, session, event_bus: EventBus):
        self.session = session
        self.event_bus = event_bus
        
        # Реєструємо обробники подій
        self.event_bus.subscribe("invoice.confirmed", self.handle_invoice_confirmed)
        self.event_bus.subscribe("invoice.cancelled", self.handle_invoice_cancelled)
        self.event_bus.subscribe("return.confirmed", self.handle_return_confirmed)
    
    async def handle_invoice_confirmed(self, event: Event) -> None:
        """
        Обробник події підтвердження накладної.
        
        Створює запис у журналі взаєморозрахунків.
        """
        payload = event.payload
        
        # Отримуємо поточний баланс
        current_balance = await self._get_balance(UUID(payload["supplier_id"]))
        amount = Decimal(str(payload["total_amount"]))
        
        # Створюємо запис
        from app.models.supplier_ledger import SupplierLedger, LedgerOperationType
        entry = SupplierLedger(
            supplier_id=UUID(payload["supplier_id"]),
            operation_type=LedgerOperationType.INVOICE,
            document_id=UUID(payload["invoice_id"]),
            document_number=payload["invoice_number"],
            amount=amount,
            balance_after=current_balance + amount,
            operation_date=datetime.fromisoformat(payload["invoice_date"]),
            notes=f"Прибуткова накладна №{payload['invoice_number']}",
        )
        self.session.add(entry)
        await self.session.flush()
        
        # Публікуємо подію про створення запису
        await self.event_bus.publish(Event(
            event_type="ledger.entry_created",
            source_module="ledger",
            payload={
                "entry_id": str(entry.id),
                "supplier_id": payload["supplier_id"],
                "operation_type": "invoice",
                "amount": float(amount),
                "balance_after": float(entry.balance_after),
                "document_id": payload["invoice_id"],
                "document_number": payload["invoice_number"],
            }
        ))
        
        print(f"[LedgerModule] Створено запис для накладної {payload['invoice_number']}")
    
    async def _get_balance(self, supplier_id: UUID) -> Decimal:
        """Отримує поточний баланс постачальника."""
        from sqlalchemy import select, func
        from app.models.supplier_ledger import SupplierLedger
        result = await self.session.execute(
            select(func.coalesce(func.sum(SupplierLedger.amount), 0))
            .where(SupplierLedger.supplier_id == supplier_id)
        )
        return Decimal(str(result.scalar() or "0.00"))
```

---

## 3. Ініціалізація системи

```python
# main.py — НОВА ВЕРСІЯ
from app.core.event_bus import EventBus
from app.core.service_registry import ServiceRegistry, ServiceInfo
from app.core.di_container import DIContainer
from app.core.context_provider import ContextProvider
from app.services.document_service import DocumentService
from app.services.stock_module import StockModule
from app.services.ledger_module import LedgerModule

# Глобальні екземпляри
event_bus = EventBus()
service_registry = ServiceRegistry()
di_container = DIContainer()
context_provider = ContextProvider()


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Ініціалізація системи при старті."""
    
    # 1. Реєструємо сервіси в DI Container
    di_container.register("document_service", lambda c: DocumentService(
        session=get_session(),
        event_bus=event_bus,
    ))
    di_container.register("stock_module", lambda c: StockModule(
        session=get_session(),
        event_bus=event_bus,
    ))
    di_container.register("ledger_module", lambda c: LedgerModule(
        session=get_session(),
        event_bus=event_bus,
    ))
    
    # 2. Реєструємо сервіси в Service Registry
    service_registry.register(ServiceInfo(
        name="document_service",
        version="1.0.0",
        description="Управління документами",
        events_publishes=["invoice.confirmed", "invoice.cancelled", ...],
        events_subscribes=[],
    ))
    service_registry.register(ServiceInfo(
        name="stock_module",
        version="1.0.0",
        description="Управління залишками",
        events_publishes=["stock.changed", "stock.low"],
        events_subscribes=["invoice.confirmed", "invoice.cancelled", ...],
    ))
    service_registry.register(ServiceInfo(
        name="ledger_module",
        version="1.0.0",
        description="Взаєморозрахунки",
        events_publishes=["ledger.entry_created"],
        events_subscribes=["invoice.confirmed", "return.confirmed", ...],
    ))
    
    # 3. Ініціалізуємо модулі (вони самі підписуються на події в конструкторі)
    stock = di_container.resolve("stock_module")
    ledger = di_container.resolve("ledger_module")
    
    # 4. Встановлюємо статуси
    service_registry.set_status("document_service", "active")
    service_registry.set_status("stock_module", "active")
    service_registry.set_status("ledger_module", "active")
    
    print(f"🚀 Torgashka запущено. Зареєстровано {len(service_registry.list_services())} сервісів")
    yield
    print("👋 Torgashka завершує роботу.")
```

---

## 4. API роутер (оновлений)

```python
# invoices.py — НОВА ВЕРСІЯ (через DI Container)
from fastapi import APIRouter, Depends
from app.core.di_container import get_container

router = APIRouter(prefix="/api/v1/invoices", tags=["Накладні"])


@router.post("/{invoice_id}/confirm")
async def confirm_invoice(
    invoice_id: UUID,
    container = Depends(get_container),
):
    """
    Підтверджує прибуткову накладну.
    
    Сервіс отримується через DI Container, а не створюється вручну.
    """
    document_service = container.resolve("document_service")
    invoice = await document_service.confirm_invoice(invoice_id)
    return InvoiceResponse.model_validate(invoice)
```

---

## 5. Діаграма послідовності

```
Користувач          API Router         DocumentService       Event Bus         StockModule       LedgerModule
    │                    │                    │                  │                  │                  │
    │  POST /confirm     │                    │                  │                  │                  │
    │───────────────────►│                    │                  │                  │                  │
    │                    │  confirm_invoice() │                  │                  │                  │
    │                    │───────────────────►│                  │                  │                  │
    │                    │                    │  Зміна статусу   │                  │                  │
    │                    │                    │  на CONFIRMED    │                  │                  │
    │                    │                    │──────────────────│                  │                  │
    │                    │                    │                  │                  │                  │
    │                    │                    │  publish(        │                  │                  │
    │                    │                    │   "invoice.      │                  │                  │
    │                    │                    │    confirmed")   │                  │                  │
    │                    │                    │─────────────────►│                  │                  │
    │                    │                    │                  │                  │                  │
    │                    │                    │                  │  handle(         │                  │
    │                    │                    │                  │   "invoice.      │                  │
    │                    │                    │                  │    confirmed")   │                  │
    │                    │                    │                  │─────────────────►│                  │
    │                    │                    │                  │                  │                  │
    │                    │                    │                  │  Оновлення       │                  │
    │                    │                    │                  │  залишків        │                  │
    │                    │                    │                  │──────────────────│                  │
    │                    │                    │                  │                  │                  │
    │                    │                    │                  │  publish(        │                  │
    │                    │                    │                  │   "stock.        │                  │
    │                    │                    │                  │    changed")     │                  │
    │                    │                    │                  │◄─────────────────│                  │
    │                    │                    │                  │                  │                  │
    │                    │                    │                  │  handle(         │                  │
    │                    │                    │                  │   "invoice.      │                  │
    │                    │                    │                  │    confirmed")   │                  │
    │                    │                    │                  │────────────────────────────────────►│
    │                    │                    │                  │                  │                  │
    │                    │                    │                  │  Створення       │                  │
    │                    │                    │                  │  запису в        │                  │
    │                    │                    │                  │  журналі         │                  │
    │                    │                    │                  │────────────────────────────────────│
    │                    │                    │                  │                  │                  │
    │                    │                    │  Відповідь       │                  │                  │
    │                    │◄───────────────────│                  │                  │                  │
    │◄───────────────────│                    │                  │                  │                  │
    │                    │                    │                  │                  │                  │
```

---

## 6. Переваги нової архітектури

| Аспект | Було (прямі виклики) | Стало (Event Bus) |
|--------|---------------------|-------------------|
| **Залежності** | `DocumentService` → `ProductService`, `LedgerService` | `DocumentService` → `EventBus` |
| **Додавання нового модуля** | Потрібно змінювати `DocumentService` | Просто підписати новий модуль на подію |
| **Тестування** | Потрібні реальні сервіси | Можна тестувати кожен модуль окремо |
| **Відмовостійкість** | Помилка в одному сервісі блокує весь потік | Кожен обробник працює незалежно |
| **Аудит** | Немає історії викликів | Event Bus зберігає всі події |
| **Асинхронність** | Синхронні виклики | Потенційно асинхронні (через черги) |

---

## 7. Код для тестування

```python
# test_integration.py
import pytest
from unittest.mock import AsyncMock, MagicMock
from app.core.event_bus import EventBus, Event


@pytest.mark.asyncio
async def test_invoice_confirmed_triggers_stock_update():
    """Тест: підтвердження накладної оновлює залишки через Event Bus."""
    
    # Створюємо Event Bus
    event_bus = EventBus()
    
    # Створюємо мок для StockModule
    stock_handler = AsyncMock()
    event_bus.subscribe("invoice.confirmed", stock_handler)
    
    # Публікуємо подію
    await event_bus.publish(Event(
        event_type="invoice.confirmed",
        source_module="document",
        payload={
            "invoice_id": "123e4567-e89b-12d3-a456-426614174000",
            "items": [
                {
                    "product_id": "123e4567-e89b-12d3-a456-426614174001",
                    "quantity": 10.0,
                }
            ],
        }
    ))
    
    # Перевіряємо, що обробник викликано
    stock_handler.handle.assert_called_once()
    
    # Перевіряємо payload
    call_args = stock_handler.handle.call_args[0][0]
    assert call_args.event_type == "invoice.confirmed"
    assert call_args.payload["items"][0]["quantity"] == 10.0


@pytest.mark.asyncio
async def test_multiple_subscribers():
    """Тест: декілька підписників отримують одну подію."""
    
    event_bus = EventBus()
    
    handler1 = AsyncMock()
    handler2 = AsyncMock()
    
    event_bus.subscribe("invoice.confirmed", handler1)
    event_bus.subscribe("invoice.confirmed", handler2)
    
    await event_bus.publish(Event(
        event_type="invoice.confirmed",
        source_module="document",
        payload={}
    ))
    
    handler1.handle.assert_called_once()
    handler2.handle.assert_called_once()


@pytest.mark.asyncio
async def test_event_history():
    """Тест: історія подій зберігається."""
    
    event_bus = EventBus()
    
    await event_bus.publish(Event(
        event_type="invoice.confirmed",
        source_module="document",
        payload={"id": 1}
    ))
    await event_bus.publish(Event(
        event_type="stock.changed",
        source_module="stock",
        payload={"id": 2}
    ))
    
    assert len(event_bus.get_history()) == 2
    assert len(event_bus.get_history("invoice.confirmed")) == 1
    assert len(event_bus.get_history("stock.changed")) == 1
```
