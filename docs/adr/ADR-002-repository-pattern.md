# ADR-002: Repository Pattern

| Метадані | Значення |
|----------|----------|
| **Статус** | ✅ Прийнято |
| **Дата** | 2026-07-20 |
| **Автор** | System Architect Agent (AEGIS v3) |
| **Версія** | 1.0.0 |

---

## Контекст

Поточна архітектура використовує сервіси (`ProductService`, `DocumentService`), які напряму працюють з SQLAlchemy сесією:

```python
# Поточний підхід (проблемний)
class ProductService:
    def __init__(self, session: AsyncSession):
        self.session = session  # Пряма залежність від SQLAlchemy

    async def get_product_by_id(self, product_id: UUID) -> Product:
        result = await self.session.execute(
            select(Product).where(Product.id == product_id)
        )
        # ...
```

**Проблеми:**
1. Сервіси жорстко прив'язані до SQLAlchemy
2. Неможливо тестувати бізнес-логіку без БД
3. При зміні ORM потрібно змінювати всі сервіси
4. Дублювання коду запитів між сервісами

## Рішення

Впровадити Repository Pattern з чітким розділенням інтерфейсу (Domain) та реалізації (Infrastructure):

```python
# Domain Layer (інтерфейс)
class IProductRepository(ABC):
    @abstractmethod
    async def save(self, product: Product) -> Product: ...

    @abstractmethod
    async def find_by_id(self, id: UUID) -> Product | None: ...

    @abstractmethod
    async def find_by_barcode(self, barcode: str) -> Product | None: ...

    @abstractmethod
    async def search(self, params: SearchParams) -> tuple[list[Product], int]: ...

    @abstractmethod
    async def delete(self, id: UUID) -> None: ...

# Infrastructure Layer (реалізація)
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

    # ... реалізація інших методів
```

## Обґрунтування

1. **Абстракція доступу до даних:** Use Cases не знають, як саме зберігаються дані
2. **Тестування:** Можна створити `MockProductRepository` для unit-тестів
3. **Гнучкість:** Легко змінити БД (PostgreSQL → SQLite для тестів)
4. **Єдина точка змін:** Всі SQL запити в одному місці

## Структура репозиторіїв

```
domain/repositories/
├── i_product_repository.py
├── i_invoice_repository.py
├── i_receipt_repository.py
├── i_user_repository.py
├── i_supplier_repository.py
├── i_category_repository.py
├── i_transfer_repository.py
├── i_write_off_repository.py
├── i_return_invoice_repository.py
└── i_ledger_repository.py

infrastructure/persistence/repositories/
├── product_repository.py
├── invoice_repository.py
├── receipt_repository.py
├── user_repository.py
├── supplier_repository.py
├── category_repository.py
├── transfer_repository.py
├── write_off_repository.py
├── return_invoice_repository.py
└── ledger_repository.py
```

## Маппінг Entity ↔ ORM Model

Кожен репозиторій містить методи маппінгу:

```python
class ProductRepository(IProductRepository):
    def _to_domain(self, model: ProductModel) -> Product:
        return Product(
            id=model.id,
            barcode=Barcode(model.barcode) if model.barcode else None,
            sku=model.sku,
            title=model.title,
            price=Money(model.price, "UAH") if model.price else None,
            stock=Quantity(model.stock, model.unit) if model.stock else None,
            # ...
        )

    def _to_orm(self, entity: Product) -> ProductModel:
        return ProductModel(
            id=entity.id,
            barcode=str(entity.barcode) if entity.barcode else None,
            # ...
        )
```

## Наслідки

**Позитивні:**
- ✅ Чисте розділення Domain та Infrastructure
- ✅ Можливість unit-тестування Use Cases
- ✅ Легка заміна ORM
- ✅ Стандартизований доступ до даних

**Негативні:**
- ❌ Додатковий код маппінгу Entity ↔ ORM
- ❌ Потрібно підтримувати два набори моделей
- ❌ Можливе дублювання полів

---

> **Пов'язані ADR:** ADR-001 (4-шарова архітектура), ADR-004 (DI Container)
