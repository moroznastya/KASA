"""
Application Layer: PrroShiftUseCase — відкриття/закриття зміни ПРРО.

Відповідає за:
  - open_shift()         — службовий чек T=108, створення PrroShift (open);
  - close_shift()        — Z-звіт, закриття PrroShift (closed, zreport_number);
  - auto_reminder_check()— попередження, якщо зміна відкрита > 24 год;
  - list_shifts()        — журнал змін.
"""

from __future__ import annotations

import logging
from datetime import datetime
from decimal import Decimal

from sqlalchemy.ext.asyncio import AsyncSession

from app.application.dto.prro_dto import PrroShiftDTO
from app.application.use_cases.prro.context import (
    CHECK_TYPE_CHK,
    CHECK_TYPE_SERVICECHK,
    CHECK_TYPE_ZREPORT,
    PrroContextFactory,
)
from app.application.use_cases.prro.status_codes import status_error_text
from app.infrastructure.persistence.models.prro import (
    PrroQueueStatus,
    PrroShift,
    PrroShiftStatus,
)
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.offline_queue import PrroOfflineQueue
from app.infrastructure.services.prro.xml_builder import (
    SERVICE_OPEN_SHIFT,
    compute_mac,
    parse_receipt_xml_totals,
)

logger = logging.getLogger(__name__)


class PrroShiftError(Exception):
    """Помилка операції зі зміною ПРРО. __str__ = "[КОД] Точний текст"."""

    def __init__(self, message: str, code: str = "PRRO_SHIFT_ERROR"):
        super().__init__(message)
        self.message = message
        self.code = code

    def __str__(self) -> str:
        return f"[{self.code}] {self.message}"


class PrroShiftUseCase:
    """
    Use Case для змін ПРРО.

    Args:
        session: асинхронна сесія БД (для коміту транзакцій).
        prro_repo: репозиторій змін/черги ПРРО.
        settings_repo: репозиторій налаштувань ПРРО.
        context_factory: фабрика компонентів ПРРО.
        offline_queue: черга офлайн-документів ПРРО.
    """

    def __init__(
        self,
        session: AsyncSession,
        prro_repo: PrroRepository,
        settings_repo: PrroSettingsRepository,
        context_factory: PrroContextFactory,
        offline_queue: PrroOfflineQueue,
    ) -> None:
        self._session = session
        self._prro_repo = prro_repo
        self._settings_repo = settings_repo
        self._context = context_factory
        self._offline_queue = offline_queue

    # ─── Відкриття зміни ───────────────────────────────────────────────────

    async def open_shift(self, comment: str | None = None) -> PrroShiftDTO:
        """
        Відкриває зміну ПРРО (службовий чек T=108, local_number=0).

        Кроки:
          1. Перевірка, що зміна не відкрита;
          2. Формування службового чеку T=108 (xml_builder);
          3. Підписання XML (crypto.sign);
          4. Надсилання send_chk (check_type=SERVICECHK);
          5. При OK — створення PrroShift (status=open) + запис у чергу (sent).

        Args:
            comment: коментар (наприклад, ПІБ касира).

        Returns:
            PrroShiftDTO — створена зміна.

        Raises:
            PrroShiftError: якщо зміна вже відкрита або сервер відхилив.
        """
        open_shift = await self._prro_repo.get_open_shift()
        if open_shift is not None:
            raise PrroShiftError(
                f"Зміна #{open_shift.shift_number} вже відкрита "
                f"({open_shift.opened_at:%Y-%m-%d %H:%M})",
                code="SHIFT_ALREADY_OPEN",
            )

        # 2. Службовий чек T=108
        xml_builder = await self._context.build_xml_builder()
        crypto = await self._context.build_crypto_signer()

        dat_xml = xml_builder.build_service_check_xml(
            service_type=SERVICE_OPEN_SHIFT
        )
        message = xml_builder.build_message(dat_xml)
        signed = crypto.sign(message.encode("utf-8"))
        mac = compute_mac(dat_xml)

        # 3. Надсилаємо (check_type=SERVICECHK, local_number=0)
        check = await self._context.build_check(
            check_sign=signed,
            local_number=0,
            check_type=CHECK_TYPE_SERVICECHK,
        )
        grpc_client = await self._context.grpc_client()
        response = await grpc_client.send_chk(check)

        if int(response.status) != 1:
            # Код + ім'я + людський опис ЗАВЖДИ; текст сервера — повністю
            # (1:1 Rust shift.rs; джерело мапи: status_codes).
            status_text = status_error_text(int(response.status))
            error_msg = (
                f"{response.error_message} | {status_text}"
                if response.error_message
                else status_text
            )
            raise PrroShiftError(
                f"Не вдалося відкрити зміну: {error_msg}",
                code="OPEN_SHIFT_FAILED",
            )

        # 4. Створюємо зміну
        shift_number = await self._context.next_shift_number()
        now = datetime.utcnow()
        shift = PrroShift(
            shift_number=shift_number,
            opened_at=now,
            signer_serial=crypto.get_serial_number(),
            signer_name=crypto.get_signer_name(),
            status=PrroShiftStatus.OPEN,
            receipt_count=0,
            total_amount=0,
            last_local_number=0,
            last_mac=mac,
        )
        await self._prro_repo.create_shift(shift)
        await self._context.save_last_shift_number(shift_number)

        # 5. Запис у чергу (успішно передано)
        queue_item = await self._offline_queue.add_document(
            receipt_id=None,
            shift_id=shift.id,
            local_number=0,
            check_type=CHECK_TYPE_SERVICECHK,
            xml_body=dat_xml,
            mac=mac,
        )
        await self._offline_queue.mark_sent(queue_item.id)

        await self._context.persist_builder_counters(xml_builder)
        await self._session.commit()

        logger.info(
            "PRRO_SHIFT | зміну #%d відкрито (signer=%s)",
            shift_number, shift.signer_name,
        )
        return self._to_dto(shift)

    # ─── Закриття зміни ────────────────────────────────────────────────────

    async def close_shift(self, comment: str | None = None) -> PrroShiftDTO:
        """
        Закриває зміну ПРРО (Z-звіт).

        Кроки:
          1. Пошук відкритої зміни;
          2. Формування Z-звіту (підсумки з чеків зміни);
          3. Підписання та надсилання send_chk (check_type=ZREPORT);
          4. При OK — закриття PrroShift (closed_at, zreport_number).

        Args:
            comment: коментар (хто закриває зміну).

        Returns:
            PrroShiftDTO — закрита зміна.

        Raises:
            PrroShiftError: якщо немає відкритої зміни або сервер відхилив.
        """
        open_shift = await self._prro_repo.get_open_shift()
        if open_shift is None:
            raise PrroShiftError(
                "Немає відкритої зміни ПРРО",
                code="NO_OPEN_SHIFT",
            )

        xml_builder = await self._context.build_xml_builder()
        crypto = await self._context.build_crypto_signer()

        # 2. Z-звіт з підсумками зміни (з фактично переданих чеків)
        z_data = await self._build_zreport_data(open_shift)
        dat_xml = xml_builder.build_zreport_xml(shift_data=z_data)
        message = xml_builder.build_message(dat_xml)
        signed = crypto.sign(message.encode("utf-8"))
        mac = compute_mac(dat_xml)

        # 3. Надсилаємо Z-звіт (check_type=ZREPORT, local_number=0)
        check = await self._context.build_check(
            check_sign=signed,
            local_number=0,
            check_type=CHECK_TYPE_ZREPORT,
        )
        grpc_client = await self._context.grpc_client()
        response = await grpc_client.send_chk(check)

        if int(response.status) != 1:
            # Код + ім'я + людський опис ЗАВЖДИ; текст сервера — повністю
            # (1:1 Rust shift.rs; джерело мапи: status_codes).
            status_text = status_error_text(int(response.status))
            error_msg = (
                f"{response.error_message} | {status_text}"
                if response.error_message
                else status_text
            )
            raise PrroShiftError(
                f"Не вдалося закрити зміну: {error_msg}",
                code="CLOSE_SHIFT_FAILED",
            )

        # 4. Закриваємо зміну
        closed = await self._prro_repo.close_shift(
            shift_id=open_shift.id,
            closed_at=datetime.utcnow(),
            closed_by=comment or "system",
            zreport_number=response.id,
            signer_serial=crypto.get_serial_number(),
            signer_name=crypto.get_signer_name(),
        )

        # Запис у чергу (Z-звіт успішно передано)
        queue_item = await self._offline_queue.add_document(
            receipt_id=None,
            shift_id=open_shift.id,
            local_number=0,
            check_type=CHECK_TYPE_ZREPORT,
            xml_body=dat_xml,
            mac=mac,
        )
        await self._offline_queue.mark_sent(queue_item.id)

        # B1: last_mac = MAC(Z) — останній успішно відправлений документ зміни.
        await self._prro_repo.update_shift_last_mac(open_shift.id, mac)

        await self._context.persist_builder_counters(xml_builder)
        await self._session.commit()

        logger.info(
            "PRRO_SHIFT | зміну #%d закрито, Z-звіт %s",
            open_shift.shift_number, response.id,
        )
        return self._to_dto(closed)

    # ─── Нагадування про відкриту зміну ────────────────────────────────────

    async def auto_reminder_check(self) -> dict | None:
        """
        Перевіряє, чи зміна відкрита більше 24 годин.

        Returns:
            dict | None — {"warning", "shift_open", "hours_open"} або None.
        """
        open_shift = await self._prro_repo.get_open_shift()
        if open_shift is None:
            return None
        hours = (datetime.utcnow() - open_shift.opened_at).total_seconds() / 3600
        if hours > 24:
            return {
                "warning": (
                    f"Зміна #{open_shift.shift_number} відкрита більше 24 годин "
                    f"({hours:.1f} год). Рекомендується закрити зміну (Z-звіт)."
                ),
                "shift_open": True,
                "hours_open": round(hours, 1),
            }
        return None

    # ─── Журнал змін ───────────────────────────────────────────────────────

    async def list_shifts(
        self, page: int = 1, size: int = 20
    ) -> tuple[list[PrroShiftDTO], int]:
        """
        Повертає список змін з пагінацією.

        Args:
            page: номер сторінки (з 1).
            size: кількість на сторінці.

        Returns:
            (список PrroShiftDTO, загальна кількість).
        """
        shifts, total = await self._prro_repo.list_shifts(page=page, size=size)
        return [self._to_dto(s) for s in shifts], total

    # ─── Підсумки зміни для Z-звіту ───────────────────────────────────────

    async def _build_zreport_data(self, shift: PrroShift) -> dict:
        """
        Розраховує підсумки зміни для Z-звіту на основі чеків, що були
        фактично передані на фіскальний сервер (PrroQueueItem status=sent,
        check_type=CHK, прив'язані до зміни).

        Для кожного чеку розбирається збережений XML (xml_body) — це
        гарантує, що Z-звіт відповідає реально переданим даним (T=0/1,
        суми, податкові групи, форми оплати).

        Returns:
            dict — shift_data для build_zreport_xml:
            {
                "shift_number": int,
                "sales_count": int,
                "returns_count": int,
                "taxes": [{tax, ts, tax_percent, tax_in, tax_out,
                           tax_type, tax_algorithm, smi, smo}, ...],
                "payments": [{code, name, smi, smo}, ...],
            }
        """
        queue_items = await self._prro_repo.list_by_shift(shift.id)
        sent_checks = [
            item for item in queue_items
            if item.check_type == CHECK_TYPE_CHK
            and item.status == PrroQueueStatus.SENT
        ]

        sales_count = 0
        returns_count = 0
        payments: dict[str, dict] = {}      # code -> {"name", "smi", "smo"}
        taxes: dict[str, dict] = {}         # tx_code -> {"percent", "in", "out"}

        for item in sent_checks:
            try:
                parsed = parse_receipt_xml_totals(item.xml_body or "")
            except ValueError:
                logger.warning(
                    "PRRO_SHIFT | Z-звіт: не вдалося розібрати XML чеку %s",
                    item.receipt_id,
                )
                continue

            is_return = parsed["check_type"] == "1"
            if is_return:
                returns_count += 1
            else:
                sales_count += 1

            # Обороти за формами оплати (SMI — отримано, SMO — видано)
            for code, amount in parsed["payments"].items():
                pay = payments.setdefault(code, {"name": "", "smi": Decimal("0"), "smo": Decimal("0")})
                if is_return:
                    pay["smo"] += amount
                else:
                    pay["smi"] += amount

            # ПДВ та обіг за податковими групами (TXI/SMI — отримано, TXO — видано)
            for code, tax in parsed["taxes"].items():
                group = taxes.setdefault(
                    code,
                    {
                        "percent": tax["percent"],
                        "in": Decimal("0"),
                        "out": Decimal("0"),
                        "smi": Decimal("0"),
                    },
                )
                group["smi"] += tax.get("smi", Decimal("0"))
                if is_return:
                    group["out"] += tax["tax_total"]
                else:
                    group["in"] += tax["tax_total"]

        today = datetime.utcnow().strftime("%Y%m%d")
        tax_rows = []
        for code, group in sorted(taxes.items()):
            tax_rows.append({
                "tax": code,
                "ts": today,
                "tax_percent": group["percent"],
                "tax_in": group["in"],
                "tax_out": group["out"],
                "tax_type": "0",
                "tax_algorithm": "0",
                "smi": group["smi"],
                "smo": Decimal("0"),
            })

        payment_rows = []
        for code, pay in sorted(payments.items()):
            payment_rows.append({
                "code": code,
                "name": pay["name"] or ("ГОТІВКА" if code == "0" else "КАРТКА"),
                "smi": pay["smi"],
                "smo": pay["smo"],
            })

        # Якщо чеків не знайдено — використовуємо лічильники зміни як fallback
        if not sent_checks:
            sales_count = int(shift.receipt_count)

        return {
            "shift_number": shift.shift_number,
            "sales_count": sales_count,
            "returns_count": returns_count,
            "taxes": tax_rows,
            "payments": payment_rows,
        }

    # ─── Допоміжне ─────────────────────────────────────────────────────────

    @staticmethod
    def _to_dto(shift: PrroShift) -> PrroShiftDTO:
        """Конвертує PrroShift у PrroShiftDTO."""
        return PrroShiftDTO(
            id=shift.id,
            shift_number=shift.shift_number,
            opened_at=shift.opened_at,
            closed_at=shift.closed_at,
            signer_name=shift.signer_name,
            status=shift.status.value if hasattr(shift.status, "value") else str(shift.status),
            receipt_count=shift.receipt_count,
            total_amount=shift.total_amount,
            zreport_number=shift.zreport_number,
        )


__all__ = ["PrroShiftError", "PrroShiftUseCase"]
