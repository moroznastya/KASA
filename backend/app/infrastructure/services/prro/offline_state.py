"""
Офлайн-режим ПРРО: переходи 109/110, резервні номери 112, id_offline.

Протокол [ДПС, 262576.docx]:
  - T=112 — запит діапазону резервних номерів:
        <C T="112"><H SIZE="150"></H></C>
    Відповідь сервера — перелік фіскальних номерів:
        <RS V="1"><C T="112"><ID>aaaaaaaaaa</ID>…</C></RS>
  - id_offline = фіскальний номер з отриманого діапазону (НЕ рядок
    "offline-{n}"); той самий номер іде в ID тега <MAC> офлайн-чека.
  - local_number офлайн-чеків = послідовний номер з початку зміни (як онлайн);
    резервний номер — лише в id_offline та ID тега <MAC>.
  - T=109 (перехід в офлайн) ОБОВ'ЯЗКОВО потрапляє в офлайн-чергу, щоб
    тимчасовий ланцюжок передавався до ФСКО після відновлення зв'язку.
"""

from __future__ import annotations

import logging
import re
from datetime import datetime
from typing import Optional

from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.xml_builder import (
    DEFAULT_RESERVE_SIZE,
    SERVICE_OFFLINE,
    SERVICE_ONLINE,
    SERVICE_RESERVE,
    compute_mac,
    cp1251_bytes,
    reconstruct_full_rq,
    signed_bytes_to_text,
)

logger = logging.getLogger(__name__)

# Ключі налаштувань (1:1 Rust models.rs)
KEY_PRRO_OFFLINE = "prro_offline"           # "1" — offline, "0"/None — online
KEY_PRRO_RESERVE_START = "prro_reserve_start"
KEY_PRRO_RESERVE_END = "prro_reserve_end"
KEY_PRRO_OFFLINE_NEXT = "prro_offline_next"

SERVICE_OFFLINE = SERVICE_OFFLINE  # Перехід в офлайн
SERVICE_ONLINE = SERVICE_ONLINE    # Перехід в онлайн
SERVICE_RESERVE = SERVICE_RESERVE  # Запит діапазону резервних номерів


def _fmt_ts(now: datetime | None = None) -> str:
    """yyyyMMddHHmmss (ЛОКАЛЬНИЙ час) — 1:1 Rust ts_now."""
    now = now or datetime.now()
    return now.strftime("%Y%m%d%H%M%S")


def parse_reserve_ids(data_sign: bytes) -> list[int]:
    """
    Парсить відповідь на T=112: перелік фіскальних номерів <ID>.

    Формат (262576.docx):
        <RS V="1"><C T="112"><ID>aaaaaaaaaa</ID><ID>bbbbbbbbb</ID>…</C></RS>

    Args:
        data_sign: байти data_sign з CheckResponse.

    Returns:
        list[int] — номери з діапазону у порядку отримання.
        Порожній список, якщо перелік відсутній або некоректний.
    """
    try:
        xml = data_sign.decode("utf-8", errors="replace")
    except Exception:
        return []
    ids = [int(x) for x in re.findall(r"<ID>\s*(\d+)\s*</ID>", xml)]
    return [x for x in ids if x >= 1]


def parse_reserve_range(data_sign: bytes) -> Optional[tuple[int, int]]:
    """
    Backward-сумісний аналог parse_reserve_ids: (start, end) або None.

    За spec F парсинг іде по <ID>-переліку (ПРРО), а не по модемному
    <CNF FR TO>.
    """
    ids = parse_reserve_ids(data_sign)
    if not ids:
        return None
    return min(ids), max(ids)


class OfflineStateMachine:
    """Державна машина офлайн-режиму ПРРО — безстатеві методи (1:1 Rust)."""

    @staticmethod
    async def is_offline(settings_repo: PrroSettingsRepository) -> bool:
        value = await settings_repo.get(KEY_PRRO_OFFLINE)
        return value is not None and str(value).strip() == "1"

    @staticmethod
    async def _next_reserve_number(
        settings_repo: PrroSettingsRepository,
    ) -> Optional[int]:
        """
        Повертає наступний резервний фіскальний номер (або None, якщо діапазон
        не отримано/вичерпано). Номер споживається (лічильник зсувається).
        """
        start_raw = await settings_repo.get(KEY_PRRO_RESERVE_START)
        end_raw = await settings_repo.get(KEY_PRRO_RESERVE_END)
        next_raw = await settings_repo.get(KEY_PRRO_OFFLINE_NEXT)
        if not start_raw or not end_raw:
            return None
        try:
            start = int(start_raw)
            end = int(end_raw)
            nxt = int(next_raw) if next_raw else start
        except (TypeError, ValueError):
            return None
        if nxt < start or nxt > end:
            return None
        await settings_repo.set(KEY_PRRO_OFFLINE_NEXT, str(nxt + 1))
        return nxt

    @staticmethod
    async def enter_offline(
        settings_repo: PrroSettingsRepository,
        grpc_client,
        xml_builder,
        crypto,
        *,
        offline_queue=None,
        prro_repo=None,
        shift_id=None,
        now: datetime | None = None,
    ) -> None:
        """
        ONLINE→OFFLINE: T=109.

        - Тіло 109 — `<C T="109"></C>`; ID тега <MAC> = резервний фіскальний
          номер (якщо діапазон уже отримано), значення = поточний ланцюг.
        - Документ НЕ втрачається: якщо доставка не вдалась — він кладеться
          в офлайн-чергу (pending) і буде переданий після відновлення зв'язку
          (спека: ланцюжок в офлайні обов'язково містить T=109).
        - Стан ПРРО перемикається в offline у будь-якому разі (помилка мережі
          не блокує перехід).
        """
        # Поточне значення ланцюга (shift.last_mac) → MAC документа 109
        doc_mac = ""
        if shift_id is not None and prro_repo is not None:
            shift = await prro_repo.get_shift(shift_id)
            doc_mac = (shift.last_mac or "") if shift is not None else ""

        # Резервний фіскальний номер для ID тега <MAC> (якщо діапазон є)
        offline_id = await OfflineStateMachine._next_reserve_number(settings_repo)
        mac_id = str(offline_id) if offline_id is not None else ""

        dat_xml = xml_builder.build_service_check_xml(
            service_type=SERVICE_OFFLINE, date_time=now
        )
        message = xml_builder.build_message(dat_xml, mac_value=doc_mac, mac_id=mac_id)
        signed = crypto.sign(cp1251_bytes(message))
        check = _make_service_check(xml_builder, signed, now)

        delivered = False
        try:
            response = await grpc_client.send_chk(check)
            delivered = bool(getattr(response, "status", None)) and int(response.status) == 1
        except Exception as exc:
            logger.warning("PRRO_OFFLINE | T=109 не доставлено: %s", exc)

        # 109 у черзі (для синку ланцюжка після відновлення зв'язку)
        if offline_queue is not None:
            item = await offline_queue.add_document(
                receipt_id=None,
                shift_id=shift_id,
                local_number=0,
                check_type="SERVICECHK",
                xml_body=dat_xml,
                mac=doc_mac,
                id_offline=mac_id or None,
                check_sign=signed_bytes_to_text(signed),
            )
            if delivered:
                await offline_queue.mark_sent(item.id)
            # Якщо не доставлено — документ лишається pending у черзі

        # Ланцюг: наступний документ (офлайн-чек) посилається на хеш 109
        if shift_id is not None and prro_repo is not None:
            await prro_repo.update_shift_last_mac(shift_id, compute_mac(message))

        await settings_repo.set(KEY_PRRO_OFFLINE, "1")

    @staticmethod
    async def reserve_numbers(
        settings_repo: PrroSettingsRepository,
        grpc_client,
        xml_builder,
        crypto,
        *,
        mac_value: str = "",
        reserve_size: int = DEFAULT_RESERVE_SIZE,
        now: datetime | None = None,
    ) -> tuple[int, int]:
        """
        T=112: запит резервного діапазону номерів для offline-чеків.

        Запит: `<C T="112"><H SIZE="{reserve_size}"></H></C>`.
        Відповідь: перелік <ID>…</ID> (parse_reserve_ids).

        Returns:
            (start, end) — отриманий діапазон.

        Raises:
            RuntimeError: якщо сервер не повернув жодного номера (без
                мовчазного фейкового дефолту — сервер його не реєстрував,
                і такі офлайн-чеки будуть відхилені з -16).
        """
        dat_xml = xml_builder.build_service_check_xml(
            service_type=SERVICE_RESERVE,
            date_time=now,
            reserve_size=reserve_size,
        )
        message = xml_builder.build_message(dat_xml, mac_value=mac_value)
        signed = crypto.sign(cp1251_bytes(message))
        check = _make_service_check(xml_builder, signed, now)
        response = await grpc_client.send_chk(check)
        data_sign = getattr(response, "data_sign", b"") or b""
        ids = parse_reserve_ids(data_sign)
        if not ids:
            raise RuntimeError(
                "PRRO_OFFLINE | T=112: сервер не повернув діапазон резервних "
                "номерів (<ID> відсутні у data_sign)"
            )
        start, end = min(ids), max(ids)
        await settings_repo.set(KEY_PRRO_RESERVE_START, str(start))
        await settings_repo.set(KEY_PRRO_RESERVE_END, str(end))
        await settings_repo.set(KEY_PRRO_OFFLINE_NEXT, str(start))
        logger.info("PRRO_OFFLINE | резервний діапазон: %d..%d", start, end)
        return start, end

    @staticmethod
    async def exit_offline(
        settings_repo: PrroSettingsRepository,
        grpc_client,
        xml_builder,
        crypto,
        sync_call,
        *,
        mac_value: str = "",
        now: datetime | None = None,
    ) -> dict:
        """OFFLINE→ONLINE: T=110 → стан online → sync офлайн-черги."""
        dat_xml = xml_builder.build_service_check_xml(
            service_type=SERVICE_ONLINE, date_time=now
        )
        message = xml_builder.build_message(dat_xml, mac_value=mac_value)
        signed = crypto.sign(cp1251_bytes(message))
        check = _make_service_check(xml_builder, signed, now)
        # T=110 обов'язковий: без нього сервер не прийме offline-ланцюжок.
        await grpc_client.send_chk(check)
        await settings_repo.set(KEY_PRRO_OFFLINE, "0")
        return await sync_call()

    @staticmethod
    async def next_offline_number(
        settings_repo: PrroSettingsRepository,
    ) -> int:
        """
        Наступний резервний фіскальний номер для офлайн-чека (id_offline).

        Returns:
            int — фіскальний номер з діапазону, отриманого через T=112.

        Raises:
            RuntimeError: якщо діапазон не отримано або вичерпано
                (треба повторити T=112) — без фейкових дефолтів.
        """
        number = await OfflineStateMachine._next_reserve_number(settings_repo)
        if number is None:
            raise RuntimeError(
                "PRRO_OFFLINE | немає резервних фіскальних номерів: спочатку "
                "отримайте діапазон через T=112 (reserve_numbers)"
            )
        return number


def _make_service_check(xml_builder, signed: bytes, now: datetime | None = None):
    """Формує службовий Check (T=108..112) — 1:1 Rust make_service_check."""
    from app.infrastructure.services.prro import prro_pb2

    if now is None:
        from app.infrastructure.services.prro.grpc_client import _check_date_time

        date_time = _check_date_time()
    else:
        date_time = int(now.strftime("%Y%m%d%H%M%S"))
    return prro_pb2.Check(
        rro_fn=xml_builder.rro_fn,
        date_time=date_time,
        check_sign=signed,
        local_number=0,
        check_type=prro_pb2.Check.SERVICECHK,
        id_offline="",
        id_cancel="",
    )


__all__ = [
    "KEY_PRRO_OFFLINE",
    "KEY_PRRO_OFFLINE_NEXT",
    "KEY_PRRO_RESERVE_END",
    "KEY_PRRO_RESERVE_START",
    "OfflineStateMachine",
    "parse_reserve_ids",
    "parse_reserve_range",
]
