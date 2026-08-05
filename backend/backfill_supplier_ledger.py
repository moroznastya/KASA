"""
Бекфілл журналу взаєморозрахунків (supplier_ledger) для Kasa POS.

Що робить:
  1. Для кожної confirmed-накладної (invoices.status='confirmed') перевіряє,
     чи існує запис у supplier_ledger з document_id = invoice.id.
     Якщо НЕ існує — створює INVOICE-запис (+total_amount).
     Дублікати INVOICE-записів з тим самим document_id — видаляє (лишає
     найстаріший за created_at).
  2. Перераховує баланси (balance_after) для ВСІХ постачальників з
     ledger-записами як послідовний ланцюжок, відсортований за
     (operation_date ASC, created_at ASC, id ASC), починаючи з 0.
  3. Виводить звіт: створено/видалено/оновлено, підсумкова таблиця
     балансів та звірка з confirmed-накладними.

Безпека: читає invoices/suppliers, пише ТІЛЬКИ в supplier_ledger.
НЕ змінює код, НЕ запускає сервер.

Запуск (з кореня backend/):
    ./venv/bin/python3 backfill_supplier_ledger.py
"""

import asyncio
from datetime import datetime
from decimal import Decimal

from sqlalchemy import func, select

from app.database import async_session
from app.infrastructure.persistence.models.invoice import Invoice, InvoiceStatus
from app.infrastructure.persistence.models.supplier import Supplier
from app.infrastructure.persistence.models.supplier_ledger import (
    LedgerOperationType,
    SupplierLedger,
)

DEC2 = Decimal("0.01")


def _naive(dt: datetime | None) -> datetime:
    """Повертає offset-naive datetime (БД зберігає timestamp without time zone)."""
    if dt is None:
        return datetime.utcnow()
    if dt.tzinfo is not None:
        return dt.replace(tzinfo=None)
    return dt


def _dec(value) -> Decimal:
    """Безпечне перетворення в Decimal(12,2)."""
    if value is None:
        return Decimal("0")
    return Decimal(str(value)).quantize(DEC2)


async def main() -> None:
    created = 0
    deleted_dups = 0
    updated = 0

    async with async_session() as session:
        # ──────────────────────────────────────────────────────────────
        # 1. Створення відсутніх INVOICE-записів
        # ──────────────────────────────────────────────────────────────
        invoices = (
            await session.execute(
                select(Invoice).where(Invoice.status == InvoiceStatus.CONFIRMED)
            )
        ).scalars().all()

        # Усі document_id, що вже є в ledger (щоб не дублювати)
        existing_docs: set = set(
            (
                await session.execute(
                    select(SupplierLedger.document_id).where(
                        SupplierLedger.document_id.is_not(None)
                    )
                )
            ).scalars().all()
        )

        for inv in invoices:
            if inv.id in existing_docs:
                continue  # запис вже існує — не дублюємо
            entry = SupplierLedger(
                supplier_id=inv.supplier_id,
                operation_type=LedgerOperationType.INVOICE,
                document_id=inv.id,
                document_number=inv.number,
                amount=_dec(inv.total_amount),
                balance_after=Decimal("0"),  # тимчасово, перерахуємо на кроці 2
                operation_date=_naive(inv.invoice_date or inv.created_at),
                notes=f"Прибуткова накладна №{inv.number}",
            )
            session.add(entry)
            existing_docs.add(inv.id)  # захист від дублів у межах батчу
            created += 1

        # ── Видалення дублікатів INVOICE з тим самим document_id ──
        dup_rows = (
            await session.execute(
                select(
                    SupplierLedger.document_id,
                    func.count(SupplierLedger.id),
                )
                .where(
                    SupplierLedger.operation_type == LedgerOperationType.INVOICE,
                    SupplierLedger.document_id.is_not(None),
                )
                .group_by(SupplierLedger.document_id)
                .having(func.count(SupplierLedger.id) > 1)
            )
        ).all()

        for doc_id, _cnt in dup_rows:
            recs = (
                await session.execute(
                    select(SupplierLedger)
                    .where(
                        SupplierLedger.operation_type == LedgerOperationType.INVOICE,
                        SupplierLedger.document_id == doc_id,
                    )
                    .order_by(
                        SupplierLedger.created_at.asc(),
                        SupplierLedger.id.asc(),
                    )
                )
            ).scalars().all()
            for extra in recs[1:]:  # лишаємо найстаріший (created_at ASC)
                await session.delete(extra)
                deleted_dups += 1

        await session.flush()  # нові записи отримають id/created_at

        # ──────────────────────────────────────────────────────────────
        # 2. Перерахунок балансів для ВСІХ постачальників з ledger
        # ──────────────────────────────────────────────────────────────
        supplier_ids: set = set(
            (
                await session.execute(
                    select(SupplierLedger.supplier_id).distinct()
                )
            ).scalars().all()
        )

        all_recs = (
            await session.execute(
                select(SupplierLedger).where(
                    SupplierLedger.supplier_id.in_(supplier_ids)
                )
            )
        ).scalars().all()

        by_supplier: dict = {}
        for rec in all_recs:
            by_supplier.setdefault(rec.supplier_id, []).append(rec)

        for _sid, recs in by_supplier.items():
            # Сортування: operation_date ASC, created_at ASC, id ASC
            recs.sort(
                key=lambda r: (
                    _naive(r.operation_date),
                    r.created_at or datetime.min,
                    str(r.id),
                )
            )
            balance = Decimal("0")
            for r in recs:
                balance += _dec(r.amount)
                r.balance_after = balance
                updated += 1

        await session.commit()
        print(f"[OK] Записи: створено={created}, видалено_дублікатів={deleted_dups}, "
              f"оновлено_балансів={updated}")

    # ──────────────────────────────────────────────────────────────
    # 3 + 5. Звірка та підсумкова таблиця (свіже читання після COMMIT)
    # ──────────────────────────────────────────────────────────────
    await report(created, deleted_dups, updated)


async def report(created: int, deleted_dups: int, updated: int) -> None:
    async with async_session() as session:
        suppliers = {
            s.id: s.name
            for s in (await session.execute(select(Supplier))).scalars().all()
        }

        ledger_rows = (
            await session.execute(select(SupplierLedger))
        ).scalars().all()

        inv_rows = (
            await session.execute(
                select(Invoice).where(Invoice.status == InvoiceStatus.CONFIRMED)
            )
        ).scalars().all()

        # ── Дані по постачальниках ──
        ledger_by_sup: dict = {}
        for r in ledger_rows:
            ledger_by_sup.setdefault(r.supplier_id, []).append(r)

        inv_by_sup: dict = {}
        for i in inv_rows:
            inv_by_sup.setdefault(i.supplier_id, []).append(i)

        print("\n" + "=" * 118)
        print("ПІДСУМКОВА ТАБЛИЦЯ ПОСТАЧАЛЬНИКІВ (supplier_ledger)")
        print("=" * 118)
        header = (
            f"{'Постачальник':<32}{'Записів':>8}{'Sum(amount)':>14}"
            f"{'Last balance':>14}{'Накл.(conf)':>11}{'Sum накл.':>13}"
            f"{'Очікув.':>12}  {'Статус':<10}"
        )
        print(header)
        print("-" * 118)

        all_ok = True
        for sup_id in sorted(ledger_by_sup, key=lambda x: suppliers.get(x, "")):
            recs = ledger_by_sup[sup_id]
            recs_sorted = sorted(
                recs,
                key=lambda r: (
                    _naive(r.operation_date),
                    r.created_at or datetime.min,
                    str(r.id),
                ),
            )
            last_balance = _dec(recs_sorted[-1].balance_after)
            sum_amount = sum((_dec(r.amount) for r in recs), Decimal("0"))
            invs = inv_by_sup.get(sup_id, [])
            sum_inv = sum((_dec(i.total_amount) for i in invs), Decimal("0"))
            sum_negative = sum(
                (_dec(r.amount) for r in recs if _dec(r.amount) < 0),
                Decimal("0"),
            )
            # Формула з ТЗ: balance = sum(confirmed invoices) + sum(amount<0)
            expected = sum_inv + sum_negative
            ok = last_balance == expected
            all_ok = all_ok and ok

            print(
                f"{suppliers.get(sup_id, sup_id):<32}{len(recs):>8}"
                f"{sum_amount:>14.2f}{last_balance:>14.2f}"
                f"{len(invs):>11}{sum_inv:>13.2f}{expected:>12.2f}  "
                f"{'✔ збіг' if ok else '✘ РОЗБІЖНІСТЬ':<10}"
            )

        print("-" * 118)
        print(f"\nПідсумок: створено={created}, видалено_дублікатів={deleted_dups}, "
              f"оновлено_балансів={updated}")
        print(f"Звірка балансів з накладними: {'ВСІ ЗБІГЛИСЯ ✔' if all_ok else 'Є РОЗБІЖНОСТІ ✘'}")

        # Постачальники з ledger, але БЕЗ confirmed накладних
        only_ledger = [s for s in ledger_by_sup if s not in inv_by_sup]
        if only_ledger:
            print("\nПостачальники з ledger, але БЕЗ confirmed накладних "
                  "(баланс лишився як є):")
            for sid in only_ledger:
                recs = ledger_by_sup[sid]
                print(f"  - {suppliers.get(sid, sid)}: sum(amount)="
                      f"{sum((_dec(r.amount) for r in recs), Decimal('0')):.2f}, "
                      f"last balance_after={_dec(recs[-1].balance_after):.2f}")


if __name__ == "__main__":
    asyncio.run(main())
