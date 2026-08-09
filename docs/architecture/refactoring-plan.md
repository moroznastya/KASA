# 📋 План рефакторингу архітектури Torgashka

> **Версія:** 2.0.0  
> **Дата:** 2026-07-20  
> **Автор:** System Architect Agent (AEGIS v3)  
> **Оцінка:** ~80-120 людино-годин (2-3 тижні для 1 розробника)

---

## 1️⃣ ЗВІТ ПРО АНАЛІЗ ПОТОЧНОЇ АРХІТЕКТУРИ

### Сильні сторони (що залишаємо)

| Аспект | Оцінка | Коментар |
|--------|--------|----------|
| **Async SQLAlchemy 2.0** | ⭐⭐⭐⭐⭐ | Сучасний async підхід, правильне використання |
| **Pydantic v2 схеми** | ⭐⭐⭐⭐⭐ | Валідація, серіалізація, документація |
| **Чітке API v1** | ⭐⭐⭐⭐ | RESTful, добре задокументовано |
| **JWT авторизація** | ⭐⭐⭐⭐ | Bearer токени, PIN-код, ролі |
| **Міграції Alembic** | ⭐⭐⭐⭐ | Версіонування схеми БД |
| **Frontend структура** | ⭐⭐⭐⭐ | Чіткий поділ на компоненти/сторінки |
| **Zustand store** | ⭐⭐⭐⭐ | Легкий, передбачуваний state management |
| **TypeScript типи** | ⭐⭐⭐⭐ | Повна типізація API відповідей |

### Слабкі місця (що потребує змін)

| Аспект | Проблема | Ступінь |
|--------|----------|---------|
| **Domain шар відсутній** | Бізнес-логіка в сервісах з SQLAlchemy | 🔴 Критично |
| **Repository Pattern** | Пряма робота з session в сервісах | 🔴 Критично |
| **Dependency Rule** | Сервіси імпортують моделі (SQLAlchemy) | 🔴 Критично |
| **Use Cases** | Логіка змішана з HTTP (роутери) | 🟡 Важливо |
| **Value Objects** | Примітивна одержимість (float, str) | 🟡 Важливо |
| **Domain Events** | Жорстка зв'язність модулів | 🟡 Важливо |
| **DI Container** | Ручне створення залежностей | 🟢 Добре мати |
| **Тестування** | Відсутні unit-тести (тільки інтеграційні) | 🟢 Добре мати |

### Порушення Dependency Rule (детально)

```
❌ api/v1/products.py → services/product_service.py → models/product.py (SQLAlchemy)
❌ services/document_service.py → services/product_service.py (жорстка залежність)
❌ services/document_service.py → services/ledger_service.py (жорстка залежність)
❌ services/auth_service.py → models/user.py (SQLAlchemy)
❌ services/*.py → database.py (прямий імпорт сесії)
```

---

## 2️⃣ ПОКРОКОВИЙ ПЛАН РЕФАКТОРИНГУ

### Фаза 0: Підготовка (2-4 години)

**Мета:** Створити структуру директорій, налаштувати інструменти.

```
Крок 0.1: Створити директорії
├── app/domain/entities/
├── app/domain/value_objects/
├── app/domain/events/
├── app/domain/services/
├── app/domain/repositories/
├── app/application/use_cases/
├── app/application/dto/
├── app/application/mappers/
├── app/application/interfaces/
├── app/infrastructure/persistence/repositories/
├── app/infrastructure/persistence/models/    (← з app/models/)
├── app/infrastructure/di/
├── app/infrastructure/event_bus/
├── app/infrastructure/external/
└── tests/
    ├── unit/
    ├── integration/
    └── e2e/

Крок 0.2: Налаштувати pytest + pytest-asyncio
Крок 0.3: Додати __init__.py з правильними експортами
```

---

### Фаза 1: Domain Layer (16-24 години)

**Мета:** Створити чисті Domain Entities, Value Objects, Repository Interfaces.

#### Крок 1.1: Value Objects (4 години)

```python
# Створити файли:
app/domain/value_objects/__init__.py
app/domain/value_objects/money.py          # Money(amount, currency)
app/domain/value_objects/barcode.py        # Barcode(value, type)
app/domain/value_objects/quantity.py       # Quantity(value, unit)
app/domain/value_objects/tax_rate.py       # TaxRate(rate)
app/domain/value_objects/ukr_tax_id.py     # UkrTaxId(value)
app/domain/value_objects/address.py        # Address(street, city, zip)
app/domain/value_objects/phone.py          # PhoneNumber(value)
app/domain/value_objects/edrpou.py         # EdrpouCode(value)
```

**Тести:** `tests/unit/domain/value_objects/test_money.py` та ін.

#### Крок 1.2: Domain Entities (8 годин)

```python
# Створити файли:
app/domain/entities/__init__.py
app/domain/entities/product.py             # Product aggregate root
app/domain/entities/invoice.py             # Invoice aggregate root
app/domain/entities/invoice_item.py        # InvoiceItem entity
app/domain/entities/receipt.py             # Receipt aggregate root
app/domain/entities/receipt_item.py        # ReceiptItem entity
app/domain/entities/user.py                # User entity
app/domain/entities/supplier.py            # Supplier entity
app/domain/entities/category.py            # Category entity
app/domain/entities/transfer.py            # Transfer aggregate root
app/domain/entities/transfer_item.py       # TransferItem entity
app/domain/entities/write_off.py           # WriteOff aggregate root
app/domain/entities/write_off_item.py      # WriteOffItem entity
app/domain/entities/return_invoice.py      # ReturnInvoice aggregate root
app/domain/entities/return_invoice_item.py # ReturnInvoiceItem entity
app/domain/entities/supplier_ledger.py     # SupplierLedger entity
```

**Ключові методи entity (приклад Product):**
```python
class Product:
    def update_stock(self, quantity: Quantity) -> None
    def change_price(self, new_price: Money) -> None
    def is_low_stock(self, threshold: Quantity) -> bool
    def apply_tax(self) -> Money
```

**Тести:** `tests/unit/domain/entities/test_product.py` та ін.

#### Крок 1.3: Domain Events (2 години)

```python
# Створити файли:
app/domain/events/__init__.py
app/domain/events/base_event.py            # DomainEvent base class
app/domain/events/product_events.py        # ProductCreated, StockChanged
app/domain/events/invoice_events.py        # InvoiceConfirmed, InvoiceCancelled
app/domain/events/receipt_events.py        # ReceiptCreated
app/domain/events/ledger_events.py         # SupplierBalanceChanged
```

#### Крок 1.4: Repository Interfaces (2 години)

```python
# Створити файли:
app/domain/repositories/__init__.py
app/domain/repositories/i_product_repository.py
app/domain/repositories/i_invoice_repository.py
app/domain/repositories/i_receipt_repository.py
app/domain/repositories/i_user_repository.py
app/domain/repositories/i_supplier_repository.py
app/domain/repositories/i_category_repository.py
app/domain/repositories/i_transfer_repository.py
app/domain/repositories/i_write_off_repository.py
app/domain/repositories/i_return_invoice_repository.py
app/domain/repositories/i_ledger_repository.py
```

#### Крок 1.5: Domain Services (2 години)

```python
# Створити файли:
app/domain/services/__init__.py
app/domain/services/pricing_service.py     # Розрахунок цін, ПДВ
app/domain/services/stock_service.py       # Логіка залишків
app/domain/services/tax_service.py         # Податкові розрахунки
app/domain/services/document_numbering.py  # Генерація номерів
```

---

### Фаза 2: Application Layer (16-20 годин)

**Мета:** Створити Use Cases, DTO, Mappers.

#### Крок 2.1: DTO (Data Transfer Objects) (4 години)

```python
# Перенести/створити файли:
app/application/dto/__init__.py
app/application/dto/product_dto.py         # ProductCreateDTO, ProductUpdateDTO, ProductResponseDTO
app/application/dto/invoice_dto.py         # InvoiceCreateDTO, InvoiceConfirmDTO
app/application/dto/receipt_dto.py         # ReceiptCreateDTO, ReceiptResponseDTO
app/application/dto/user_dto.py            # UserCreateDTO, LoginDTO
app/application/dto/supplier_dto.py        # SupplierCreateDTO
app/application/dto/category_dto.py        # CategoryCreateDTO
app/application/dto/ledger_dto.py          # LedgerEntryDTO, PaymentDTO
app/application/dto/search_params.py       # SearchParamsDTO (загальний)
```

**Примітка:** DTO — це нові Pydantic моделі, окремі від `app/schemas/`. 
Схеми в `app/schemas/` залишаються для API відповідей (Response models).

#### Крок 2.2: Mappers (4 години)

```python
# Створити файли:
app/application/mappers/__init__.py
app/application/mappers/product_mapper.py  # Entity ↔ DTO
app/application/mappers/invoice_mapper.py
app/application/mappers/receipt_mapper.py
app/application/mappers/user_mapper.py
app/application/mappers/supplier_mapper.py
app/application/mappers/category_mapper.py
```

#### Крок 2.3: Use Cases (8 годин)

```python
# Створити файли:
app/application/use_cases/__init__.py
app/application/use_cases/product_use_case.py
app/application/use_cases/invoice_use_case.py
app/application/use_cases/receipt_use_case.py
app/application/use_cases/auth_use_case.py
app/application/use_cases/user_use_case.py
app/application/use_cases/supplier_use_case.py
app/application/use_cases/category_use_case.py
app/application/use_cases/transfer_use_case.py
app/application/use_cases/write_off_use_case.py
app/application/use_cases/return_invoice_use_case.py
app/application/use_cases/ledger_use_case.py
app/application/use_cases/document_use_case.py
```

**Приклад Use Case:**
```python
class ProductUseCase:
    def __init__(
        self,
        product_repo: IProductRepository,
        unit_of_work: IUnitOfWork,
        event_bus: IEventBus,
    ):
        self._product_repo = product_repo
        self._uow = unit_of_work
        self._event_bus = event_bus

    async def create_product(self, dto: ProductCreateDTO) -> ProductResponseDTO:
        # 1. Маппінг DTO → Entity
        product = ProductMapper.dto_to_entity(dto)

        # 2. Бізнес-валідація
        existing = await self._product_repo.find_by_barcode(product.barcode)
        if existing:
            raise BusinessError("Barcode already exists")

        # 3. Збереження
        async with self._uow:
            saved = await self._product_repo.save(product)
            await self._uow.commit()

        # 4. Подія
        await self._event_bus.publish(ProductCreated(product_id=saved.id))

        # 5. Відповідь
        return ProductMapper.entity_to_dto(saved)
```

#### Крок 2.4: Application Interfaces (2 години)

```python
# Створити файли:
app/application/interfaces/__init__.py
app/application/interfaces/unit_of_work.py  # IUnitOfWork
app/application/interfaces/event_bus.py     # IEventBus
```

---

### Фаза 3: Infrastructure Layer (16-20 годин)

**Мета:** Реалізувати репозиторії, DI контейнер, Event Bus.

#### Крок 3.1: ORM Models (перейменування) (2 години)

```python
# Перемістити з app/models/ → app/infrastructure/persistence/models/
app/infrastructure/persistence/models/__init__.py
app/infrastructure/persistence/models/product_model.py     # ← з app/models/product.py
app/infrastructure/persistence/models/invoice_model.py     # ← з app/models/invoice.py
app/infrastructure/persistence/models/receipt_model.py     # ← з app/models/receipt.py
app/infrastructure/persistence/models/user_model.py        # ← з app/models/user.py
app/infrastructure/persistence/models/supplier_model.py    # ← з app/models/supplier.py
app/infrastructure/persistence/models/category_model.py    # ← з app/models/category.py
app/infrastructure/persistence/models/transfer_model.py    # ← з app/models/transfer.py
app/infrastructure/persistence/models/write_off_model.py   # ← з app/models/write_off.py
app/infrastructure/persistence/models/return_invoice_model.py
app/infrastructure/persistence/models/supplier_ledger_model.py
app/infrastructure/persistence/models/barcode_model.py     # ← з app/models/barcode.py
app/infrastructure/persistence/models/product_image_model.py
```

**Важливо:** Додати префікс `Model` до назв класів (наприклад, `Product` → `ProductModel`), щоб уникнути конфлікту з Domain Entity.

#### Крок 3.2: Repository Implementations (8 годин)

```python
# Створити файли:
app/infrastructure/persistence/repositories/__init__.py
app/infrastructure/persistence/repositories/product_repository.py
app/infrastructure/persistence/repositories/invoice_repository.py
app/infrastructure/persistence/repositories/receipt_repository.py
app/infrastructure/persistence/repositories/user_repository.py
app/infrastructure/persistence/repositories/supplier_repository.py
app/infrastructure/persistence/repositories/category_repository.py
app/infrastructure/persistence/repositories/transfer_repository.py
app/infrastructure/persistence/repositories/write_off_repository.py
app/infrastructure/persistence/repositories/return_invoice_repository.py
app/infrastructure/persistence/repositories/ledger_repository.py
```

**Ключові методи реалізації:**
```python
class ProductRepository(IProductRepository):
    def __init__(self, session: AsyncSession):
        self._session = session

    async def save(self, product: Product) -> Product:
        model = self._to_orm(product)
        self._session.add(model)
        await self._session.flush()
        return self._to_domain(model)

    async def find_by_id(self, id: UUID) -> Product | None:
        result = await self._session.execute(
            select(ProductModel).where(ProductModel.id == id)
        )
        model = result.scalar_one_or_none()
        return self._to_domain(model) if model else None

    def _to_domain(self, model: ProductModel) -> Product:
        return Product(
            id=model.id,
            barcode=Barcode(model.barcode) if model.barcode else None,
            sku=model.sku,
            title=model.title,
            price=Money(model.price, "UAH") if model.price else None,
            stock=Quantity(model.stock, model.unit) if model.stock else None,
            # ... інші поля
        )

    def _to_orm(self, entity: Product) -> ProductModel:
        return ProductModel(
            id=entity.id,
            barcode=str(entity.barcode) if entity.barcode else None,
            sku=entity.sku,
            title=entity.title,
            price=float(entity.price.amount) if entity.price else None,
            stock=float(entity.stock.value) if entity.stock else None,
            # ... інші поля
        )
```

#### Крок 3.3: Unit of Work (2 години)

```python
# app/infrastructure/persistence/unit_of_work.py
class UnitOfWork(IUnitOfWork):
    def __init__(self, session_factory):
        self._session_factory = session_factory
        self._session: AsyncSession | None = None

    async def __aenter__(self):
        self._session = self._session_factory()
        return self

    async def __aexit__(self, *args):
        if self._session:
            await self._session.close()

    async def commit(self):
        await self._session.commit()

    async def rollback(self):
        await self._session.rollback()
```

#### Крок 3.4: Event Bus (2 години)

```python
# app/infrastructure/event_bus/local_event_bus.py
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

#### Крок 3.5: DI Container (2 години)

```python
# app/infrastructure/di/container.py
class DIContainer:
    def __init__(self):
        self._factories: dict = {}
        self._instances: dict = {}

    def register(self, interface: type, implementation: type, **kwargs):
        self._factories[interface] = (implementation, kwargs)

    def resolve(self, interface: type):
        if interface in self._instances:
            return self._instances[interface]
        impl, kwargs = self._factories[interface]
        instance = impl(**kwargs)
        if kwargs.get("singleton"):
            self._instances[interface] = instance
        return instance

# app/infrastructure/di/modules.py
def register_modules(container: DIContainer):
    # Repositories
    container.register(IProductRepository, ProductRepository)
    container.register(IInvoiceRepository, InvoiceRepository)
    # ... інші репозиторії

    # Use Cases
    container.register(ProductUseCase)
    container.register(InvoiceUseCase)
    # ... інші Use Cases

    # Infrastructure
    container.register(IEventBus, LocalEventBus, singleton=True)
```

#### Крок 3.6: Оновлення database.py (2 години)

```python
# app/database.py (оновлений)
# Додати session_factory для UnitOfWork
session_factory = async_sessionmaker(
    bind=engine,
    class_=AsyncSession,
    expire_on_commit=False,
)
```

---

### Фаза 4: Presentation Layer (8-12 годин)

**Мета:** Оновити API роутери для роботи через Use Cases.

#### Крок 4.1: Оновлення роутерів (8 годин)

```python
# app/api/v1/products.py (оновлений)
from app.application.use_cases.product_use_case import ProductUseCase
from app.application.dto.product_dto import ProductCreateDTO, ProductSearchParamsDTO

router = APIRouter(prefix="/products", tags=["Товари"])

@router.get("", response_model=ProductListResponse)
async def list_products(
    params: ProductSearchParamsDTO = Depends(),
    use_case: ProductUseCase = Depends(get_product_use_case),
    current_user = Depends(AuthService.get_current_user),
):
    result = await use_case.search_products(params)
    return ProductListResponse(
        items=[ProductResponse.model_validate(p) for p in result.items],
        total=result.total,
        page=params.page,
        size=params.size,
    )

@router.post("", response_model=ProductResponse, status_code=201)
async def create_product(
    data: ProductCreate,
    use_case: ProductUseCase = Depends(get_product_use_case),
    current_user = Depends(AuthService.get_current_user),
):
    dto = ProductCreateDTO(**data.model_dump())
    result = await use_case.create_product(dto)
    return ProductResponse.model_validate(result)
```

#### Крок 4.2: Оновлення main.py (2 години)

```python
# app/main.py (оновлений)
from app.infrastructure.di.container import DIContainer
from app.infrastructure.di.modules import register_modules

@asynccontextmanager
async def lifespan(app: FastAPI):
    container = DIContainer()
    register_modules(container)
    app.state.container = container
    yield
```

#### Крок 4.3: Оновлення middleware (2 години)

```python
# app/middleware/auth_middleware.py (оновлений)
# Використовує AuthUseCase замість прямого імпорту User моделі
```

---

### Фаза 5: Тестування (16-20 годин)

**Мета:** Написати unit-тести для Domain та Application шарів.

#### Крок 5.1: Unit-тести Domain (6 годин)

```
tests/unit/domain/
├── entities/
│   ├── test_product.py          # Тести Product entity
│   ├── test_invoice.py          # Тести Invoice aggregate
│   └── test_receipt.py          # Тести Receipt aggregate
├── value_objects/
│   ├── test_money.py            # Тести Money VO
│   ├── test_barcode.py          # Тести Barcode VO
│   └── test_quantity.py         # Тести Quantity VO
└── services/
    ├── test_pricing_service.py  # Тести ціноутворення
    └── test_stock_service.py    # Тести залишків
```

#### Крок 5.2: Unit-тести Application (6 годин)

```
tests/unit/application/
├── use_cases/
│   ├── test_product_use_case.py    # Mock репозиторію
│   ├── test_invoice_use_case.py    # Mock репозиторію
│   └── test_auth_use_case.py       # Mock репозиторію
└── mappers/
    ├── test_product_mapper.py      # Entity ↔ DTO
    └── test_invoice_mapper.py      # Entity ↔ DTO
```

#### Крок 5.3: Інтеграційні тести (4 години)

```
tests/integration/
├── repositories/
│   ├── test_product_repository.py  # Тест з реальною БД (test DB)
│   └── test_invoice_repository.py  # Тест з реальною БД
└── api/
    ├── test_products_api.py        # HTTP тести
    └── test_invoices_api.py        # HTTP тести
```

#### Крок 5.4: Налаштування тестової інфраструктури (4 години)

```python
# tests/conftest.py
@pytest.fixture
async def in_memory_db():
    engine = create_async_engine("sqlite+aiosqlite:///:memory:")
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
    yield engine
    await engine.dispose()

@pytest.fixture
def mock_product_repo():
    return AsyncMock(spec=IProductRepository)

@pytest.fixture
def product_use_case(mock_product_repo):
    return ProductUseCase(
        product_repo=mock_product_repo,
        unit_of_work=AsyncMock(),
        event_bus=AsyncMock(),
    )
```

---

### Фаза 6: Міграція та деплой (4-8 годин)

**Мета:** Забезпечити плавний перехід без простою.

#### Крок 6.1: Стратегія міграції

```
Фаза 0: Підготовка (структура, тести)
    ↓
Фаза 1: Domain Layer (нові файли, старі працюють)
    ↓
Фаза 2: Application Layer (нові файли, старі працюють)
    ↓
Фаза 3: Infrastructure Layer (нові файли + старі моделі)
    ↓
Фаза 4: Presentation Layer (переключення на нові Use Cases)
    ↓
Фаза 5: Видалення старого коду (services/, старі моделі)
    ↓
Фаза 6: Тестування та деплой
```

#### Крок 6.2: Поетапне переключення

1. **Тиждень 1:** Domain + Application (нове, паралельно зі старим)
2. **Тиждень 2:** Infrastructure + Presentation (переключення)
3. **Тиждень 3:** Тестування, видалення старого коду, деплой

#### Крок 6.3: Валідація

```bash
# Перевірка, що старий код не використовується
grep -r "from app.models" app/api/  # Має бути 0
grep -r "from app.services" app/api/  # Має бути 0
grep -r "SQLAlchemy" app/domain/  # Має бути 0
grep -r "FastAPI" app/domain/  # Має бути 0
```

---

## 3️⃣ ОЦІНКА ОБСЯГУ РОБІТ

| Фаза | Години | Файли (нові) | Файли (змінені) | Файли (видалені) |
|------|--------|-------------|-----------------|------------------|
| **0: Підготовка** | 2-4 | 10 | 3 | 0 |
| **1: Domain** | 16-24 | 45 | 0 | 0 |
| **2: Application** | 16-20 | 35 | 0 | 0 |
| **3: Infrastructure** | 16-20 | 25 | 15 | 12 |
| **4: Presentation** | 8-12 | 0 | 13 | 0 |
| **5: Тестування** | 16-20 | 30 | 2 | 0 |
| **6: Міграція** | 4-8 | 0 | 5 | 12 |
| **Всього** | **78-108** | **~145** | **~38** | **~24** |

### Розподіл за типами

| Тип роботи | Години | % |
|-----------|--------|---|
| Написання нового коду | 50-70 | 60% |
| Рефакторинг існуючого | 15-20 | 20% |
| Тестування | 16-20 | 15% |
| Документація | 4-6 | 5% |

### Рекомендація

**Для 1 розробника:** 2-3 тижні full-time  
**Для 2 розробників:** 1-2 тижні (паралельно Domain + Infrastructure)  
**Для команди (3+):** 1 тиждень (розподіл по модулях)

---

## 4️⃣ РИЗИКИ ТА ПОМ'ЯКШЕННЯ

| Ризик | Ймовірність | Вплив | Пом'якшення |
|-------|------------|-------|-------------|
| **Регресія функціональності** | Висока | Критичний | Поетапна міграція, тести |
| **Конфлікти імпортів** | Середня | Середній | Чіткі імена (Model suffix) |
| **Продуктивність** | Низька | Низький | Маппінг може додати overhead |
| **Збільшення кодової бази** | Висока | Низький | Чиста архітектура варта того |
| **Опір команди** | Середня | Середній | Документація, ADR, code review |

---

## 5️⃣ ЧЕКЛИСТ ГОТОВНОСТІ

- [ ] Всі Domain Entities мають unit-тести
- [ ] Всі Value Objects мають валідацію
- [ ] Всі Repository Interfaces визначені
- [ ] Всі Use Cases покриті тестами (mock)
- [ ] DI Container налаштований
- [ ] Event Bus працює
- [ ] API роутери використовують Use Cases
- [ ] Старі сервіси видалені
- [ ] Інтеграційні тести проходять
- [ ] Документація оновлена

---

> **Документ створено:** System Architect Agent (AEGIS v3)  
> **Останнє оновлення:** 2026-07-20
