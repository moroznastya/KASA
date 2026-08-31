"""
Офлайн-режим ПРРО: переходи 109/110, резервні номери 112, id_offline.

1:1 Rust `frontend/src-tauri/crates/torgashka-prro/src/prro/offline.rs`.

Протокол [ДПС]: службовий чек T=109 — перехід в офлайн, T=110 — в онлайн,
T=112 — запит діапазону резервних номерів (відповідь: `<CNF TY="C" FR=".."
TO=".."/>` у `data_sign` — СЗЗД 2.1.7, формат повідомлення від серверу).
Offline-чеки використовують local_number з резервного діапазону та
id_offline (не порожній).
"""

from __future__ import annotations

import logging
import re
from datetime import datetime
from typing import Optional

from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)

logger = logging.getLogger(__name__)

# Дефолтний резервний діапазон, якщо сервер не відповів на T=112.
DEFAULT_RESERVE_START = 1_000_000
DEFAULT_RESERVE_END = 1_000_999

# Ключі налаштувань (1:1 Rust models.rs)
KEY_PRRO_OFFLINE = "prro_offline"           # "1" — offline, "0"/None — online
KEY_PRRO_RESERVE_START = "prro_reserve_start"
KEY_PRRO_RESERVE_END = "prro_reserve_end"
KEY_PRRO_OFFLINE_NEXT = "prro_offline_next"

SERVICE_OFFLINE = "109"  # Перехід в офлайн
SERVICE_ONLINE = "110"   # Перехід в онлайн
SERVICE_RESERVE = "112"  # Запит діапазону резервних номерів


def _fmt_ts(now: datetime | None = None) -> str:
    """yyyyMMddHHmmss (локальний час) — 1:1 Rust ts_now."""
    now = now or datetime.utcnow()
    return now.strftime("%Y%m%d%H%M%S")


def parse_reserve_range(data_sign: bytes) -> Optional[tuple[int, int]]:
    """Парсить `<CNF TY="C" FR="1001" TO="1100"/>` з data_sign; None → дефолт."""
    try:
        xml = data_sign.decode("utf-8", errors="replace")
    except Exception:
        return None
    m = re.search(r'<CNF[^>]*\bFR="(\d+)"[^>]*\bTO="(\d+)"', xml)
    if not m:
        return None
    start, end = int(m.group(1)), int(m.group(2))
    if start < 1 or end < start:
        return None
    return start, end


class OfflineStateMachine:
    """Державна машина офлайн-режиму ПРРО — безстатеві методи (1:1 Rust)."""

    @staticmethod
    async def is_offline(settings_repo: PrroSettingsRepository) -> bool:
        value = await settings_repo.get(KEY_PRRO_OFFLINE)
        return value is not None and str(value).strip() == "1"

    @staticmethod
    async def enter_offline(
        settings_repo: PrroSettingsRepository,
        grpc_client,
        xml_builder,
        crypto,
        now: datetime | None = None,
    ) -> None:
        """ONLINE→OFFLINE: T=109 (best-effort; помилка мережі не блокує стан)."""
        dat_xml = xml_builder.build_service_check_xml(
            service_type=SERVICE_OFFLINE, date_time=now
        )
        message = xml_builder.build_message(dat_xml)
        signed = crypto.sign(message.encode("utf-8"))
        check = _make_service_check(xml_builder, signed, now)
        try:
            await grpc_client.send_chk(check)
        except Exception as exc:
            logger.warning("PRRO_OFFLINE | T=109 не доставлено: %s", exc)
        await settings_repo.set(KEY_PRRO_OFFLINE, "1")

    @staticmethod
    async def reserve_numbers(
        settings_repo: PrroSettingsRepository,
        grpc_client,
        xml_builder,
        crypto,
        now: datetime | None = None,
    ) -> tuple[int, int]:
        """T=112: запит резервного діапазону номерів для offline-чеків."""
        dat_xml = xml_builder.build_service_check_xml(
            service_type=SERVICE_RESERVE, date_time=now
        )
        message = xml_builder.build_message(dat_xml)
        signed = crypto.sign(message.encode("utf-8"))
        check = _make_service_check(xml_builder, signed, now)
        response = await grpc_client.send_chk(check)
        data_sign = getattr(response, "data_sign", b"")
        start, end = parse_reserve_range(data_sign) or (
            DEFAULT_RESERVE_START,
            DEFAULT_RESERVE_END,
        )
        await settings_repo.set(KEY_PRRO_RESERVE_START, str(start))
        await settings_repo.set(KEY_PRRO_RESERVE_END, str(end))
        await settings_repo.set(KEY_PRRO_OFFLINE_NEXT, str(start))
        return start, end

    @staticmethod
    async def exit_offline(
        settings_repo: PrroSettingsRepository,
        grpc_client,
        xml_builder,
        crypto,
        sync_call,
        now: datetime | None = None,
    ) -> dict:
        """OFFLINE→ONLINE: T=110 → стан online → sync офлайн-черги."""
        dat_xml = xml_builder.build_service_check_xml(
            service_type=SERVICE_ONLINE, date_time=now
        )
        message = xml_builder.build_message(dat_xml)
        signed = crypto.sign(message.encode("utf-8"))
        check = _make_service_check(xml_builder, signed, now)
        # T=110 обов'язковий: без нього сервер не прийме offline-ланцюжок.
        await grpc_client.send_chk(check)
        await settings_repo.set(KEY_PRRO_OFFLINE, "0")
        return await sync_call()

    @staticmethod
    async def next_offline_local(settings_repo: PrroSettingsRepository) -> tuple[int, str]:
        """Наступний (local_number, id_offline) для offline-чека з резервного
        діапазону. id_offline — НЕ порожній: "offline-{local_number}"."""
        start_raw = await settings_repo.get(KEY_PRRO_RESERVE_START)
        end_raw = await settings_repo.get(KEY_PRRO_RESERVE_END)
        next_raw = await settings_repo.get(KEY_PRRO_OFFLINE_NEXT)
        start = int(start_raw) if start_raw else DEFAULT_RESERVE_START
        end = int(end_raw) if end_raw else DEFAULT_RESERVE_END
        nxt = int(next_raw) if next_raw else start
        n = min(nxt, end)
        await settings_repo.set(KEY_PRRO_OFFLINE_NEXT, str(n + 1))
        return n, f"offline-{n}"


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
    "DEFAULT_RESERVE_END",
    "DEFAULT_RESERVE_START",
    "KEY_PRRO_OFFLINE",
    "KEY_PRRO_OFFLINE_NEXT",
    "KEY_PRRO_RESERVE_END",
    "KEY_PRRO_RESERVE_START",
    "OfflineStateMachine",
    "parse_reserve_range",
]
