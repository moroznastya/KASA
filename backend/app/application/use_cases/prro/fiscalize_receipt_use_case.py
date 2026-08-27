"""
Application Layer: FiscalizeReceiptUseCase — фіскалізація чеку через ПРРО.

Головний use case фіскалізації (Фаза 2):

  1. Завантаження Receipt + ReceiptItems (фіскальні поля);
  2. Якщо немає фіскальних позицій (fiscal_quantity <= 0) — без дій;
  3. Для повернення (T=1): перевірка посилання на оригінальний фіскальний
     чек (original_receipt_id) — якщо оригінал не фіскалізований,
     повернення не фіскалізується;
  4. Валідація (prro_domain_service): ПРРО налаштований, чек ще не
     фіскалізований, сума чека > 0, зміна відкрита;
  5. ЧАСТКОВА фіскалізація (split): якщо фіскальних позицій менше ніж
     у чеку або fiscal_stock не покриває всю кількість — чек розділяється:
       - оригінальний чек стає ФІСКАЛЬНИМ (fiscal_quantity = min(...));
       - створюється НЕФІСКАЛЬНИЙ дублікат (split_group_id = id фіскального
         чека) з позиціями, що не увійшли у фіскальний чек;
  6. Формування XML чеку T=0 (sale) / T=1 (return, RT="0") — лише фіскальні
     позиції, суми перераховуються пропорційно fiscal_quantity;
  7. Підписання XML (crypto.sign);
  8. Надсилання send_chk (CHK) через gRPC;
  9. При OK — оновлення Receipt (SENT, fiscal_number, fiscal_sent_at),
     зменшення Product.fiscal_stock (для повернення — збільшення),
     запис у prro_queue (sent), формування fiscal_check_url (QR);
 10. При помилці — Receipt (FAILED, fiscal_error), prro_queue (failed);
     при ERROR_SAVE/-12 — спроба lastChk/дедуплікації.

Статуси: pending → (send_chk) → sent | failed.

Де робиться split: У ФАЗІ ФІСКАЛІЗАЦІЇ (FiscalizeReceiptUseCase), а не при
створенні чека. Це дозволяє звичайному продажу не ускладнюватися, якщо ПРРО
не налаштовано, і виконувати розділення лише за потреби.
"""

from __future__ import annotations

import logging
import os
import time
from datetime import datetime
from decimal import Decimal, ROUND_HALF_UP
from typing import Optional
from app.application.use_cases.prro.status_codes import status_name
from uuid import UUID, uuid4

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.application.dto.prro_dto import FiscalizeResponseDTO
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.receipt import (
    FiscalStatus,
    Receipt,
    ReceiptItem,
)
from app.infrastructure.persistence.repositories.prro_repository import PrroRepository
from app.infrastructure.persistence.repositories.prro_settings_repository import (
    PrroSettingsRepository,
)
from app.infrastructure.services.prro.xml_builder import extract_check_no
from app.infrastructure.services.prro.offline_queue import (
    PrroOfflineQueue,
    CHECK_TYPE_CHK,
)
from app.infrastructure.services.prro.offline_state import OfflineStateMachine
from app.infrastructure.services.prro.qr_url import build_fiscal_check_url
from app.infrastructure.services.prro.xml_builder import (
    CHK_TYPE_RETURN,
    CHK_TYPE_SALE,
    compute_mac,
)
from app.application.use_cases.prro.context import (
    PrroContextFactory,
    CHECK_TYPE_CHK as _CHK,
    KEY_AUTO_FISCALIZE,
    KEY_PRRO_FN,
    KEY_PRRO_STUB_MODE,
)

logger = logging.getLogger(__name__)

# Коди помилок фіскального сервера (CheckResponse.Status у prro.proto)
ERROR_SAVE = -3          # Помилка запису (можливий дубль)
ERROR_BAD_HASH_PREV = -12  # Невірний хеш попереднього чеку


class PrroFiscalizeError(Exception):
    """Помилка фіскалізації чеку ПРРО. __str__ = "[КОД] Точний текст" — код
    помилки (GRPC_ERROR/ERROR_SAVE/-12/...) завжди присутній у фінальному
    повідомленні користувачу."""

    def __init__(self, message: str, code: str = "PRRO_FISCALIZE_ERROR"):
        super().__init__(message)
        self.message = message
        self.code = code

    def __str__(self) -> str:
        return f"[{self.code}] {self.message}"


def server_error_text(status: int, error_message: str) -> str:
    """Фінальний текст помилки: `[КОД] Точний текст` — 1:1 Rust
    `server_error_text`. Код ДПС завжди присутній; текст — повідомлення
    сервера, а якщо його немає — людське пояснення статусу."""
    from app.application.use_cases.prro.prro_settings_use_case import (
        PrroSettingsUseCase,
    )
    text = error_message.strip() or PrroSettingsUseCase._STATUS_MESSAGES.get(
        status, "Невідомий статус фіскального сервера."
    )
    return f"[{status_name(status)}] {text}"

class FiscalizeReceiptUseCase:
    """
    Use Case для фіскалізації чеку.

    Args:
        session: асинхронна сесія БД (завантаження/оновлення чеків, товарів).
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

    @property
    def session(self) -> AsyncSession:
        """Сесія БД (для закриття після авто-фіскалізації)."""
        return self._session

    # ─── Головний метод ────────────────────────────────────────────────────

    async def fiscalize_receipt(
        self,
        receipt_id: UUID,
        manual: bool = False,
    ) -> FiscalizeResponseDTO:
        """
        Фіскалізує чек продажу/повернення через ПРРО (з можливим split).

        Args:
            receipt_id: ID чеку.
            manual: ручна фіскалізація (true) / автоматична (false).

        Returns:
            FiscalizeResponseDTO — результат операції.

        Raises:
            PrroFiscalizeError: чек не знайдено, зміна не відкрита,
                ПРРО не налаштований, чек вже фіскалізований або сума ≤ 0.
        """
        # 0. Режим заглушки (stub): реальний ПРРО не підключений.
        #    Для РУЧНОГО виклику фіскалізуємо ЗАВЖДИ; для АВТО — лише якщо
        #    увімкнено auto_fiscalize (щоб не фіскалізувати чеки проти
        #    налаштувань). Успішна фіскалізація без реальних викликів ПРРО.
        auto_enabled = await self._auto_fiscalize_enabled()
        if await self._stub_mode_enabled() and (manual or auto_enabled):
            receipt = await self._load_receipt(receipt_id)
            if receipt is None:
                raise PrroFiscalizeError(
                    f"Чек з ID '{receipt_id}' не знайдено", code="RECEIPT_NOT_FOUND"
                )
            return await self._fiscalize_stub(receipt)

        # 1. Авто-фіскалізація: якщо вимкнена і виклик не ручний — без дій
        if not manual and not auto_enabled:
            return FiscalizeResponseDTO(
                receipt_id=receipt_id,
                fiscal_status="none",
                error="Авто-фіскалізація вимкнена (auto_fiscalize=false)",
            )

        # 2. Завантажуємо чек з позиціями
        receipt = await self._load_receipt(receipt_id)
        if receipt is None:
            raise PrroFiscalizeError(
                f"Чек з ID '{receipt_id}' не знайдено", code="RECEIPT_NOT_FOUND"
            )

        # 3. Фіскальні позиції (fiscal_quantity > 0)
        fiscal_items = [
            item for item in receipt.items
            if getattr(item, "fiscal_quantity", 0) and item.fiscal_quantity > 0
        ]
        if not fiscal_items:
            return FiscalizeResponseDTO(
                receipt_id=receipt_id,
                fiscal_status="none",
                error="Немає фіскальних позицій (fiscal_quantity <= 0)",
            )

        is_return = bool(getattr(receipt, "is_return", False))

        # 3. Повернення (T=1): має посилатися на оригінальний фіскальний чек
        if is_return:
            original = None
            if getattr(receipt, "original_receipt_id", None):
                original = await self._load_receipt(receipt.original_receipt_id)
            if original is None:
                return FiscalizeResponseDTO(
                    receipt_id=receipt_id,
                    fiscal_status="none",
                    error=(
                        "Повернення не фіскалізується: не вказано "
                        "оригінальний чек (original_receipt_id)"
                    ),
                )
            orig_status = self._status_str(original.fiscal_status)
            if orig_status != "sent":
                return FiscalizeResponseDTO(
                    receipt_id=receipt_id,
                    fiscal_status="none",
                    error=(
                        "Повернення не фіскалізується: оригінальний чек "
                        f"не фіскалізований (статус '{orig_status}')"
                    ),
                )

        # 4. Валідація перед фіскалізацією
        await self._validate(receipt)

        # 5. Відкрита зміна
        open_shift = await self._prro_repo.get_open_shift()
        if open_shift is None:
            raise PrroFiscalizeError(
                "Зміна ПРРО не відкрита. Відкрийте зміну перед фіскалізацією",
                code="ERROR_NOT_OPEN_SHIFT",
            )

        warnings: list[str] = []

        # 6. Split: ефективні фіскальні кількості + нефіскальний дублікат
        planned, split_receipt_id = await self._prepare_split(
            receipt,
            is_return=is_return,
            warnings=warnings,
        )
        if not planned:
            # Повністю нефіскальний чек — позначаємо та завершуємо без дій
            await self._mark_fully_non_fiscal(receipt)
            return FiscalizeResponseDTO(
                receipt_id=receipt_id,
                fiscal_status="none",
                error="Немає фіскальних позицій після перевірки залишків",
                warning="; ".join(warnings) or None,
            )

        xml_builder = await self._context.build_xml_builder()
        crypto = await self._context.build_crypto_signer()

        items_xml, total, tax_groups = self._build_receipt_payload(planned)
        # B4: offline-режим — local_number з резервного діапазону (T=112) +
        # id_offline (не порожній); online — звичайна нумерація зміни.
        if await OfflineStateMachine.is_offline(self._settings_repo):
            local_number, id_offline = await OfflineStateMachine.next_offline_local(
                self._settings_repo
            )
        else:
            # M1: атомарний інкремент+збереження (SQL UPDATE ... RETURNING)
            local_number = await self._prro_repo.next_local_number(open_shift.id)
            id_offline = "" 

        totals = {
            "total": total,
            "fiscal_number": local_number,
            "tax_groups": list(tax_groups.values()),
            "se": total - sum(
                tg["tax_total"] for tg in tax_groups.values()
            ),
            "cashier": 1,
        }
        payments = self._build_payments(receipt, total)

        dat_xml = xml_builder.build_receipt_xml(
            check_type=CHK_TYPE_RETURN if is_return else CHK_TYPE_SALE,
            items=items_xml,
            payments=payments,
            totals=totals,
            return_type="0",  # RT: 0 — повернення товару (для T=1)
            prev_hash=open_shift.last_mac,  # B1: хеш попереднього Check
        )
        message = xml_builder.build_message(dat_xml)
        signed = crypto.sign(message.encode("utf-8"))
        mac = compute_mac(dat_xml)

        # 7. Надсилаємо чек (CHK)
        check = await self._context.build_check(
            check_sign=signed,
            local_number=local_number,
            check_type=_CHK,
            id_offline=id_offline,  # B4: offline-чек — id_offline не порожній
        )
        grpc_client = await self._context.grpc_client()
        try:
            response = await grpc_client.send_chk(check)
        except Exception as exc:  # noqa: BLE001 — H1/B4: транспортний таймаут
            # H1: НЕ сліпий retry — спочатку lastChk: сервер міг зберегти чек,
            # а відповідь загубилась. Збіг NO (local_number) у XML останнього
            # чека → чек уже там → SENT (без дубліката).
            error_message = f"[GRPC_ERROR] gRPC send_chk не вдався: {exc}"
            try:
                last = await grpc_client.last_chk()
                last_xml = (getattr(last, "data_sign", b"") or b"").decode(
                    "utf-8", errors="replace"
                )
                if int(last.status) == 1 and extract_check_no(last_xml) == local_number:
                    logger.info(
                        "PRRO_FISCALIZE | H1: чек %s вже на сервері (lastChk NO=%d) → SENT",
                        receipt.id, local_number,
                    )
                    return await self._on_success(
                        receipt=receipt,
                        planned=planned,
                        total=total,
                        local_number=local_number,
                        dat_xml=dat_xml,
                        mac=mac,
                        check_sign=signed.decode("utf-8"),
                        id_offline=id_offline,
                        response_id=last.id,
                        id_sign=getattr(last, "id_sign", b""),
                        open_shift_id=open_shift.id,
                        xml_builder=xml_builder,
                        is_return=is_return,
                        split_receipt_id=split_receipt_id,
                        warnings=warnings,
                    )
            except Exception as last_exc:  # noqa: BLE001 — lastChk не вдався
                logger.warning("PRRO_FISCALIZE | H1: lastChk не вдався: %s", last_exc)

            # H1: чека немає → один контрольований повторний send
            try:
                response = await grpc_client.send_chk(check)
            except Exception as exc2:  # noqa: BLE001 — B4: мережа впала (повторно)
                # Документ у offline-чергу (failed), ПРРО → офлайн (T=109) +
                # резервний діапазон (T=112). Документ НЕ втрачається.
                error_message2 = f"[GRPC_ERROR] gRPC send_chk повторно не вдався: {exc2}"
                result = await self._on_error(
                    receipt=receipt,
                    local_number=local_number,
                    dat_xml=dat_xml,
                    mac=mac,
                    check_sign=signed.decode("utf-8"),
                    id_offline=id_offline,
                    open_shift_id=open_shift.id,
                    response_status=-1,
                    error_message=error_message2,
                    split_receipt_id=split_receipt_id,
                    warnings=warnings,
                )
                if not await OfflineStateMachine.is_offline(self._settings_repo):
                    try:
                        await OfflineStateMachine.enter_offline(
                            self._settings_repo, grpc_client, xml_builder, crypto
                        )
                        await OfflineStateMachine.reserve_numbers(
                            self._settings_repo, grpc_client, xml_builder, crypto
                        )
                    except Exception:  # noqa: BLE001 — стан уже offline, не фатально
                        logger.warning(
                            "PRRO_OFFLINE | перехід в офлайн: не вдалося", exc_info=True
                        )
                return result
            if int(response.status) != 1:
                error_msg = server_error_text(
                    int(response.status), response.error_message
                )
                return await self._on_error(
                    receipt=receipt,
                    local_number=local_number,
                    dat_xml=dat_xml,
                    mac=mac,
                    check_sign=signed.decode("utf-8"),
                    id_offline=id_offline,
                    open_shift_id=open_shift.id,
                    response_status=int(response.status),
                    error_message=error_msg,
                    split_receipt_id=split_receipt_id,
                    warnings=warnings,
                )
            return await self._on_success(
                receipt=receipt,
                planned=planned,
                total=total,
                local_number=local_number,
                dat_xml=dat_xml,
                mac=mac,
                check_sign=signed.decode("utf-8"),
                id_offline=id_offline,
                response_id=response.id,
                id_sign=getattr(response, "id_sign", b""),
                open_shift_id=open_shift.id,
                xml_builder=xml_builder,
                is_return=is_return,
                split_receipt_id=split_receipt_id,
                warnings=warnings,
            )

        # 8. Обробка відповіді
        if int(response.status) == 1:
            return await self._on_success(
                receipt=receipt,
                planned=planned,
                total=total,
                local_number=local_number,
                dat_xml=dat_xml,
                mac=mac,
                check_sign=signed.decode("utf-8"),
                id_offline=id_offline,
                response_id=response.id,
                id_sign=getattr(response, "id_sign", b""),
                open_shift_id=open_shift.id,
                xml_builder=xml_builder,
                is_return=is_return,
                split_receipt_id=split_receipt_id,
                warnings=warnings,
            )

        return await self._on_error(
            receipt=receipt,
            local_number=local_number,
            dat_xml=dat_xml,
            mac=mac,
            check_sign=signed.decode("utf-8"),
            id_offline=id_offline,
            open_shift_id=open_shift.id,
            response_status=int(response.status),
            error_message=server_error_text(
                int(response.status), response.error_message
            ),
            split_receipt_id=split_receipt_id,
            warnings=warnings,
        )

    # ─── Режим заглушки (тимчасово, ПРРО не підключений) ──────────────────

    async def _stub_mode_enabled(self) -> bool:
        """
        Режим заглушки фіскалізації (stub).

        Активний, якщо увімкнено ХОЧ ОДИН з джерел:
          - налаштування системи `prro_stub_mode` = 'true' або '1';
          - env-змінна PRRO_STUB = true.

        Returns:
            True — stub активний (реальні виклики ПРРО не виконуються).
        """
        try:
            value = await self._settings_repo.get(KEY_PRRO_STUB_MODE)
            if value and str(value).strip().lower() in ("true", "1"):
                return True
        except Exception:  # noqa: BLE001
            logger.debug("Не вдалося прочитати prro_stub_mode", exc_info=True)
        return os.getenv("PRRO_STUB", "").strip().lower() in ("true", "1")

    async def _fiscalize_stub(self, receipt: Receipt) -> FiscalizeResponseDTO:
        """
        Тимчасова заглушка фіскалізації: без реальних викликів ПРРО.

        - Генерує фіскальний номер STUB-<номер чека>-<timestamp>;
        - Позначає чек фіскалізованим (fiscalized, is_fiscal=True);
        - Зберігає зміни у БД (commit);
        - Повертає успішний FiscalizeResponseDTO.

        Args:
            receipt: чек продажу/повернення.

        Returns:
            FiscalizeResponseDTO — status='success'.
        """
        now = datetime.utcnow()
        stub_number = f"STUB-{receipt.receipt_number}-{int(time.time())}"
        receipt.fiscal_status = FiscalStatus.FISCALIZED
        receipt.fiscal_number = stub_number
        receipt.fiscal_serial = "STUB"
        receipt.fiscal_sent_at = now
        receipt.is_fiscal = True
        receipt.fiscal_error = None
        await self._session.commit()
        logger.info(
            "PRRO_STUB | чек %s позначено фіскалізованим: %s",
            receipt.id, stub_number,
        )
        return FiscalizeResponseDTO(
            receipt_id=receipt.id,
            status="success",
            fiscal_status=FiscalStatus.FISCALIZED.value,
            fiscal_number=stub_number,
            fiscal_serial="STUB",
            fiscal_sent_at=now,
            fiscal_date=now,
            message="Фіскалізацію виконано (заглушка)",
        )

    # ─── Валідація (2.4) ───────────────────────────────────────────────────

    async def _validate(self, receipt: Receipt) -> None:
        """
        Валідує чек перед фіскалізацією.

        Перевірки:
          - чек не був раніше фіскалізований (fiscal_status != sent);
          - сума чека > 0;
          - ПРРО налаштований (prro_fn задано, ключ КЕП існує).

        Raises:
            PrroFiscalizeError: при порушенні будь-якої перевірки.
        """
        status_str = self._status_str(receipt.fiscal_status)
        if status_str == "sent":
            raise PrroFiscalizeError(
                f"Чек {receipt.id} вже фіскалізований "
                f"(fiscal_number={receipt.fiscal_number})",
                code="PRRO_ALREADY_FISCALIZED",
            )

        total = Decimal(str(getattr(receipt, "total_amount", 0) or 0))
        if total <= 0:
            raise PrroFiscalizeError(
                f"Сума чеку {receipt.id} повинна бути додатною "
                f"(отримано {total})",
                code="PRRO_ZERO_TOTAL",
            )

        fn = await self._settings_repo.get(KEY_PRRO_FN)
        if not fn:
            raise PrroFiscalizeError(
                "ПРРО не налаштований: не задано фіскальний номер (prro_fn)",
                code="PRRO_NOT_CONFIGURED",
            )

        ok, reason = self._context.check_configured()
        if not ok:
            raise PrroFiscalizeError(
                f"ПРРО не налаштований: {reason}",
                code="PRRO_NOT_CONFIGURED",
            )

    # ─── Підготовка даних ──────────────────────────────────────────────────

    async def _auto_fiscalize_enabled(self) -> bool:
        """Чи увімкнена авто-фіскалізація (auto_fiscalize у налаштуваннях)."""
        value = await self._settings_repo.get(KEY_AUTO_FISCALIZE)
        if value is None:
            return False
        return value.strip().lower() in ("1", "true", "yes", "on")

    async def _load_receipt(self, receipt_id: UUID) -> Optional[Receipt]:
        """Завантажує чек з позиціями (selectinload)."""
        stmt = (
            select(Receipt)
            .options(selectinload(Receipt.items))
            .where(Receipt.id == receipt_id)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def _load_product(self, product_id: UUID) -> Optional[Product]:
        """Завантажує товар за ID."""
        result = await self._session.execute(
            select(Product).where(Product.id == product_id)
        )
        return result.scalar_one_or_none()

    # ─── Спліт чеку (2.2) ──────────────────────────────────────────────────

    async def _prepare_split(
        self,
        receipt: Receipt,
        *,
        is_return: bool,
        warnings: list[str],
    ) -> tuple[list[tuple], Optional[UUID]]:
        """
        Визначає ефективні фіскальні кількості та виконує split при потребі.

        Алгоритм (split робиться при фіскалізації):
          1. Для кожної позиції з fiscal_quantity > 0:
             fiscal_qty = min(fiscal_quantity, fiscal_stock) — для продажу;
             для повернення кількість не обмежується залишком.
          2. Якщо сумарно (quantity - fiscal_qty) > 0 хоча б по одній позиції:
             - ОРИГІНАЛЬНИЙ чек стає фіскальним: quantity = fiscal_qty,
               total = price × fiscal_qty; позиції з fiscal_qty = 0
               видаляються; total_amount перераховується;
             - СТВОРЮЄТЬСЯ нефіскальний дублікат (is_fiscal=False,
               fiscal_status=none, split_group_id = id фіскального чека)
               з позиціями quantity - fiscal_qty.

        Returns:
            (planned, split_receipt_id):
              planned — [(ReceiptItem, Decimal qty, Product | None)] для XML;
              split_receipt_id — ID нефіскального дубліката (None без split).
        """
        planned: list[tuple] = []
        split_items: list[tuple[ReceiptItem, Decimal]] = []
        fiscal_items = [
            item for item in receipt.items
            if getattr(item, "fiscal_quantity", 0) and item.fiscal_quantity > 0
        ]

        # Ефективна фіскальна кількість по кожній позиції
        for item in fiscal_items:
            qty = Decimal(str(item.fiscal_quantity))
            total_qty = Decimal(str(item.quantity))
            product = await self._load_product(item.product_id)

            if not is_return and product is not None:
                remaining = Decimal(str(product.fiscal_stock or 0))
                effective = max(Decimal("0"), min(qty, remaining))
                if effective < qty:
                    name = product.title if product.title else str(product.id)
                    warnings.append(
                        f"Товар '{name}': фіскальний залишок {remaining}, "
                        f"заплановано {qty} → фіскалізовано {effective}"
                    )
            else:
                effective = qty

            effective = min(effective, total_qty)
            item.fiscal_quantity = effective
            if effective > 0:
                planned.append((item, effective, product))
            remainder = total_qty - effective
            if remainder > 0:
                split_items.append((item, remainder))

        # Нефіскальні позиції (fiscal_quantity == 0) — повністю у дублікат
        for item in receipt.items:
            if getattr(item, "fiscal_quantity", 0) and item.fiscal_quantity > 0:
                continue
            total_qty = Decimal(str(item.quantity))
            if total_qty > 0:
                split_items.append((item, total_qty))

        # Повністю нефіскальний чек — split не потрібен (нічого фіскалізувати)
        if not planned:
            return planned, None
        if not split_items:
            return planned, None

        # ── Split: коригуємо оригінальний чек (фіскальна частина) ─────────
        for item, effective, _product in planned:
            item.quantity = effective
            item.total = self._item_total(item, effective)

        # Позиції без фіскальної частини видаляємо з фіскального чека
        # (cascade="all, delete-orphan" видалить їх при flush)
        receipt.items = [
            item for item in receipt.items
            if (getattr(item, "fiscal_quantity", 0) or 0) > 0
        ]

        receipt.total_amount = sum(
            (Decimal(str(i.total or 0)) for i in receipt.items), Decimal("0")
        )
        receipt.is_fiscal = True
        receipt.fiscal_status = self._pending_status()

        # ── Створюємо нефіскальний дублікат ───────────────────────────────
        duplicate = Receipt(
            receipt_number=f"NF-{receipt.receipt_number[:20]}-{uuid4().hex[:6]}",
            cashier_id=receipt.cashier_id,
            total_amount=sum(
                (Decimal(str(_i.price)) * qty for _i, qty in split_items),
                Decimal("0"),
            ),
            paid_amount=receipt.paid_amount,
            change_amount=receipt.change_amount,
            debtor_id=receipt.debtor_id,
            is_return=is_return,
            notes=receipt.notes,
            payment_method=receipt.payment_method,
            original_receipt_id=receipt.original_receipt_id,
            return_reason=receipt.return_reason,
            is_fiscal=False,
            fiscal_status=self._none_status(),
            split_group_id=receipt.id,
        )
        self._session.add(duplicate)
        await self._session.flush()

        for item, remainder in split_items:
            self._session.add(ReceiptItem(
                receipt_id=duplicate.id,
                product_id=item.product_id,
                quantity=remainder,
                price=item.price,
                total=self._item_total(item, remainder),
                purchase_price=item.purchase_price,
                fiscal_quantity=Decimal("0"),
            ))

        warnings.append(
            f"Часткова фіскалізація: фіскальний чек #{receipt.receipt_number}, "
            f"нефіскальна частина — чек #{duplicate.receipt_number} "
            f"(split_group_id={duplicate.id})"
        )
        return planned, duplicate.id

    async def _mark_fully_non_fiscal(self, receipt: Receipt) -> None:
        """
        Позначає чек як повністю нефіскальний (fiscal_status=none).

        Викликається, коли після перевірки fiscal_stock не залишилось
        жодної фіскальної позиції (спліт не потрібен — весь чек нефіскальний).
        """
        receipt.fiscal_status = self._none_status()
        receipt.is_fiscal = False
        for item in receipt.items:
            item.fiscal_quantity = Decimal("0")
        await self._session.commit()

    # ─── Побудова payload ──────────────────────────────────────────────────

    def _build_receipt_payload(self, planned: list[tuple]) -> tuple[list, Decimal, dict]:
        """
        Будує позиції XML та податкові групи.

        Returns:
            (items_xml, total, tax_groups)
        """
        items_xml: list[dict] = []
        tax_groups: dict[str, dict] = {}
        total = Decimal("0")

        for item, qty, product in planned:
            price = Decimal(str(item.price))
            item_total = (price * qty).quantize(
                Decimal("0.01"), rounding=ROUND_HALF_UP
            )
            total += item_total

            tax_percent = self._tax_percent(product)
            tx_code = self._tax_code(tax_percent)

            items_xml.append({
                "code": str(item.product_id),
                "name": (product.title if product and product.title
                         else str(item.product_id)),
                "quantity": qty,
                "price": price,
                "total": item_total,
                "tax_rate": tx_code,
            })

            vat = self._vat_amount(item_total, tax_percent)
            group = tax_groups.setdefault(
                tx_code,
                {
                    "tax": tx_code,
                    "tax_percent": tax_percent,
                    "tax_total": Decimal("0"),
                    "dtpr": Decimal("0"),
                    "dtsm": Decimal("0"),
                    "tax_type": "0",
                    "tax_algorithm": "0",
                },
            )
            group["tax_total"] += vat

        return items_xml, total, tax_groups

    def _build_payments(self, receipt, total: Decimal) -> list[dict]:
        """Формує оплати для XML чеку за способом оплати.

        Розбивка оплат відповідає способу оплати чеку (payment_method):
          - "cash"  -> [{code:0, ГОТІВКА, total}] (+ здача change_amount, якщо > 0);
          - "card"  -> [{code:1, КАРТКА, total}];
          - "mixed" -> два платежі: [{code:0, ГОТІВКА, cash_amount},
                                    {code:1, КАРТКА, card_amount}],
                      сума яких = total (з округленням до 2 знаків; останній
                      платіж коригується для уникнення копійчаних розбіжностей).
                      Якщо cash+card != total (напр. split — фіскалізується
                      частина чеку) — коригується ГОТІВКОВА частина.
        """
        method = str(getattr(receipt, "payment_method", "") or "cash").lower()
        total = Decimal(str(total)).quantize(
            Decimal("0.01"), rounding=ROUND_HALF_UP
        )
        payments: list[dict] = []

        if method == "mixed":
            cash = self._payment_share(receipt, "cash_amount")
            card = self._payment_share(receipt, "card_amount")

            # Сума платежів має дорівнювати total. При розбіжності
            # (наприклад, split: фіскалізується лише частина чеку)
            # коригуємо ГОТІВКОВУ частину, щоб cash + card == total.
            if cash + card != total:
                if card > total:
                    card = total
                    cash = Decimal("0")
                else:
                    cash = total - card

            # Копійчана корекція останнього (карткового) платежу:
            # гарантуємо cash + card == total точно до копійки.
            card = (total - cash).quantize(
                Decimal("0.01"), rounding=ROUND_HALF_UP
            )

            if cash > 0:
                payments.append(
                    {"code": "0", "name": "ГОТІВКА", "amount": cash}
                )
            if card > 0:
                payments.append(
                    {"code": "1", "name": "КАРТКА", "amount": card}
                )
            if not payments:
                # Обидві частини нульові (не мало статись після валідації)
                payments.append({"code": "0", "name": "ГОТІВКА", "amount": total})

        elif "card" in method:
            payments.append({"code": "1", "name": "КАРТКА", "amount": total})

        else:
            # Готівка та інші способи (bank_transfer тощо) — як готівковий платіж
            cash_pay: dict = {"code": "0", "name": "ГОТІВКА", "amount": total}
            change = self._payment_share(receipt, "change_amount")
            if change > 0:
                # Здача готівкою (для готівкових чеків зі здачею)
                cash_pay["change"] = change
            payments.append(cash_pay)

        return payments

    @staticmethod
    def _payment_share(receipt, attr: str) -> Decimal:
        """Повертає суму (грн) з атрибута чеку.

        Підтримує доменний Money (атрибут .amount), Decimal та float
        (колонки БД Numeric). None -> Decimal("0").
        """
        value = getattr(receipt, attr, None)
        if value is None:
            return Decimal("0")
        if hasattr(value, "amount"):
            value = value.amount
        try:
            return Decimal(str(value)).quantize(
                Decimal("0.01"), rounding=ROUND_HALF_UP
            )
        except (TypeError, ValueError, ArithmeticError):
            return Decimal("0")

    # ─── Обробка успіху ────────────────────────────────────────────────────

    async def _on_success(
        self,
        *,
        receipt,
        planned: list[tuple],
        total: Decimal,
        local_number: int,
        dat_xml: str,
        mac: str,
        check_sign: str,  # B2: повний підписаний XML (RQ+MAC+підпис) — у чергу as-is
        id_offline: str,  # B4: "offline-{n}" або ""
        response_id: str,
        id_sign: bytes,
        open_shift_id: UUID,
        xml_builder,
        is_return: bool,
        split_receipt_id: Optional[UUID],
        warnings: list[str],
    ) -> FiscalizeResponseDTO:
        """Оновлює чек, залишки, чергу — при успішній відповіді ПРРО."""
        now = datetime.utcnow()
        serial = self._id_sign_str(id_sign, response_id)

        # 9. Оновлюємо чек
        receipt.fiscal_status = "sent"
        receipt.fiscal_number = response_id
        receipt.fiscal_serial = serial
        receipt.fiscal_sent_at = now
        receipt.fiscal_error = None

        # Зменшуємо/збільшуємо fiscal_stock товарів
        for item, qty, product in planned:
            if product is None:
                continue
            current = Decimal(str(product.fiscal_stock or 0))
            if is_return:
                product.fiscal_stock = current + qty
            else:
                product.fiscal_stock = max(Decimal("0"), current - qty)

        # Запис у чергу (sent)
        queue_item = await self._offline_queue.add_document(
            receipt_id=receipt.id,
            shift_id=open_shift_id,
            local_number=local_number,
            check_type=CHECK_TYPE_CHK,
            xml_body=dat_xml,
            mac=mac,
            check_sign=check_sign,  # B2: повний підписаний check_sign
        )
        await self._offline_queue.mark_sent(queue_item.id)

        # Лічильники зміни
        await self._prro_repo.increment_shift_counters(
            shift_id=open_shift_id,
            amount=total,  # Decimal — грошові суми без float
            last_local_number=local_number,
            last_mac=mac,
        )

        await self._context.persist_builder_counters(xml_builder)
        await self._session.commit()

        # 2.6 QR-код: URL перевірки фіскального чеку
        fiscal_check_url = build_fiscal_check_url(
            fiscal_number=response_id,
            amount=total,
            prro_fn=getattr(xml_builder, "rro_fn", ""),
            sent_at=now,
            mac=mac,  # V1: mac = MAC чека (не id_sign) — ДПС §5 «Перевірка чеку»
        )

        logger.info(
            "PRRO_FISCALIZE | чек %s фіскалізовано: №%s (local=%d, сума=%.2f%s)",
            receipt.id, response_id, local_number, total,
            f", split={split_receipt_id}" if split_receipt_id else "",
        )
        return FiscalizeResponseDTO(
            receipt_id=receipt.id,
            fiscal_status="sent",
            fiscal_number=response_id,
            fiscal_serial=serial,
            fiscal_sent_at=now,
            split_receipt_id=split_receipt_id,
            fiscal_check_url=fiscal_check_url,
            warning="; ".join(warnings) or None,
        )

    # ─── Обробка помилки ───────────────────────────────────────────────────

    async def _on_error(
        self,
        *,
        receipt,
        local_number: int,
        dat_xml: str,
        mac: str,
        check_sign: str,  # B2: повний підписаний XML — у чергу as-is
        id_offline: str,  # B4: "offline-{n}" або ""
        open_shift_id: UUID,
        response_status: int,
        error_message: str,
        split_receipt_id: Optional[UUID],
        warnings: list[str],
    ) -> FiscalizeResponseDTO:
        """Фіксує помилку; при ERROR_SAVE/-12 пробує lastChk (дедуплікація)."""
        receipt.fiscal_status = "failed"
        receipt.fiscal_error = error_message

        queue_item = await self._offline_queue.add_document(
            receipt_id=receipt.id,
            shift_id=open_shift_id,
            local_number=local_number,
            check_type=CHECK_TYPE_CHK,
            xml_body=dat_xml,
            mac=mac,
            check_sign=check_sign,  # B2: повний підписаний check_sign
        )
        await self._offline_queue.mark_failed(queue_item.id, error_message)

        # Дедуплікація: сервер міг зберегти чек, але відповідь загубилась
        if response_status in (ERROR_SAVE, ERROR_BAD_HASH_PREV):
            try:
                grpc_client = await self._context.grpc_client()
                last = await grpc_client.last_chk()
                if int(last.status) == 1 and last.id:
                    receipt.fiscal_status = "sent"
                    receipt.fiscal_number = last.id
                    receipt.fiscal_serial = self._id_sign_str(
                        getattr(last, "id_sign", b""), last.id
                    )
                    receipt.fiscal_sent_at = datetime.utcnow()
                    receipt.fiscal_error = None
                    await self._offline_queue.mark_sent(queue_item.id)
                    await self._session.commit()
                    logger.info(
                        "PRRO_FISCALIZE | дедуплікація: чек %s вже збережено (%s)",
                        receipt.id, last.id,
                    )
                    return FiscalizeResponseDTO(
                        receipt_id=receipt.id,
                        fiscal_status="sent",
                        fiscal_number=last.id,
                        fiscal_serial=self._id_sign_str(
                            getattr(last, "id_sign", b""), last.id
                        ),
                        fiscal_sent_at=receipt.fiscal_sent_at,
                        split_receipt_id=split_receipt_id,
                        warning="; ".join(warnings) or None,
                    )
            except Exception as exc:  # noqa: BLE001
                logger.warning("PRRO_FISCALIZE | lastChk не вдався: %s", exc)

        await self._session.commit()
        logger.warning(
            "PRRO_FISCALIZE | чек %s: помилка %s",
            receipt.id, error_message,
        )
        return FiscalizeResponseDTO(
            receipt_id=receipt.id,
            fiscal_status="failed",
            error=error_message,
            split_receipt_id=split_receipt_id,
            warning="; ".join(warnings) or None,
        )

    # ─── Допоміжне ─────────────────────────────────────────────────────────

    @staticmethod
    def _status_str(status) -> str:
        """Статус у вигляді рядка (enum → value)."""
        if status is None:
            return ""
        return status.value if hasattr(status, "value") else str(status)

    @staticmethod
    def _pending_status():
        """Enum статусу pending (для призначення новому чеку)."""
        from app.infrastructure.persistence.models.receipt import FiscalStatus
        return FiscalStatus.PENDING

    @staticmethod
    def _none_status():
        """Enum статусу none."""
        from app.infrastructure.persistence.models.receipt import FiscalStatus
        return FiscalStatus.NONE

    @staticmethod
    def _item_total(item: ReceiptItem, qty: Decimal) -> Decimal:
        """Сума позиції = price × qty (округлення до 2 знаків)."""
        return (Decimal(str(item.price)) * qty).quantize(
            Decimal("0.01"), rounding=ROUND_HALF_UP
        )

    @staticmethod
    def _tax_percent(product) -> Decimal:
        """Ставка ПДВ (%) з товару (за замовчуванням 20%)."""
        if product is not None and getattr(product, "tax_rate", None) is not None:
            try:
                return Decimal(str(product.tax_rate))
            except (TypeError, ValueError):
                pass
        return Decimal("20")

    @staticmethod
    def _tax_code(percent: Decimal) -> str:
        """Зіставлення ставки % → код TX СЗЗД (0=20%, 1=7%, 2=0%, -1=без ПДВ)."""
        if percent == Decimal("7"):
            return "1"
        if percent <= 0:
            return "2"
        return "0"

    @staticmethod
    def _vat_amount(amount: Decimal, percent: Decimal) -> Decimal:
        """Сума ПДВ: amount * percent / (100 + percent)."""
        if percent <= 0:
            return Decimal("0")
        return (amount * percent / (Decimal("100") + percent)).quantize(
            Decimal("0.01"), rounding=ROUND_HALF_UP
        )

    @staticmethod
    def _id_sign_str(id_sign: bytes, fallback: str) -> str:
        """Формує фіскальний серійний номер з id_sign (bytes) або fallback."""
        if id_sign:
            try:
                return id_sign.decode("utf-8", errors="replace")
            except Exception:  # noqa: BLE001
                return id_sign.hex()
        return fallback


__all__ = ["FiscalizeReceiptUseCase", "PrroFiscalizeError"]
