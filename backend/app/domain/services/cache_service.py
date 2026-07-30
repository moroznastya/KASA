"""
Domain Service Interface: ICacheService (Protocol).

Визначає контракт для сервісу кешування в Domain Layer.
Реалізація знаходиться в Infrastructure Layer (redis_cache.py).

Правила Clean Architecture:
- Domain Layer визначає ТІЛЬКИ інтерфейс (Protocol)
- Жодних залежностей від Redis, aioredis чи інших бібліотек
- Інфраструктурна реалізація імплементує цей Protocol
"""

from __future__ import annotations

from typing import Any, Optional, Protocol


class ICacheService(Protocol):
    """
    Інтерфейс сервісу кешування.

    Визначає методи для роботи з кешем:
    - get / set / delete — базові операції
    - exists — перевірка наявності ключа
    - clear_pattern — інвалідація за патерном (наприклад, "products:*")
    - close — graceful shutdown (закриття з'єднання)

    Кеш має бути прозорим:
    - Якщо Redis недоступний, методи повертають None / False без ексепшену
    - Логування помилок підключення (debug рівень)
    """

    async def get(self, key: str) -> Optional[Any]:
        """
        Отримати значення з кешу.

        Args:
            key: Ключ кешу (наприклад, "product:uuid").

        Returns:
            Значення, якщо ключ знайдено, інакше None.
            При помилці підключення повертає None.
        """
        ...

    async def set(
        self,
        key: str,
        value: Any,
        ttl: Optional[int] = None,
    ) -> bool:
        """
        Зберегти значення в кеш.

        Args:
            key: Ключ кешу.
            value: Значення (серіалізується в JSON).
            ttl: Час життя в секундах. Якщо None — використовується TTL за замовчуванням.

        Returns:
            True якщо успішно збережено, False при помилці.
        """
        ...

    async def delete(self, key: str) -> bool:
        """
        Видалити значення з кешу.

        Args:
            key: Ключ кешу.

        Returns:
            True якщо ключ було видалено, False якщо ключа не існувало.
        """
        ...

    async def exists(self, key: str) -> bool:
        """
        Перевірити, чи існує ключ в кеші.

        Args:
            key: Ключ кешу.

        Returns:
            True якщо ключ існує, False якщо ні або при помилці.
        """
        ...

    async def clear_pattern(self, pattern: str) -> int:
        """
        Інвалідувати всі ключі, що відповідають патерну.

        Використовується для масової інвалідації:
        - clear_pattern("product:*") — всі кеші продуктів
        - clear_pattern("category:*") — всі кеші категорій
        - clear_pattern("products:list*") — всі списки продуктів

        Args:
            pattern: Патерн для пошуку ключів (наприклад, "product:*").

        Returns:
            Кількість видалених ключів.
        """
        ...

    async def close(self) -> None:
        """
        Закрити з'єднання з Redis (graceful shutdown).

        Викликається при зупинці застосунку (lifespan).
        """
        ...
