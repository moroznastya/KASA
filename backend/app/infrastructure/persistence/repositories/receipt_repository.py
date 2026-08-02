"""
Repository Implementation: SQLAlchemyReceiptRepository.

Реалізація IReceiptRepository з використанням SQLAlchemy.

Оптимізація N+1:
  - receipt → items (to-many)        → selectinload
  - receipt → items → product (to-one) → joinedload (вкладений)
  - receipt → cashier / debtor (to-one) → joinedload
"""

from datetime import datetime, timezone, timedelta
from decimal import Decimal
from typing import Optional
from uuid import UUID, uuid4

from sqlalchemy import func, or_, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import joinedload, selectinload

from app.domain.repositories import IReceiptRepository
from app.infrastructure.persistence.models.product import Product
from app.infrastructure.persistence.models.receipt import (
    Receipt,
    ReceiptItem,
    ReceiptPaymentMethod,
    ReceiptType,
)

# Спільні опції eager-loading для чеку з повним вмістом (позиції + товари)
_RECEIPT_DETAIL_OPTIONS = (
    joinedload(Receipt.cashier),
    joinedload(Receipt.debtor),
    selectinload(Receipt.items).joinedload(ReceiptItem.product),
)

# Спільні опції eager-loading для списків чеків (позиції + товари)
_RECEIPT_LIST_OPTIONS = (
    selectinload(Receipt.items).joinedload(ReceiptItem.product),
)


class SQLAlchemyReceiptRepository(IReceiptRepository):
    """
    SQLAlchemy реалізація репозиторію чеків продажу.

    Працює з моделями Receipt та ReceiptItem.
    """

    def __init__(self, session: AsyncSession):
        self._session = session

    @staticmethod
    def _to_orm(receipt) -> "Receipt":
        """Конвертує доменну Receipt entity в ORM Receipt (якщо це не ORM)."""
        if isinstance(receipt, Receipt):
            return receipt
        from app.infrastructure.persistence.models.receipt import (
            FiscalStatus as OrmFiscalStatus,
        )

        orm = Receipt(
            id=receipt.id,
            receipt_number=receipt.number or f"RCPT-{datetime.now().strftime('%Y%m%d')}-{uuid4().hex[:6].upper()}",
            receipt_type=ReceiptType.SALE
            if getattr(receipt, "receipt_type", "sale") == "sale"
            else ReceiptType.RETURN,
            cashier_id=receipt.cashier_id,
            total_amount=float(receipt.total.amount) if receipt.total is not None else 0.0,
            change_amount=float(receipt.change_amount.amount) if receipt.change_amount is not None else None,
            is_return=getattr(receipt, "receipt_type", "sale") == "return",
            notes=receipt.notes or None,
            payment_method=ReceiptPaymentMethod(receipt.payment_method.value)
            if hasattr(receipt.payment_method, "value")
            else ReceiptPaymentMethod(receipt.payment_method),
            is_fiscal=receipt.is_fiscal,
            fiscal_status=OrmFiscalStatus(receipt.fiscal_status.value)
            if hasattr(receipt.fiscal_status, "value")
            else OrmFiscalStatus(receipt.fiscal_status),
            split_group_id=receipt.split_group_id,
        )
        orm.items = [
            ReceiptItem(
                product_id=item.product_id,
                quantity=float(item.quantity.value),
                price=float(item.price.amount),
                total=float(item.total.amount),
            )
            for item in receipt.items
        ]
        return orm

    async def save(self, receipt: Receipt) -> Receipt:
        """Зберігає новий чек (доменну entity або ORM-модель)."""
        orm = self._to_orm(receipt)
        self._session.add(orm)
        await self._session.flush()
        return orm

    async def find_by_id(self, receipt_id: UUID) -> Optional[Receipt]:
        """Знаходить чек за ID (з позиціями, товарами, касиром, боржником)."""
        stmt = (
            select(Receipt)
            .where(Receipt.id == receipt_id)
            .options(*_RECEIPT_DETAIL_OPTIONS)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_number(self, number: str) -> Optional[Receipt]:
        """Знаходить чек за номером (з позиціями, товарами, касиром, боржником)."""
        stmt = (
            select(Receipt)
            .where(Receipt.receipt_number == number)
            .options(*_RECEIPT_DETAIL_OPTIONS)
        )
        result = await self._session.execute(stmt)
        return result.scalar_one_or_none()

    async def find_by_date_range(
        self,
        date_from: datetime,
        date_to: datetime,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Receipt], int]:
        """Знаходить чеки за діапазоном дат з пагінацією (з позиціями та товарами)."""
        base_stmt = select(Receipt).where(
            Receipt.created_at >= date_from,
            Receipt.created_at <= date_to,
        )

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_RECEIPT_LIST_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(Receipt.created_at.desc())
        )
        result = await self._session.execute(stmt)
        receipts = list(result.scalars().all())

        return receipts, total

    async def search(
        self,
        query: Optional[str] = None,
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        payment_method: Optional[str] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Receipt], int]:
        """Розширений пошук чеків (з позиціями та товарами)."""
        base_stmt = select(Receipt)

        if query:
            like_pattern = f"%{query}%"
            base_stmt = base_stmt.where(
                or_(
                    Receipt.receipt_number.ilike(like_pattern),
                    Receipt.notes.ilike(like_pattern),
                )
            )
        if date_from is not None:
            base_stmt = base_stmt.where(Receipt.created_at >= date_from)
        if date_to is not None:
            base_stmt = base_stmt.where(Receipt.created_at <= date_to)
        if payment_method is not None:
            base_stmt = base_stmt.where(
                Receipt.payment_method == ReceiptPaymentMethod(payment_method)
            )

        count_stmt = select(func.count()).select_from(base_stmt.subquery())
        total_result = await self._session.execute(count_stmt)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_stmt
            .options(*_RECEIPT_LIST_OPTIONS)
            .offset(offset)
            .limit(size)
            .order_by(Receipt.created_at.desc())
        )
        result = await self._session.execute(stmt)
        receipts = list(result.scalars().all())

        return receipts, total

    async def delete(self, receipt_id: UUID) -> None:
        """Видаляє чек за ID."""
        receipt = await self.find_by_id(receipt_id)
        if receipt is not None:
            await self._session.delete(receipt)
            await self._session.flush()

    async def count(self) -> int:
        """Повертає загальну кількість чеків."""
        stmt = select(func.count()).select_from(Receipt)
        result = await self._session.execute(stmt)
        return result.scalar() or 0

    async def get_daily_total(self, date: datetime) -> float:
        """
        Повертає загальну суму продажів за день.

        Враховує тільки чеки продажу (sale), без повернень.
        """
        start_of_day = datetime(date.year, date.month, date.day)
        end_of_day = datetime(
            date.year, date.month, date.day, 23, 59, 59
        )

        stmt = select(func.coalesce(func.sum(Receipt.total_amount), 0)).where(
            Receipt.created_at >= start_of_day,
            Receipt.created_at <= end_of_day,
            Receipt.is_return.is_(False),
        )
        result = await self._session.execute(stmt)
        return float(result.scalar() or 0.0)

    # ─── Статистика та звіти ───────────────────────────────────────────────

    async def get_today_stats(self) -> dict:
        """
        Повертає статистику чеків за сьогодні (UTC).

        Returns:
            dict: {total_sales, total_returns, total_profit, total_vat,
                   receipts_count, items_sold, date}
        """
        now_utc = datetime.now(timezone.utc)
        today_start = datetime(now_utc.year, now_utc.month, now_utc.day)
        today_end = datetime(now_utc.year, now_utc.month, now_utc.day, 23, 59, 59, 999999)

        # Загальна сума продажів сьогодні
        sales_sum = await self._session.execute(
            select(func.coalesce(func.sum(Receipt.total_amount), 0))
            .where(Receipt.receipt_type == ReceiptType.SALE)
            .where(Receipt.created_at >= today_start, Receipt.created_at <= today_end)
        )
        total_sales = float(sales_sum.scalar() or 0)

        # Загальна сума повернень сьогодні
        returns_sum = await self._session.execute(
            select(func.coalesce(func.sum(Receipt.total_amount), 0))
            .where(Receipt.receipt_type == ReceiptType.RETURN)
            .where(Receipt.created_at >= today_start, Receipt.created_at <= today_end)
        )
        total_returns = float(returns_sum.scalar() or 0)

        # Кількість чеків сьогодні
        count_result = await self._session.execute(
            select(func.count(Receipt.id))
            .where(Receipt.created_at >= today_start, Receipt.created_at <= today_end)
        )
        receipts_count = count_result.scalar() or 0

        # Кількість проданих товарів сьогодні
        items_result = await self._session.execute(
            select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
            .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
            .where(Receipt.receipt_type == ReceiptType.SALE)
            .where(Receipt.created_at >= today_start, Receipt.created_at <= today_end)
        )
        items_sold = int(items_result.scalar() or 0)

        # Чистий прибуток та ПДВ за сьогодні
        profit_result = await self._session.execute(
            select(ReceiptItem)
            .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
            .where(Receipt.receipt_type == ReceiptType.SALE)
            .where(Receipt.created_at >= today_start, Receipt.created_at <= today_end)
            .options(joinedload(ReceiptItem.product))
        )
        today_items = list(profit_result.scalars().all())
        total_profit = Decimal("0")
        total_vat = Decimal("0")
        for item in today_items:
            purchase_price = item.purchase_price
            if purchase_price is None and item.product and item.product.cost_price is not None:
                purchase_price = float(item.product.cost_price)
            if purchase_price is not None:
                item_total = Decimal(str(item.total))
                item_cost = Decimal(str(purchase_price)) * Decimal(str(item.quantity))
                total_profit += item_total - item_cost

            # ПДВ: ПДВ = (ціна_з_пдв * ставка) / (1 + ставка)
            tax_rate = None
            if item.product and item.product.tax_rate is not None:
                tax_rate = Decimal(str(item.product.tax_rate))
            if tax_rate and tax_rate != 0:
                rate = tax_rate / Decimal("100")
                total = Decimal(str(item.price)) * Decimal(str(item.quantity))
                total_vat += (total * rate) / (Decimal("1") + rate)

        return {
            "total_sales": total_sales,
            "total_returns": total_returns,
            "total_profit": float(total_profit),
            "total_vat": float(total_vat),
            "receipts_count": receipts_count,
            "items_sold": items_sold,
            "date": now_utc.strftime("%Y-%m-%d"),
        }

    async def search_with_details(
        self,
        q: str = "",
        date_from: Optional[datetime] = None,
        date_to: Optional[datetime] = None,
        receipt_type: Optional[ReceiptType] = None,
        page: int = 1,
        size: int = 20,
    ) -> tuple[list[Receipt], int]:
        """
        Пошук чеків для повернень (за номером або назвою товару).

        Шукає за номером чеку (ILIKE) або за назвою товару в позиціях.
        Фільтрує за датою та типом чеку.

        Returns:
            Кортеж (список чеків, загальна кількість).
        """
        receipt_type = receipt_type or ReceiptType.SALE

        # Нормалізуємо date_to: якщо передано лише дату (00:00:00),
        # встановлюємо 23:59:59.999999, щоб включити всі чеки за цей день
        if date_to is not None and date_to.hour == 0 and date_to.minute == 0 and date_to.second == 0 and date_to.microsecond == 0:
            date_to = date_to.replace(hour=23, minute=59, second=59, microsecond=999999)

        base_query = select(Receipt).where(Receipt.receipt_type == receipt_type)
        count_query = select(func.count(Receipt.id)).where(Receipt.receipt_type == receipt_type)

        if date_from:
            base_query = base_query.where(Receipt.created_at >= date_from)
            count_query = count_query.where(Receipt.created_at >= date_from)
        if date_to:
            base_query = base_query.where(Receipt.created_at <= date_to)
            count_query = count_query.where(Receipt.created_at <= date_to)

        if q.strip():
            search_pattern = f"%{q.strip()}%"
            base_query = (
                base_query
                .join(ReceiptItem, Receipt.id == ReceiptItem.receipt_id)
                .join(Product, ReceiptItem.product_id == Product.id)
                .where(or_(
                    Receipt.receipt_number.ilike(search_pattern),
                    Product.title.ilike(search_pattern),
                ))
            )
            count_query = (
                count_query
                .join(ReceiptItem, Receipt.id == ReceiptItem.receipt_id)
                .join(Product, ReceiptItem.product_id == Product.id)
                .where(or_(
                    Receipt.receipt_number.ilike(search_pattern),
                    Product.title.ilike(search_pattern),
                ))
            )

        total_result = await self._session.execute(count_query)
        total = total_result.scalar() or 0

        offset = (page - 1) * size
        stmt = (
            base_query
            .options(
                selectinload(Receipt.items).joinedload(ReceiptItem.product),
                joinedload(Receipt.cashier),
            )
            .order_by(Receipt.created_at.desc())
            .offset(offset)
            .limit(size)
            .distinct()
        )
        result = await self._session.execute(stmt)
        receipts = list(result.scalars().all())

        return receipts, total

    async def find_recent_sales_by_product(
        self,
        query: str,
        limit: int = 5,
    ) -> list[dict]:
        """
        Останні продажі товарів за штрих-кодом або назвою (для повернення).

        Returns:
            list[dict]: [{product: {...}, total_sold, total_returned,
                          returnable, recent_sales: [...]}]
        """
        product_result = await self._session.execute(
            select(Product)
            .where(or_(
                Product.barcode == query,
                Product.title.ilike(f"%{query}%"),
            ))
            .order_by(Product.title)
            .limit(20)
        )
        products = list(product_result.scalars().all())

        items_list = []
        for product in products:
            recent_items_result = await self._session.execute(
                select(ReceiptItem)
                .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
                .where(ReceiptItem.product_id == product.id)
                .where(Receipt.receipt_type == ReceiptType.SALE)
                .order_by(Receipt.created_at.desc())
                .limit(limit)
                .options(joinedload(ReceiptItem.receipt))
            )
            recent_items = list(recent_items_result.scalars().all())

            total_sold, total_returned = await self.get_sold_returned_totals(product.id)
            returnable = max(Decimal("0"), total_sold - total_returned)

            recent_sales = []
            for item in recent_items:
                recent_sales.append({
                    "receipt_id": item.receipt_id,
                    "receipt_number": item.receipt.receipt_number if item.receipt else "",
                    "created_at": item.created_at,
                    "quantity": Decimal(str(item.quantity)),
                    "price": Decimal(str(item.price)),
                })

            items_list.append({
                "product": {
                    "id": product.id,
                    "title": product.title,
                    "barcode": product.barcode,
                    "price": Decimal(str(product.price)),
                    "unit": product.unit,
                },
                "total_sold": total_sold,
                "total_returned": total_returned,
                "returnable": returnable,
                "recent_sales": recent_sales,
            })

        return items_list

    async def get_sold_returned_totals(
        self,
        product_id: UUID,
    ) -> tuple[Decimal, Decimal]:
        """
        Повертає (total_sold, total_returned) для товару.

        Returns:
            Кортеж (продано, повернуто) у Decimal.
        """
        sold_result = await self._session.execute(
            select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
            .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
            .where(ReceiptItem.product_id == product_id)
            .where(Receipt.receipt_type == ReceiptType.SALE)
        )
        total_sold = Decimal(str(sold_result.scalar() or 0))

        returned_result = await self._session.execute(
            select(func.coalesce(func.sum(ReceiptItem.quantity), 0))
            .join(Receipt, ReceiptItem.receipt_id == Receipt.id)
            .where(ReceiptItem.product_id == product_id)
            .where(Receipt.receipt_type == ReceiptType.RETURN)
        )
        total_returned = Decimal(str(returned_result.scalar() or 0))

        return total_sold, total_returned

    async def get_returnable_quantity(self, product_id: UUID) -> Decimal:
        """
        Скільки одиниць товару ще можна повернути.

        Returns:
            Decimal: max(0, продано - повернуто).
        """
        total_sold, total_returned = await self.get_sold_returned_totals(product_id)
        return max(Decimal("0"), total_sold - total_returned)

    async def find_items_with_products(self, receipt_id: UUID) -> list[ReceiptItem]:
        """
        Знаходить позиції чеку з підвантаженими товарами.

        Args:
            receipt_id: ID чеку.

        Returns:
            Список ReceiptItem (з .product).
        """
        stmt = (
            select(ReceiptItem)
            .where(ReceiptItem.receipt_id == receipt_id)
            .options(joinedload(ReceiptItem.product))
            .order_by(ReceiptItem.created_at)
        )
        result = await self._session.execute(stmt)
        return list(result.scalars().all())
