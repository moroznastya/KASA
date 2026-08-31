"""Unit-тести серіалізації RedisCacheService (Pydantic/dataclass/UUID/Decimal/datetime)."""

from dataclasses import dataclass
from datetime import datetime
from decimal import Decimal
from uuid import UUID, uuid4

from pydantic import BaseModel

from app.infrastructure.cache.redis_cache import RedisCacheService


class ItemDTO(BaseModel):
    id: UUID
    name: str
    price: Decimal
    created_at: datetime | None = None


class TestDefaultSerializer:
    def test_pydantic_model_to_dict(self):
        dto = ItemDTO(id=uuid4(), name="Хліб", price=Decimal("12.50"))
        result = RedisCacheService._default_serializer(dto)
        assert isinstance(result, dict)
        assert result["name"] == "Хліб"
        assert isinstance(result["id"], str)
        assert result["price"] == "12.50"

    def test_uuid_to_str(self):
        uid = uuid4()
        assert RedisCacheService._default_serializer(uid) == str(uid)

    def test_decimal_to_str(self):
        assert RedisCacheService._default_serializer(Decimal("9.99")) == "9.99"

    def test_datetime_to_iso(self):
        dt = datetime(2026, 8, 1, 12, 30, 0)
        assert RedisCacheService._default_serializer(dt) == "2026-08-01T12:30:00"

    def test_unknown_falls_back_to_str(self):
        assert RedisCacheService._default_serializer(42) == "42"

    def test_json_round_trip_with_pydantic(self):
        """json.dumps з _default_serializer має бути JSON-сумісним."""
        import json

        uid = uuid4()
        dto = ItemDTO(id=uid, name="Молоко", price=Decimal("28.40"))
        payload = {"items": [dto], "total": 1}
        serialized = json.dumps(payload, default=RedisCacheService._default_serializer)
        parsed = json.loads(serialized)
        assert parsed["items"][0]["id"] == str(uid)
        assert parsed["items"][0]["price"] == "28.40"
        assert parsed["items"][0]["name"] == "Молоко"

    def test_nested_pydantic_list(self):
        import json

        dtos = [ItemDTO(id=uuid4(), name=f"Товар {i}", price=Decimal(i)) for i in range(3)]
        serialized = json.dumps(dtos, default=RedisCacheService._default_serializer)
        parsed = json.loads(serialized)
        assert len(parsed) == 3
        assert all(isinstance(x["id"], str) for x in parsed)


@dataclass
class ProductDTO:
    """Dataclass (як ProductDTO у застосунку)."""
    id: UUID
    name: str
    price: Decimal | None = None
    stock: Decimal | None = None


class TestDataclassSerializer:
    def test_dataclass_to_dict(self):
        dto = ProductDTO(id=uuid4(), name="Хліб", price=Decimal("12.50"))
        result = RedisCacheService._default_serializer(dto)
        assert isinstance(result, dict)
        assert result["name"] == "Хліб"
        assert isinstance(result["id"], UUID), "значення залишаються сирими (обробляє json.dumps)"

    def test_json_round_trip_with_dataclass(self):
        """Кешування списку dataclass має давати dict-и (не рядки!)."""
        import json

        dtos = [ProductDTO(id=uuid4(), name=f"Т {i}", price=Decimal(i)) for i in range(2)]
        payload = {"items": dtos, "total": 2}
        serialized = json.dumps(payload, default=RedisCacheService._default_serializer)
        parsed = json.loads(serialized)
        assert isinstance(parsed["items"][0], dict), "items[0] має бути dict, не рядок"
        assert parsed["items"][0]["name"].startswith("Т ")
        assert parsed["items"][0]["price"] == "0" or parsed["items"][0]["price"] == "1"
        assert isinstance(parsed["items"][0]["id"], str)

    def test_nested_dataclass_in_dataclass(self):
        import json

        @dataclass
        class Wrapper:
            items: list

        w = Wrapper(items=[ProductDTO(id=uuid4(), name="Вкладений", price=Decimal("5"))])
        serialized = json.dumps(w, default=RedisCacheService._default_serializer)
        parsed = json.loads(serialized)
        assert isinstance(parsed["items"][0], dict)
