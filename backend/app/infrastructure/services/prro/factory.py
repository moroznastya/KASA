"""
Infrastructure Layer: PrroServiceFactory — фабрика gRPC-клієнтів ПРРО.

Кешує gRPC-канали та клієнти за (url, rro_fn):
  - один канал на URL (test/prod);
  - один PrroGrpcClient на (url, rro_fn) — клієнт зберігає rro_fn
    і підставляє його у всі повідомлення Check автоматично.

При зміні налаштувань ПРРО (url/rro_fn) створюється новий клієнт,
а старі канали закриваються при close().

Використання:
    factory = PrroServiceFactory()
    client = factory.grpc_client(url="cabinet.tax.gov.ua:9443", rro_fn="4538765845")
    ...
    await factory.close()
"""

from __future__ import annotations

import logging
from typing import Optional

import grpc
from grpc import aio

from app.config import settings
from app.infrastructure.services.prro.grpc_client import PrroGrpcClient

logger = logging.getLogger(__name__)


class PrroServiceFactory:
    """
    Кешує gRPC-канали та клієнтів ПРРО.

    Args:
        use_ssl: використовувати TLS-з'єднання (None — з config).
        config: об'єкт налаштувань (за замовчуванням app.config.settings).
    """

    def __init__(
        self,
        use_ssl: bool | None = None,
        config=None,
    ) -> None:
        self._config = config or settings
        self._use_ssl = (
            use_ssl if use_ssl is not None else bool(self._config.PRRO_USE_SSL)
        )
        self._channels: dict[str, aio.Channel] = {}
        self._clients: dict[tuple[str, str], PrroGrpcClient] = {}

    # ─── Створення каналу ──────────────────────────────────────────────────

    def _get_channel(self, url: str) -> aio.Channel:
        """Повертає кешований gRPC-канал для URL (створює при потребі)."""
        channel = self._channels.get(url)
        if channel is None:
            if self._use_ssl:
                channel = aio.secure_channel(
                    url, credentials=grpc.ssl_channel_credentials()
                )
            else:
                channel = aio.insecure_channel(url)
            self._channels[url] = channel
            logger.info("PRRO_FACTORY | канал створено: %s (ssl=%s)", url, self._use_ssl)
        return channel

    # ─── Клієнт ────────────────────────────────────────────────────────────

    def grpc_client(
        self,
        url: str,
        rro_fn: str | None = None,
        rro_fn_sign: bytes | None = None,
    ) -> PrroGrpcClient:
        """
        Повертає кешованого PrroGrpcClient для (url, rro_fn).

        Args:
            url: адреса фіскального сервера (host:port).
            rro_fn: фіскальний номер ПРРО (підставляється в Check).
            rro_fn_sign: B3 — підписаний ФН ПРРО (тим самим КЕП-ключем).

        Returns:
            PrroGrpcClient — клієнт з готовим каналом.
        """
        key = (url, rro_fn or "")
        client = self._clients.get(key)
        if client is None:
            channel = self._get_channel(url)
            client = PrroGrpcClient(channel, rro_fn=rro_fn, rro_fn_sign=rro_fn_sign)
            self._clients[key] = client
            logger.info(
                "PRRO_FACTORY | клієнт створено: url=%s rro_fn=%s", url, rro_fn,
            )
        return client

    # ─── Очищення ──────────────────────────────────────────────────────────

    async def close(self) -> None:
        """Закриває всі gRPC-канали та скидає кеш клієнтів."""
        for url, channel in list(self._channels.items()):
            try:
                await channel.close()
                logger.info("PRRO_FACTORY | канал закрито: %s", url)
            except Exception:  # noqa: BLE001
                logger.warning("PRRO_FACTORY | помилка закриття каналу %s", url)
        self._channels.clear()
        self._clients.clear()

    @property
    def active_channels(self) -> int:
        """Кількість активних gRPC-каналів (для моніторингу)."""
        return len(self._channels)


__all__ = ["PrroServiceFactory"]
