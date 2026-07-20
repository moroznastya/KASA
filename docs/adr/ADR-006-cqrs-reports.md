# ADR-006: CQRS для звітів (опціонально)

| Метадані | Значення |
|----------|----------|
| **Статус** | ⏳ Відкладено (Sprint 3+) |
| **Дата** | 2026-07-20 |
| **Автор** | System Architect Agent (AEGIS v3) |
| **Версія** | 1.0.0 |

---

## Контекст

Звіти та аналітика потребують складних запитів, які об'єднують дані з різних модулів:

```sql
-- Приклад складного звіту: Продажі за період з собівартістю
SELECT
    p.title,
    SUM(ri.quantity) as qty,
    SUM(ri.total) as revenue,
    SUM(ri.quantity * p.cost_price) as cost,
    SUM(ri.total) - SUM(ri.quantity * p.cost_price) as profit
FROM receipt_items ri
JOIN products p ON p.id = ri.product_id
JOIN receipts r ON r.id = ri.receipt_id
WHERE r.created_at BETWEEN $1 AND $2
GROUP BY p.id, p.title
```

**Проблеми:**
1. Складні JOIN-запити навантажують основну БД
2. Звіти можуть блокувати транзакції запису
3. Важко оптимізувати читання без впливу на запис

## Рішення

Впровадити CQRS (Command Query Responsibility Segregation) для звітів:

```python
# 1. Materialized View (БД)
CREATE MATERIALIZED VIEW sales_report AS
SELECT
    p.id as product_id,
    p.title,
    DATE(r.created_at) as sale_date,
    SUM(ri.quantity) as quantity,
    SUM(ri.total) as revenue,
    SUM(ri.quantity * p.cost_price) as cost
FROM receipt_items ri
JOIN products p ON p.id = ri.product_id
JOIN receipts r ON r.id = ri.receipt_id
GROUP BY p.id, p.title, DATE(r.created_at);

CREATE UNIQUE INDEX ON sales_report (product_id, sale_date);

# 2. Read Model (Python)
class SalesReportReadModel:
    """Окрема модель для читання звітів."""
    product_id: UUID
    title: str
    sale_date: date
    quantity: Decimal
    revenue: Money
    cost: Money
    profit: Money

# 3. Query Handler
class GetSalesReportQuery:
    period_start: date
    period_end: date
    group_by: Literal["day", "week", "month"]

class SalesReportQueryHandler:
    def __init__(self, read_db: AsyncSession):
        self._db = read_db

    async def handle(self, query: GetSalesReportQuery) -> list[SalesReportReadModel]:
        # Читання з Materialized View або read-only репліки
        result = await self._db.execute(
            select(SalesReportReadModel).where(...)
        )
        return result.scalars().all()

# 4. Оновлення Materialized View (через події)
class SalesReportRefresher:
    def __init__(self, db):
        self._db = db

    async def on_receipt_created(self, event: ReceiptCreated):
        # Оновлення MV асинхронно
        await self._db.execute("REFRESH MATERIALIZED VIEW CONCURRENTLY sales_report")
```

## Коли використовувати

| Сценарій | CRUD (Command) | CQRS (Query) |
|----------|---------------|--------------|
| Створення товару | ✅ | ❌ |
| Пошук товарів | ✅ | ❌ |
| Дашборд продажів | ❌ | ✅ |
| Звіт прибутковості | ❌ | ✅ |
| Аналітика залишків | ❌ | ✅ |
| Історія операцій | ✅ | ❌ |

## Обґрунтування

1. **Продуктивність:** Складні запити не блокують операції запису
2. **Масштабування:** Можна мати окрему read-only репліку БД
3. **Оптимізація:** Materialized View можна оптимізувати незалежно
4. **Спрощення:** Команди стають простішими (без звітних JOIN)

## Наслідки

**Позитивні:**
- ✅ Вища продуктивність звітів
- ✅ Менше навантаження на основну БД
- ✅ Незалежне масштабування читання/запису

**Негативні:**
- ❌ Додаткова складність архітектури
- ❌ Eventual consistency (MV не real-time)
- ❌ Потрібно підтримувати додаткові моделі

---

> **Пов'язані ADR:** ADR-001 (4-шарова архітектура), ADR-003 (Domain Events)
