"""Product list query: category_id='' (порожній рядок) → None, не 422.

Фронтенд шле '' замість null/undefined, коли категорія не вибрана.
Раніше UUID-тип у query-параметрі давав 422 uuid_parsing.
"""

from uuid import UUID

import pytest
from fastapi import HTTPException

from app.api.v1.products import _uuid_or_none as v1_uuid_or_none
from app.api.v2.products import _uuid_or_none as v2_uuid_or_none

UID = "3fa85f64-5717-4562-b3fc-2c963f66afa6"


def test_empty_string_means_none_v1():
    """GET /api/v1/products?category_id= → category_id=None."""
    assert v1_uuid_or_none("", "category_id") is None


def test_empty_string_means_none_v2():
    """GET /api/v2/products?category_id= → category_id=None."""
    assert v2_uuid_or_none("", "category_id") is None


def test_none_means_none_v1():
    assert v1_uuid_or_none(None, "category_id") is None


def test_none_means_none_v2():
    assert v2_uuid_or_none(None, "category_id") is None


def test_valid_uuid_parsed_v1():
    assert v1_uuid_or_none(UID, "category_id") == UUID(UID)


def test_valid_uuid_parsed_v2():
    assert v2_uuid_or_none(UID, "category_id") == UUID(UID)


def test_invalid_string_400_v1():
    with pytest.raises(HTTPException) as exc:
        v1_uuid_or_none("not-a-uuid", "category_id")
    assert exc.value.status_code == 400
    assert "category_id" in exc.value.detail


def test_invalid_string_400_v2():
    with pytest.raises(HTTPException) as exc:
        v2_uuid_or_none("not-a-uuid", "category_id")
    assert exc.value.status_code == 400
    assert "category_id" in exc.value.detail
