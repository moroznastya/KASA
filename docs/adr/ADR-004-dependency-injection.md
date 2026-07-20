# ADR-004: Dependency Injection Container

| Метадані | Значення |
|----------|----------|
| **Статус** | ✅ Прийнято |
| **Дата** | 2026-07-20 |
| **Автор** | System Architect Agent (AEGIS v3) |
| **Версія** | 1.0.0 |

---

## Контекст

Поточна архітектура створює залежності вручну в кожному роутері:

```python
# Поточний підхід (ручне створення)
@router.get("/products/{id}")
async def get_product(
    product_id: UUID,
    session: AsyncSession = Depends(get_session),
    current_user = Depends(AuthService.get_current_user),
):
    service = ProductService(session)  # Ручне створення
    product = await service.get_product_by_id(product_id)
    return ProductResponse.model_validate(product)
```

**Проблеми:**
1. **Дублювання коду:** Кожен роутер створює сервіси вручну
2. **Важка заміна реалізацій:** Потрібно змінювати код в багатьох місцях
3. **Відсутність централізованого управління:** Не видно всі залежності в одному місці
4. **Порушення DI принципу:** Класи самі створюють свої залежності

## Рішення

Впровадити централізований DI контейнер:

```python
# infrastructure/di/container.py
from functools import lru_cache
from app.domain.repositories.i_product_repository import IProductRepository
from app.infrastructure.persistence.repositories.product_repository import ProductRepository
from app.application.use_cases.product_use_case import ProductUseCase

class DIContainer:
    def __init__(self, session_factory):
        self._session_factory = session_factory
        self._instances = {}

    async def get_session(self):
        async with self._session_factory() as session:
            yield session

    @property
    def product_repository(self) -> IProductRepository:
        return ProductRepository(self._get_session())

    @property
    def product_use_case(self) -> ProductUseCase:
        return ProductUseCase(
            product_repo=self.product_repository,
            unit_of_work=self.unit_of_work,
            event_bus=self.event_bus,
        )

    @property
    def unit_of_work(self):
        return UnitOfWork(self._get_session())

    @property
    def event_bus(self):
        return LocalEventBus()

# Використання в роутері
@router.get("/products/{id}")
async def get_product(
    product_id: UUID,
    container: DIContainer = Depends(get_container),
    current_user = Depends(AuthService.get_current_user),
):
    use_case = container.product_use_case
    product = await use_case.get_product(product_id)
    return ProductResponse.model_validate(product)
```

## FastAPI Depends інтеграція

```python
# infrastructure/di/fastapi_di.py
from fastapi import Request, Depends
from app.infrastructure.di.container import DIContainer

def get_container(request: Request) -> DIContainer:
    return request.app.state.container

# В main.py
from app.infrastructure.di.container import DIContainer
from app.infrastructure.di.modules import register_modules

app.state.container = DIContainer()
register_modules(app.state.container)
```

## Модульна реєстрація

```python
# infrastructure/di/modules.py
def register_modules(container: DIContainer):
    # Repositories
    container.register(IProductRepository, ProductRepository)
    container.register(IInvoiceRepository, InvoiceRepository)
    container.register(IReceiptRepository, ReceiptRepository)
    container.register(IUserRepository, UserRepository)
    container.register(ISupplierRepository, SupplierRepository)
    container.register(ICategoryRepository, CategoryRepository)

    # Use Cases
    container.register(ProductUseCase)
    container.register(InvoiceUseCase)
    container.register(ReceiptUseCase)
    container.register(AuthUseCase)

    # Services
    container.register(StockService)
    container.register(PricingService)
    container.register(TaxService)

    # Infrastructure
    container.register(IEventBus, LocalEventBus, singleton=True)
    container.register(IUnitOfWork, UnitOfWork)
```

## Обґрунтування

1. **Централізоване управління:** Всі залежності в одному місці
2. **Легка заміна:** Достатньо змінити реєстрацію в контейнері
3. **Тестування:** Можна підставити mock-об'єкти
4. **Singleton управління:** EventBus, Cache — єдині екземпляри

## Альтернативи

| Альтернатива | За | Проти |
|-------------|-----|-------|
| **FastAPI Depends (поточний)** | Простота | Немає централізації |
| **Manual DI (вручну)** | Прозорість | Багато коду |
| **Third-party (dependency-injector)** | Потужний | Зайва залежність |
| **Custom container (наш вибір)** | Контроль, простота | Потрібно писати самому |

## Наслідки

**Позитивні:**
- ✅ Єдина точка конфігурації залежностей
- ✅ Легке тестування (підміна реалізацій)
- ✅ Чистий код роутерів
- ✅ Singleton для спільних об'єктів

**Негативні:**
- ❌ Додатковий код контейнера
- ❌ Складніше відстежити створення об'єкта
- ❌ Можливі проблеми з memory leaks (при неправильному налаштуванні)

---

> **Пов'язані ADR:** ADR-001 (4-шарова архітектура), ADR-002 (Repository Pattern)
