"""
Спільні Pydantic схеми для пагінованих відповідей та інших утиліт.
"""

from typing import Generic, TypeVar

from pydantic import BaseModel, computed_field

T = TypeVar("T")


class PaginatedResponse(BaseModel, Generic[T]):
    """
    Базова схема для пагінованої відповіді.

    Використовується для всіх list endpoints, що повертають
    велику кількість записів.

    Типовий приклад:
    ```json
    {
      "items": [...],
      "total": 100,
      "page": 1,
      "page_size": 20,
      "pages": 5
    }
    ```
    """
    items: list[T]
    total: int
    page: int
    page_size: int
    pages: int

    @computed_field
    @property
    def has_next(self) -> bool:
        """Чи є наступна сторінка."""
        return self.page < self.pages

    @computed_field
    @property
    def has_prev(self) -> bool:
        """Чи є попередня сторінка."""
        return self.page > 1
