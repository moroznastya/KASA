"""ЕТАП 7b (LOW 5.5): drop рудиментарної колонки receipts.client_receipt_uuid.

Revision: 0014 (короткий id — alembic_version varchar(32)).

Колонка `client_receipt_uuid` — kasa-спадок schema.sql, що лишився поряд із
`client_uuid` (offline-first, Alembic 0013). Перевірено grep по backend/app і
всіх Rust-крейтах: колонка НЕ читається жодним кодом і НЕ згадується в
жодній Python-моделі/схемі. Безпечно видалити.

УВАГА: в 0013 зафіксовано рудиментарний індекс `uq_receipts_client_uuid` на
цій колонці (kasa-спадок). 0013 перестворює індекс на `client_uuid`
(DROP INDEX IF EXISTS + CREATE) — отже індекс на старій колонці вже
відсутній на БД, мігрованих до 0013. Тут — лише DROP COLUMN.

Revision ID: 0014_drop_receipts_client_receipt_uuid
Revises: 0013_sync_push_idempotency
Create Date: 2026-09-02
"""

from typing import Sequence, Union

from alembic import op
from sqlalchemy import text


# revision identifiers, used by Alembic.
revision: str = "0014"
down_revision: Union[str, Sequence[str], None] = "0013_sync_push_idempotency"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Видалити мертву колонку (якщо існує — БД без неї не ламаються)."""
    bind = op.get_bind()
    has_col = bind.execute(
        text("SELECT 1 FROM information_schema.columns "
            "WHERE table_name='receipts' AND column_name='client_receipt_uuid'")
    ).first()
    if has_col:
        op.execute(
            "DROP INDEX IF EXISTS uq_receipts_client_receipt_uuid"
        )
        op.drop_column("receipts", "client_receipt_uuid")


def downgrade() -> None:
    """Повернути колонку (без даних — рудимент, лише для відкату ланцюга)."""
    op.execute(
        "ALTER TABLE receipts ADD COLUMN IF NOT EXISTS "
        "client_receipt_uuid uuid"
    )
