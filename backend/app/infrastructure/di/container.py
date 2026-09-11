"""
Infrastructure Layer: DIContainer — контейнер Dependency Injection.

Реєструє фабрики для створення сервісів та керує їх життєвим циклом.
Підтримує singleton та transient (щоразу новий екземпляр).
"""

from __future__ import annotations

import logging
from collections.abc import Callable
from typing import Any, TypeVar

logger = logging.getLogger(__name__)

T = TypeVar("T")

# Тип для фабрики сервісу
ServiceFactory = Callable[["DIContainer"], Any]


class DIContainer:
    """
    Контейнер Dependency Injection.

    Реєструє фабрики для створення сервісів та керує їх життєвим циклом.

    Підтримує:
    - Singleton: один екземпляр на весь час життя контейнера
    - Transient: новий екземпляр при кожному resolve()

    Приклад використання:
        container = DIContainer()
        container.register("product_repository", lambda c: ProductRepository(session))
        container.register("product_service", lambda c: ProductService(
            repo=c.resolve("product_repository"),
            event_bus=c.resolve("event_bus"),
        ), singleton=True)
        service = container.resolve("product_service")
    """

    def __init__(self) -> None:
        """Ініціалізує порожній контейнер."""
        self._factories: dict[str, ServiceFactory] = {}
        self._singletons: dict[str, Any] = {}
        self._singleton_flags: dict[str, bool] = {}

    # ─── Реєстрація ─────────────────────────────────────────────────────────

    def register(
        self,
        name: str,
        factory: ServiceFactory,
        singleton: bool = False,
    ) -> None:
        """
        Реєструє фабрику для сервісу.

        Args:
            name: Унікальне ім'я сервісу (наприклад, "product_service").
            factory: Фабрика, яка приймає DIContainer та повертає екземпляр.
            singleton: Якщо True — створюється один екземпляр при першому resolve().

        Raises:
            ValueError: Якщо сервіс з таким ім'ям вже зареєстровано.
        """
        if name in self._factories:
            raise ValueError(f"Service '{name}' is already registered")

        self._factories[name] = factory
        self._singleton_flags[name] = singleton

        logger.debug(
            f"Registered service '{name}' "
            f"({'singleton' if singleton else 'transient'})"
        )

    def register_instance(self, name: str, instance: Any) -> None:
        """
        Реєструє вже готовий екземпляр як singleton.

        Args:
            name: Унікальне ім'я сервісу.
            instance: Готовий екземпляр сервісу.

        Raises:
            ValueError: Якщо сервіс з таким ім'ям вже зареєстровано.
        """
        if name in self._factories:
            raise ValueError(f"Service '{name}' is already registered")

        self._singletons[name] = instance
        self._singleton_flags[name] = True

        logger.debug(f"Registered instance '{name}' as singleton")

    # ─── Розв'язання залежностей ────────────────────────────────────────────

    def resolve(self, name: str) -> Any:
        """
        Отримує екземпляр сервісу за ім'ям.

        Для singleton повертає закешований екземпляр.
        Для transient створює новий екземпляр при кожному виклику.

        Args:
            name: Ім'я сервісу.

        Returns:
            Екземпляр сервісу.

        Raises:
            KeyError: Якщо сервіс не зареєстровано.
        """
        # Перевіряємо, чи є вже створений singleton
        if name in self._singletons:
            return self._singletons[name]

        # Отримуємо фабрику
        factory = self._factories.get(name)
        if factory is None:
            raise KeyError(
                f"Service '{name}' is not registered. "
                f"Available services: {', '.join(sorted(self._factories.keys()))}"
            )

        # Створюємо екземпляр
        instance = factory(self)

        # Якщо singleton — кешуємо
        if self._singleton_flags.get(name, False):
            self._singletons[name] = instance
            logger.debug(f"Created singleton instance for '{name}'")

        return instance

    # ─── Допоміжні методи ───────────────────────────────────────────────────

    def has(self, name: str) -> bool:
        """
        Перевіряє, чи зареєстровано сервіс.

        Args:
            name: Ім'я сервісу.

        Returns:
            True якщо сервіс зареєстровано.
        """
        return name in self._factories or name in self._singletons

    def remove(self, name: str) -> None:
        """
        Видаляє сервіс з контейнера.

        Args:
            name: Ім'я сервісу.
        """
        self._factories.pop(name, None)
        self._singletons.pop(name, None)
        self._singleton_flags.pop(name, None)

    def clear(self) -> None:
        """Очищає всі зареєстровані сервіси."""
        self._factories.clear()
        self._singletons.clear()
        self._singleton_flags.clear()

    @property
    def registered_services(self) -> list[str]:
        """Список імен зареєстрованих сервісів."""
        return sorted(set(self._factories.keys()) | set(self._singletons.keys()))

    @property
    def singleton_count(self) -> int:
        """Кількість закешованих singleton-екземплярів."""
        return len(self._singletons)

    def __contains__(self, name: str) -> bool:
        """Перевіряє, чи зареєстровано сервіс (оператор 'in')."""
        return self.has(name)
