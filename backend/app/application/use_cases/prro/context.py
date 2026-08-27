"""
Application Layer: PrroContextFactory — збірка компонентів ПРРО з налаштувань.

Єдине місце, де з поточних налаштувань (PrroSettingsRepository + PrroKeyStore)
створюються:
  - XmlBuilder           (реквізити ПРРО: FN/TN/ZN);
  - PrroCryptoSigner     (шлях/пароль ключа з key_store);
  - PrroGrpcClient       (через PrroServiceFactory, кеш за url+rro_fn);
  - prro_pb2.Check       (повідомлення для sendChkV2).

Лічильники DI/NT (унікальність пакетів у межах ПРРО) зберігаються
в налаштуваннях (last_packet_id, last_mac_number) — це гарантує
унікальність DI після перезапуску застосунку.
"""

from __future__ import annotations

import logging
from datetime import datetime
from typing import TYPE_CHECKING

from app.config import settings as app_settings

from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.key_store import PrroKeyStore
from app.infrastructure.services.prro.xml_builder import XmlBuilder

# Ключі налаштувань ПРРО
KEY_PRRO_FN = "prro_fn"
KEY_PRRO_TN = "prro_tn"
KEY_PRRO_ZN = "prro_zn"
KEY_PRRO_MODE = "mode"
KEY_PRRO_URL = "url"
KEY_LAST_SHIFT_NUMBER = "last_shift_number"
KEY_LAST_PACKET_ID = "last_packet_id"
KEY_LAST_MAC_NUMBER = "last_mac_number"
KEY_AUTO_FISCALIZE = "auto_fiscalize"
KEY_PRRO_STUB_MODE = "prro_stub_mode"

# Типи чеків (збігаються з enum Check.Type у prro.proto)
CHECK_TYPE_CHK = "CHK"
CHECK_TYPE_ZREPORT = "ZREPORT"
CHECK_TYPE_SERVICECHK = "SERVICECHK"

# Зіставлення типу → значення enum для prro_pb2.Check.Type
_PRRO_CHECK_TYPE_MAP: dict[str, int] = {
    CHECK_TYPE_CHK: 1,          # Check.CHK
    CHECK_TYPE_ZREPORT: 2,      # Check.ZREPORT
    CHECK_TYPE_SERVICECHK: 3,   # Check.SERVICECHK
}

if TYPE_CHECKING:
    from app.infrastructure.services.prro.grpc_client import PrroGrpcClient
    from app.infrastructure.services.prro.factory import PrroServiceFactory

logger = logging.getLogger(__name__)


class PrroContextFactory:
    """
    Збирає компоненти ПРРО з поточних налаштувань.

    Args:
        settings_repo: репозиторій налаштувань ПРРО (PrroSetting).
        key_store: сховище шляху/пароля ключа КЕП.
        service_factory: фабрика gRPC-клієнтів (PrroServiceFactory).
        config: об'єкт налаштувань застосунку (за замовчуванням app.config.settings).
    """

    def __init__(
        self,
        settings_repo: PrroSettingsRepository,
        key_store: PrroKeyStore,
        service_factory: "PrroServiceFactory",
        config=None,
    ) -> None:
        self._settings_repo = settings_repo
        self._key_store = key_store
        self._service_factory = service_factory
        self._config = config or app_settings

    # ─── Режим та URL ──────────────────────────────────────────────────────

    async def mode(self) -> str:
        """Поточний режим ПРРО: 'test' або 'prod'."""
        stored = await self._settings_repo.get(KEY_PRRO_MODE)
        return stored or self._config.PRRO_MODE

    async def url(self) -> str:
        """URL фіскального сервера залежно від режиму."""
        mode = await self.mode()
        return self._config.PRRO_PROD_URL if mode == "prod" else self._config.PRRO_TEST_URL

    # ─── gRPC-клієнт ───────────────────────────────────────────────────────

    async def grpc_client(self) -> "PrroGrpcClient":
        """Повертає кешованого PrroGrpcClient (url + rro_fn з налаштувань).

        B3: формує rro_fn_sign — підпис фіскального номера ПРРО тим самим
        КЕП-ключем (statusRro/infoRro/lastChk/delLastChk/delLastChkId).
        Якщо ключ не налаштовано — rro_fn_sign=b'' (виклик усе одно піде,
        але тестовий сервер відкине запит; статус налаштувань покаже це).
        """
        url = await self.url()
        fn = await self._settings_repo.get(KEY_PRRO_FN) or None
        rro_fn_sign = None
        if fn:
            try:
                signer = await self.build_crypto_signer()
                rro_fn_sign = signer.sign(fn.encode("utf-8"))
            except Exception as exc:  # noqa: BLE001 — ключ може бути не налаштований
                logger.warning("PRRO_RRO_FN_SIGN | не вдалося підписати ФН: %s", exc)
                rro_fn_sign = None
        return self._service_factory.grpc_client(
            url=url, rro_fn=fn, rro_fn_sign=rro_fn_sign
        )

    # ─── XmlBuilder / CryptoSigner ─────────────────────────────────────────

    async def build_xml_builder(self) -> XmlBuilder:
        """
        Створює XmlBuilder з реквізитами ПРРО та останніми лічильниками DI/NT.

        Returns:
            XmlBuilder — готовий білдер XML СЗЗД 2.1.7.
        """
        keys = await self._settings_repo.get_many(
            [KEY_PRRO_FN, KEY_PRRO_TN, KEY_PRRO_ZN,
             KEY_LAST_PACKET_ID, KEY_LAST_MAC_NUMBER]
        )
        try:
            initial_packet_id = int(keys.get(KEY_LAST_PACKET_ID) or "0")
        except (TypeError, ValueError):
            initial_packet_id = 0
        try:
            initial_mac_number = int(keys.get(KEY_LAST_MAC_NUMBER) or "0")
        except (TypeError, ValueError):
            initial_mac_number = 0

        return XmlBuilder(
            rro_fn=keys.get(KEY_PRRO_FN) or "",
            tax_number=keys.get(KEY_PRRO_TN) or "",
            factory_number=keys.get(KEY_PRRO_ZN) or "",
            initial_packet_id=initial_packet_id,
            initial_mac_number=initial_mac_number,
        )

    async def build_crypto_signer(self):
        """
        Створює PrroCryptoSigner з ключа КЕП (шлях/пароль з key_store).

        Returns:
            PrroCryptoSigner — підписант XAdES.

        Raises:
            PrroKeyStoreError: якщо ключ або пароль не налаштовано.
            PrroCryptoError: якщо файл ключа не знайдено/не валідний.
        """
        from app.infrastructure.services.prro.crypto_signer import PrroCryptoSigner

        key_path = self._key_store.get_key_path()
        password = self._key_store.decrypt_password()
        key_format = self._key_store.get_key_format()
        return PrroCryptoSigner(
            key_path=key_path,
            key_password=password,
            key_format=key_format,
        )

    async def persist_builder_counters(self, xml_builder: XmlBuilder) -> None:
        """
        Зберігає останні лічильники DI/NT у налаштування (для унікальності).

        Args:
            xml_builder: XmlBuilder, що щойно сформував документ.
        """
        await self._settings_repo.set(KEY_LAST_PACKET_ID, str(xml_builder.last_packet_id))
        await self._settings_repo.set(KEY_LAST_MAC_NUMBER, str(xml_builder.last_mac_number))

    # ─── Налаштованість ПРРО ───────────────────────────────────────────────

    def check_configured(self) -> tuple[bool, str]:
        """
        Перевіряє, чи ПРРО налаштований для фіскалізації (ключ КЕП + пароль).

        Returns:
            (ok, reason): ok=True якщо ключ збережено; інакше (False, причина).
        """
        try:
            key_path = self._key_store.get_key_path()
            has_password = self._key_store.is_configured()
        except Exception as exc:  # noqa: BLE001
            return False, f"ключ КЕП недоступний: {exc}"
        if not key_path:
            return False, "ключ КЕП не збережено"
        if not has_password:
            return False, "пароль ключа КЕП не збережено"
        return True, ""

    # ─── Повідомлення Check ────────────────────────────────────────────────

    async def build_check(
        self,
        check_sign: bytes,
        local_number: int,
        check_type: str = CHECK_TYPE_CHK,
        id_offline: str = "",
    ):
        """
        Формує prro_pb2.Check для sendChkV2.

        Args:
            check_sign: підписаний XML-документ СЗЗД (bytes).
            local_number: локальний номер чеку (0 — відкриття зміни).
            check_type: "CHK" / "ZREPORT" / "SERVICECHK".
            id_offline: B4 — офлайн-ідентифікатор ("offline-{n}" в офлайні).

        Returns:
            prro_pb2.Check — готове повідомлення.
        """
        from app.infrastructure.services.prro import prro_pb2

        fn = await self._settings_repo.get(KEY_PRRO_FN) or ""
        return prro_pb2.Check(
            rro_fn=fn,
            date_time=int(datetime.utcnow().timestamp()),
            check_sign=check_sign,
            local_number=int(local_number),
            check_type=_PRRO_CHECK_TYPE_MAP.get(
                check_type, _PRRO_CHECK_TYPE_MAP[CHECK_TYPE_CHK]
            ),
            id_offline=id_offline,
        )

    # ─── Лічильник змін ────────────────────────────────────────────────────

    async def next_shift_number(self) -> int:
        """Повертає наступний номер зміни (last_shift_number + 1)."""
        last_raw = await self._settings_repo.get(KEY_LAST_SHIFT_NUMBER)
        try:
            last = int(last_raw or "0")
        except (TypeError, ValueError):
            last = 0
        return last + 1

    async def save_last_shift_number(self, shift_number: int) -> None:
        """Зберігає номер останньої зміни."""
        await self._settings_repo.set(KEY_LAST_SHIFT_NUMBER, str(shift_number))


__all__ = [
    "PrroContextFactory",
    "KEY_PRRO_FN",
    "KEY_PRRO_TN",
    "KEY_PRRO_ZN",
    "KEY_PRRO_MODE",
    "KEY_PRRO_URL",
    "KEY_LAST_SHIFT_NUMBER",
    "KEY_LAST_PACKET_ID",
    "KEY_LAST_MAC_NUMBER",
    "KEY_AUTO_FISCALIZE",
    "KEY_PRRO_STUB_MODE",
    "CHECK_TYPE_CHK",
    "CHECK_TYPE_ZREPORT",
    "CHECK_TYPE_SERVICECHK",
]
