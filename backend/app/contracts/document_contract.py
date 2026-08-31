"""
Контракт модуля Document (Документи).

Визначає інтерфейс для роботи з документами:
прибуткові накладні, переміщення, списання, повернення постачальнику.
"""

from typing import TYPE_CHECKING, Protocol, Union
from uuid import UUID

if TYPE_CHECKING:
    from app.infrastructure.persistence.models.invoice import Invoice
    from app.infrastructure.persistence.models.return_invoice import ReturnInvoice
    from app.infrastructure.persistence.models.transfer import Transfer
    from app.infrastructure.persistence.models.write_off import WriteOff


class DocumentModuleInterface(Protocol):
    """
    Інтерфейс модуля документів.

    Відповідає за:
    - Підтвердження документів (зміна статусу)
    - Скасування документів
    - Координацію оновлення залишків та взаєморозрахунків через Event Bus

    Модуль НЕ викликає напряму StockModule або LedgerModule.
    Замість цього він публікує події, на які підписані відповідні модулі.
    """

    # ─── Події, які публікує ─────────────────────────────────────────────
    # publishes:
    #   - "invoice.confirmed"     — при підтвердженні прибуткової накладної
    #   - "invoice.cancelled"     — при скасуванні прибуткової накладної
    #   - "transfer.confirmed"    — при підтвердженні переміщення
    #   - "transfer.cancelled"    — при скасуванні переміщення
    #   - "return.confirmed"      — при підтвердженні повернення
    #   - "return.cancelled"      — при скасуванні повернення
    #
    # ─── Події, на які підписується ───────────────────────────────────────
    # subscribes:
    #   - (не підписується на події інших модулів)

    # ─── Прибуткова накладна (Invoice) ────────────────────────────────────

    async def confirm_invoice(self, invoice_id: UUID) -> "Invoice":
        """
        Підтверджує прибуткову накладну.

        При підтвердженні:
        1. Змінює статус на CONFIRMED
        2. Публікує подію "invoice.confirmed"
           (StockModule збільшує залишки, LedgerModule створює запис)

        Args:
            invoice_id: UUID накладної.

        Returns:
            Оновлений об'єкт Invoice.

        Raises:
            DocumentNotFound: Якщо накладну не знайдено.
            InvalidDocumentStatus: Якщо накладна не в статусі DRAFT.
        """
        ...

    async def cancel_invoice(self, invoice_id: UUID) -> "Invoice":
        """
        Скасовує прибуткову накладну.

        При скасуванні:
        1. Змінює статус на CANCELLED
        2. Публікує подію "invoice.cancelled"
           (StockModule зменшує залишки, LedgerModule створює запис)

        Args:
            invoice_id: UUID накладної.

        Returns:
            Оновлений об'єкт Invoice.

        Raises:
            DocumentNotFound: Якщо накладну не знайдено.
            InvalidDocumentStatus: Якщо накладна не в статусі CONFIRMED.
        """
        ...

    # ─── Переміщення (Transfer) ──────────────────────────────────────────

    async def confirm_transfer(self, transfer_id: UUID) -> "Transfer":
        """
        Підтверджує переміщення товару.

        При підтвердженні:
        1. Змінює статус на CONFIRMED
        2. Публікує подію "transfer.confirmed"
           (StockModule оновлює залишки)

        Args:
            transfer_id: UUID переміщення.

        Returns:
            Оновлений об'єкт Transfer.
        """
        ...

    async def cancel_transfer(self, transfer_id: UUID) -> "Transfer":
        """
        Скасовує переміщення.

        Args:
            transfer_id: UUID переміщення.

        Returns:
            Оновлений об'єкт Transfer.
        """
        ...

    # ─── Списання (WriteOff) ─────────────────────────────────────────────

    async def confirm_write_off(self, write_off_id: UUID) -> "WriteOff":
        """
        Підтверджує списання товару.

        При підтвердженні публікує подію для оновлення залишків.

        Args:
            write_off_id: UUID списання.

        Returns:
            Оновлений об'єкт WriteOff.
        """
        ...

    # ─── Повернення постачальнику (ReturnInvoice) ────────────────────────

    async def confirm_return_invoice(self, return_id: UUID) -> "ReturnInvoice":
        """
        Підтверджує повернення товару постачальнику.

        При підтвердженні:
        1. Змінює статус на CONFIRMED
        2. Публікує подію "return.confirmed"
           (StockModule зменшує залишки, LedgerModule створює запис)

        Args:
            return_id: UUID повернення.

        Returns:
            Оновлений об'єкт ReturnInvoice.
        """
        ...

    async def cancel_return_invoice(self, return_id: UUID) -> "ReturnInvoice":
        """
        Скасовує повернення постачальнику.

        Args:
            return_id: UUID повернення.

        Returns:
            Оновлений об'єкт ReturnInvoice.
        """
        ...

    # ─── Загальні методи ─────────────────────────────────────────────────

    async def get_document_by_id(
        self,
        document_id: UUID,
        document_type: str,
    ) -> Union["Invoice", "Transfer", "WriteOff", "ReturnInvoice"]:
        """
        Отримує документ за ID та типом.

        Args:
            document_id: UUID документа.
            document_type: Тип документа ("invoice", "transfer", "write_off", "return").

        Returns:
            Об'єкт документа відповідного типу.

        Raises:
            DocumentNotFound: Якщо документ не знайдено.
        """
        ...
