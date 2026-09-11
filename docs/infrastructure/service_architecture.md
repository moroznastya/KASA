# Архітектура сервісів Torgashka

**Версія:** 1.0.0  
**Дата:** 2025-01-20  
**Статус:** Проєкт (Contract First)

---

## 1. Аналіз поточної інфраструктури

### 1.1 Як зараз організована взаємодія

Поточна архітектура kasa/ — це **класичний моноліт FastAPI** з наступними шарами:

```
HTTP Request
    │
    ▼
API Router (api/v1/*.py)
    │  Валідація через Pydantic схеми
    │  Depends(get_session) — отримання сесії БД
    │  Depends(AuthService.get_current_user) — перевірка авторизації
    ▼
Service Layer (services/*.py)
    │  Бізнес-логіка
    │  Прямі виклики інших сервісів (new ProductService(session))
    │  Прямі SQLAlchemy запити
    ▼
Models (models/*.py)
    │  SQLAlchemy ORM моделі
    ▼
Database (PostgreSQL via asyncpg)
```

### 1.2 Виявлені проблеми

| # | Проблема | Опис | Серйозність |
|---|----------|------|-------------|
| 1 | **Пряма залежність сервісів** | `DocumentService` створює `ProductService(session)` та `LedgerService(session)` напряму — жорстке зв'язування | 🔴 Висока |
| 2 | **Відсутність інтерфейсів** | Немає абстракцій/Protocols — сервіси залежать від конкретних реалізацій | 🔴 Висока |
| 3 | **Немає Event Bus** | При підтвердженні накладної `DocumentService` напряму викликає `ProductService.update_stock()` — синхронно і жорстко | 🟠 Середня |
| 4 | **Немає Service Registry** | Кожен сервіс створюється вручну в кожному роутері: `ProductService(session)` | 🟠 Середня |
| 5 | **Немає Context Provider** | Сервіси не мають доступу до контексту системи (хто викликав, який tenant, які права) | 🟡 Низька |
| 6 | **Дублювання створення сервісів** | В кожному ендпоінті: `service = ProductService(session)` — створюється новий екземпляр | 🟡 Низька |
| 7 | **Немає асинхронних подій** | Всі операції синхронні — немає механізму для фонових задач (наприклад, надіслати email після створення накладної) | 🟡 Низька |

### 1.3 Типовий потік зараз

```
POST /api/v1/invoices/{id}/confirm
    │
    ▼
invoices.py (роутер)
    │  service = DocumentService(session)
    │  await service.confirm_invoice(invoice_id)
    │
    ▼
DocumentService.confirm_invoice()
    │  1. SELECT invoice WHERE id = ...
    │  2. Перевірка статусу (DRAFT?)
    │  3. for item in invoice.items:
    │        self.product_service.update_stock(item.product_id, +item.quantity)
    │  4. self.ledger_service.create_ledger_entry(...)
    │  5. invoice.status = CONFIRMED
    │  6. flush()
    │
    ▼
Відповідь: Invoice (JSON)
```

**Проблема:** `DocumentService` жорстко залежить від `ProductService` та `LedgerService`.  
Якщо додати новий модуль (наприклад, `NotificationService`), потрібно змінювати код `DocumentService`.

---

## 2. Цільова архітектура (за зразком AEGIS v3)

### 2.1 Принципи

1. **Модулі НЕ знають про внутрішню реалізацію один одного** — тільки через інтерфейси (Protocols)
2. **Події — єдиний спосіб асинхронної комунікації** між модулями
3. **Кожен модуль ізольований і тестований незалежно**
4. **Contract First** — усі інтерфейси визначені до реалізації
5. **Dependency Injection** — сервіси отримують залежності через конструктор

### 2.2 Нова структура

```
kasa/backend/app/
├── api/v1/                    # API роутери (без змін)
├── middleware/                 # Middleware (без змін)
├── models/                    # SQLAlchemy моделі (без змін)
├── schemas/                   # Pydantic DTO (без змін)
├── core/                      # НОВЕ: ядро системи
│   ├── config.py              # Конфігурація (була)
│   ├── database.py            # Підключення до БД (було)
│   ├── exceptions.py          # Кастомні винятки
│   ├── event_bus.py           # НОВЕ: шина подій
│   ├── service_registry.py    # НОВЕ: реєстр сервісів
│   ├── context_provider.py    # НОВЕ: контекст системи
│   └── di_container.py        # НОВЕ: DI контейнер
├── contracts/                 # НОВЕ: контракти (Protocols)
│   ├── product_contract.py
│   ├── stock_contract.py
│   ├── document_contract.py
│   ├── ledger_contract.py
│   └── auth_contract.py
├── services/                  # Сервіси (оновлені)
│   ├── product_service.py
│   ├── document_service.py
│   ├── ledger_service.py
│   └── auth_service.py
└── main.py                    # Точка входу (оновлена)
```

### 2.3 Нова архітектура взаємодії

```
HTTP Request
    │
    ▼
API Router
    │  Валідація через Pydantic схеми
    │  Отримання сервісу через DI Container
    ▼
Service Layer
    │  Бізнес-логіка
    │  Виклики через Protocols (інтерфейси)
    │  Публікація подій через Event Bus
    │  Читання контексту через Context Provider
    ▼
Event Bus ─────────► Інші модулі (асинхронно)
    │
    ▼
Models → Database
```

---

## 3. Компоненти цільової інфраструктури

### 3.1 Event Bus (Шина подій)

**Призначення:** Центральна шина для асинхронної комунікації між модулями.

```python
# core/event_bus.py

from typing import Protocol, Any, Callable, Dict, List
from dataclasses import dataclass
from datetime import datetime
from uuid import uuid4


@dataclass
class Event:
    """Базовий клас для всіх подій системи."""
    event_id: str = ""
    event_type: str = ""
    timestamp: datetime = None
    payload: Dict[str, Any] = None
    source_module: str = ""
    
    def __post_init__(self):
        if not self.event_id:
            self.event_id = str(uuid4())
        if not self.timestamp:
            self.timestamp = datetime.utcnow()


class EventHandler(Protocol):
    """Протокол обробника подій."""
    async def handle(self, event: Event) -> None: ...


class EventBus:
    """
    Центральна шина подій.
    
    Модулі публікують події та підписуються на події інших модулів.
    """
    
    def __init__(self):
        self._handlers: Dict[str, List[EventHandler]] = {}
        self._history: List[Event] = []
    
    def subscribe(self, event_type: str, handler: EventHandler) -> None:
        """Підписати обробник на тип події."""
        if event_type not in self._handlers:
            self._handlers[event_type] = []
        self._handlers[event_type].append(handler)
    
    def unsubscribe(self, event_type: str, handler: EventHandler) -> None:
        """Відписати обробник."""
        if event_type in self._handlers:
            self._handlers[event_type].remove(handler)
    
    async def publish(self, event: Event) -> None:
        """Опублікувати подію — сповістити всіх підписників."""
        self._history.append(event)
        handlers = self._handlers.get(event.event_type, [])
        for handler in handlers:
            await handler.handle(event)
    
    def get_history(self, event_type: str = None) -> List[Event]:
        """Отримати історію подій (для аудиту)."""
        if event_type:
            return [e for e in self._history if e.event_type == event_type]
        return self._history
```

### 3.2 Service Registry (Реєстр сервісів)

**Призначення:** Центральний реєстр всіх сервісів/модулів системи.

```python
# core/service_registry.py

from typing import Dict, Any, Optional, Type
from dataclasses import dataclass, field


@dataclass
class ServiceInfo:
    """Інформація про зареєстрований сервіс."""
    name: str
    version: str
    description: str
    dependencies: list = field(default_factory=list)
    events_publishes: list = field(default_factory=list)
    events_subscribes: list = field(default_factory=list)
    status: str = "inactive"  # inactive | active | degraded | error


class ServiceRegistry:
    """
    Реєстр сервісів/модулів системи.
    
    Дозволяє:
    - Реєструвати нові сервіси
    - Отримувати інформацію про сервіс
    - Перевіряти залежності
    - Відстежувати статус сервісів
    """
    
    def __init__(self):
        self._services: Dict[str, ServiceInfo] = {}
        self._instances: Dict[str, Any] = {}
    
    def register(self, service: ServiceInfo, instance: Any = None) -> None:
        """Зареєструвати сервіс."""
        self._services[service.name] = service
        if instance:
            self._instances[service.name] = instance
    
    def get_info(self, name: str) -> Optional[ServiceInfo]:
        """Отримати інформацію про сервіс."""
        return self._services.get(name)
    
    def get_instance(self, name: str) -> Optional[Any]:
        """Отримати екземпляр сервісу."""
        return self._instances.get(name)
    
    def set_status(self, name: str, status: str) -> None:
        """Оновити статус сервісу."""
        if name in self._services:
            self._services[name].status = status
    
    def list_services(self) -> Dict[str, ServiceInfo]:
        """Отримати список всіх сервісів."""
        return dict(self._services)
    
    def check_dependencies(self, name: str) -> bool:
        """Перевірити, чи всі залежності сервісу зареєстровані."""
        info = self._services.get(name)
        if not info:
            return False
        for dep in info.dependencies:
            if dep not in self._services:
                return False
        return True
```

### 3.3 Context Provider (Провайдер контексту)

**Призначення:** Надає контекстну інформацію кожному модулю.

```python
# core/context_provider.py

from typing import Optional, Dict, Any
from dataclasses import dataclass, field
from uuid import UUID


@dataclass
class SystemContext:
    """Контекст системи для поточного запиту."""
    # Інформація про користувача
    user_id: Optional[UUID] = None
    user_role: Optional[str] = None
    user_login: Optional[str] = None
    
    # Інформація про запит
    request_id: str = ""
    request_path: str = ""
    request_method: str = ""
    
    # Інформація про систему
    app_name: str = "Torgashka"
    app_version: str = "1.0.0"
    
    # Метadata
    metadata: Dict[str, Any] = field(default_factory=dict)


class ContextProvider:
    """
    Провайдер контексту системи.
    
    Дозволяє модулям отримувати інформацію про:
    - Поточного користувача
    - Поточний запит
    - Стан системи
    """
    
    def __init__(self):
        self._context: Optional[SystemContext] = None
    
    def set_context(self, context: SystemContext) -> None:
        """Встановити контекст для поточного запиту."""
        self._context = context
    
    def get_context(self) -> Optional[SystemContext]:
        """Отримати поточний контекст."""
        return self._context
    
    def clear_context(self) -> None:
        """Очистити контекст після запиту."""
        self._context = None
    
    @property
    def current_user_id(self) -> Optional[UUID]:
        """Отримати ID поточного користувача."""
        return self._context.user_id if self._context else None
    
    @property
    def current_user_role(self) -> Optional[str]:
        """Отримати роль поточного користувача."""
        return self._context.user_role if self._context else None
```

### 3.4 DI Container (Контейнер залежностей)

**Призначення:** Централізоване керування залежностями сервісів.

```python
# core/di_container.py

from typing import Dict, Any, Type, Callable, Optional


class DIContainer:
    """
    Контейнер Dependency Injection.
    
    Реєструє фабрики для створення сервісів та керує їх життєвим циклом.
    """
    
    def __init__(self):
        self._factories: Dict[str, Callable] = {}
        self._singletons: Dict[str, Any] = {}
        self._services: Dict[str, bool] = {}  # True = singleton
    
    def register(self, name: str, factory: Callable, singleton: bool = False) -> None:
        """Зареєструвати фабрику для сервісу."""
        self._factories[name] = factory
        self._services[name] = singleton
    
    def resolve(self, name: str) -> Any:
        """Отримати екземпляр сервісу."""
        if name in self._singletons:
            return self._singletons[name]
        
        factory = self._factories.get(name)
        if not factory:
            raise KeyError(f"Service '{name}' not registered")
        
        instance = factory(self)
        
        if self._services.get(name, False):
            self._singletons[name] = instance
        
        return instance
    
    def has(self, name: str) -> bool:
        """Перевірити, чи зареєстровано сервіс."""
        return name in self._factories
```

---

## 4. Новий потік даних (цільовий)

```
POST /api/v1/invoices/{id}/confirm
    │
    ▼
invoices.py (роутер)
    │  Отримуємо сервіс через DI Container
    │  document_service = container.resolve("document_service")
    │  await document_service.confirm_invoice(invoice_id, context)
    │
    ▼
DocumentService.confirm_invoice()
    │  1. Перевірка статусу
    │  2. for item in invoice.items:
    │        # Через Event Bus, а не прямий виклик!
    │        await event_bus.publish(Event(
    │            event_type="stock.quantity_changed",
    │            payload={"product_id": item.product_id, "change": +item.quantity}
    │        ))
    │  3. await event_bus.publish(Event(
    │        event_type="ledger.entry_created",
    │        payload={...}
    │    ))
    │  4. invoice.status = CONFIRMED
    │
    ▼
Event Bus
    │
    ├──► StockModule.handle_stock_change()
    │       Оновлює залишки товарів
    │
    ├──► LedgerModule.handle_ledger_entry()
    │       Створює запис у журналі
    │
    └──► NotificationModule.handle_invoice_confirmed()
            Надсилає сповіщення (асинхронно)
```

---

## 5. План міграції

### Фаза 1: Contract First (1-2 дні)
1. Створити `contracts/` з Protocols для всіх модулів
2. Визначити всі події в `event_catalog.md`
3. Створити `core/event_bus.py`, `core/service_registry.py`, `core/context_provider.py`, `core/di_container.py`

### Фаза 2: Інтеграція Event Bus (2-3 дні)
1. Додати Event Bus в `main.py` (lifespan)
2. Замінити прямі виклики `ProductService.update_stock()` на події
3. Додати обробники подій в кожен модуль

### Фаза 3: DI Container (1-2 дні)
1. Зареєструвати всі сервіси в DI Container
2. Оновити роутери для використання DI Container
3. Додати Context Provider в middleware

### Фаза 4: Тестування (1-2 дні)
1. Написати unit-тести для кожного модуля ізольовано
2. Написати integration-тести для Event Bus
3. Перевірити, що всі старі ендпоінти працюють
