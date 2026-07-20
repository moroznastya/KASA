# ADR-005: Value Objects

| Метадані | Значення |
|----------|----------|
| **Статус** | ✅ Прийнято |
| **Дата** | 2026-07-20 |
| **Автор** | System Architect Agent (AEGIS v3) |
| **Версія** | 1.0.0 |

---

## Контекст

Поточна архітектура використовує примітивні типи для фінансових та бізнес-значень:

```python
# Поточний підхід (примітивна одержимість)
class Product(Base):
    price: float = mapped_column(Numeric(10, 2))  # float без контексту
    stock: float = mapped_column(Numeric(10, 3))  # float без одиниць виміру
    barcode: str = mapped_column(String(50))       # str без валідації
```

**Проблеми:**
1. **Відсутність валідації:** Будь-який рядок може стати штрих-кодом
2. **Змішування валют:** Немає інформації про валюту ціни
3. **Помилки округлення:** float для грошей — небезпечно
4. **Відсутність семантики:** `price` і `cost_price` — просто числа

## Рішення

Впровадити Value Objects для ключових бізнес-типів:

```python
# domain/value_objects/money.py
from dataclasses import dataclass
from decimal import Decimal

@dataclass(frozen=True)
class Money:
    amount: Decimal
    currency: str = "UAH"

    def __post_init__(self):
        if self.amount < 0:
            raise ValueError("Amount cannot be negative")
        if self.amount.as_tuple().exponent < -2:
            raise ValueError("Amount cannot have more than 2 decimal places")

    def __add__(self, other: "Money") -> "Money":
        if self.currency != other.currency:
            raise ValueError("Cannot add different currencies")
        return Money(self.amount + other.amount, self.currency)

    def __sub__(self, other: "Money") -> "Money":
        if self.currency != other.currency:
            raise ValueError("Cannot subtract different currencies")
        return Money(self.amount - other.amount, self.currency)

    def __mul__(self, factor: Decimal) -> "Money":
        return Money((self.amount * factor).quantize(Decimal("0.01")), self.currency)

    def is_positive(self) -> bool:
        return self.amount > 0

    def is_zero(self) -> bool:
        return self.amount == 0

# domain/value_objects/barcode.py
@dataclass(frozen=True)
class Barcode:
    value: str
    type: BarcodeType = BarcodeType.EAN13

    def __post_init__(self):
        if not self._validate():
            raise ValueError(f"Invalid barcode: {self.value}")

    def _validate(self) -> bool:
        if self.type == BarcodeType.EAN13:
            return len(self.value) == 13 and self.value.isdigit()
        elif self.type == BarcodeType.CODE128:
            return len(self.value) <= 50
        return True

# domain/value_objects/quantity.py
@dataclass(frozen=True)
class Quantity:
    value: Decimal
    unit: str = "шт"

    def __post_init__(self):
        if self.value < 0:
            raise ValueError("Quantity cannot be negative")

    def __add__(self, other: "Quantity") -> "Quantity":
        if self.unit != other.unit:
            raise ValueError("Cannot add different units")
        return Quantity(self.value + other.value, self.unit)

    def is_zero(self) -> bool:
        return self.value == 0

# domain/value_objects/tax_rate.py
@dataclass(frozen=True)
class TaxRate:
    rate: Decimal  # 0.20 for 20%

    def __post_init__(self):
        if not Decimal("0") <= self.rate <= Decimal("1"):
            raise ValueError("Tax rate must be between 0 and 1")

    def apply_to(self, money: Money) -> Money:
        return money * self.rate
```

## Список Value Objects

| Value Object | Поля | Валідація |
|-------------|------|-----------|
| `Money` | amount, currency | ≥ 0, max 2 decimal places |
| `Barcode` | value, type | EAN-13: 13 digits, Code128: ≤ 50 chars |
| `Quantity` | value, unit | ≥ 0 |
| `TaxRate` | rate | 0.0 – 1.0 |
| `UkrTaxId` | value | 10 or 12 digits (УКТЗЕД) |
| `Address` | street, city, zip | Обов'язкові поля |
| `PhoneNumber` | value | +380 format |
| `EdrpouCode` | value | 8 or 10 digits (ЄДРПОУ) |

## Обґрунтування

1. **Безпека типів:** Неможливо передати ціну замість кількості
2. **Вбудована валідація:** Неможливо створити невалідний об'єкт
3. **Самодокументованість:** Код говорить сам за себе
4. **Бізнес-логіка:** Методи (`apply_tax`, `add`) інкапсульовані в VO

## Наслідки

**Позитивні:**
- ✅ Безпека типів на рівні компіляції
- ✅ Валідація при створенні (fail fast)
- ✅ Зрозумілий код
- ✅ Легке тестування

**Негативні:**
- ❌ Більше коду для простих полів
- ❌ Потрібен маппінг при роботі з ORM
- ❌ Frozen dataclasses — не можна змінити після створення

---

> **Пов'язані ADR:** ADR-001 (4-шарова архітектура), ADR-002 (Repository Pattern)
