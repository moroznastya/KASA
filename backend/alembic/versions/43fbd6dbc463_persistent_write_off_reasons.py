"""persistent write_off reasons list; reason enum -> string

Revision ID: 43fbd6dbc463
Revises: c71e80a10fd9
Create Date: 2026-08-09 22:10:00.000000+00:00

Зміни:
1. Нова таблиця write_off_reasons — персистентний довідник причин списання.
2. Seed: 5 стандартних причин українською.
3. write_offs.reason: ENUM(write_off_reason) -> VARCHAR(100) з маппінгом
   старих enum-значень на українські назви.
4. DROP TYPE write_off_reason (використовувався тільки в write_offs —
   перевірено через information_schema).
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa

# revision identifiers, used by Alembic.
revision: str = '43fbd6dbc463'
down_revision: Union[str, None] = 'c71e80a10fd9'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# Стандартні причини (seed)
STANDARD_REASONS = [
    {"id": "10000000-0000-4000-8000-000000000001", "name": "Прострочений термін"},
    {"id": "10000000-0000-4000-8000-000000000002", "name": "Пошкодження / бій"},
    {"id": "10000000-0000-4000-8000-000000000003", "name": "Брак / дефект"},
    {"id": "10000000-0000-4000-8000-000000000004", "name": "Крадіжка"},
    {"id": "10000000-0000-4000-8000-000000000005", "name": "Інвентаризація (нестача)"},
]

# Маппінг старих enum-значень -> українські назви
REASON_MAPPING = {
    "expired": "Прострочений термін",
    "damaged": "Пошкодження / бій",
    "defect": "Брак / дефект",
    "theft": "Крадіжка",
    "inventory": "Інвентаризація (нестача)",
    "other": "Інше",
}


def upgrade() -> None:
    """Створює довідник причин, переводить reason на назви рядком."""
    # 1. Таблиця довідника
    reasons_table = op.create_table(
        'write_off_reasons',
        sa.Column('id', sa.UUID(), nullable=False),
        sa.Column('name', sa.String(length=100), nullable=False),
        sa.Column('is_active', sa.Boolean(), nullable=False, server_default=sa.text('true')),
        sa.Column('created_at', sa.DateTime(), nullable=False, server_default=sa.text('now()')),
        sa.PrimaryKeyConstraint('id'),
        sa.UniqueConstraint('name'),
        comment='Персистентний довідник причин списання товару',
    )

    # 2. Seed стандартних причин
    from datetime import datetime
    now = datetime.utcnow()
    op.bulk_insert(
        reasons_table,
        [
            {**r, "is_active": True, "created_at": now}
            for r in STANDARD_REASONS
        ],
    )

    # 3. ENUM -> VARCHAR(100) з маппінгом старих значень на українські назви
    case_when = " ".join(
        f"WHEN '{old}' THEN '{new}'" for old, new in REASON_MAPPING.items()
    )
    op.execute(
        sa.text(
            f"ALTER TABLE write_offs "
            f"ALTER COLUMN reason TYPE VARCHAR(100) "
            f"USING CASE reason::text {case_when} ELSE reason::text END"
        )
    )

    # 4. Видаляємо enum-тип (використовувався лише в write_offs)
    op.execute(sa.text("DROP TYPE write_off_reason"))


def downgrade() -> None:
    """Повертає enum-тип та видаляє довідник."""
    # 1. Відновлюємо enum-тип
    op.execute(sa.text(
        "CREATE TYPE write_off_reason AS ENUM "
        "('expired', 'damaged', 'defect', 'theft', 'inventory', 'other')"
    ))

    # 2. VARCHAR -> ENUM (зворотний маппінг: українська назва -> enum-значення)
    case_when = " ".join(
        f"WHEN '{new}' THEN '{old}'::write_off_reason"
        for old, new in REASON_MAPPING.items()
    )
    op.execute(
        sa.text(
            f"ALTER TABLE write_offs "
            f"ALTER COLUMN reason TYPE write_off_reason "
            f"USING CASE reason {case_when} ELSE 'other'::write_off_reason END"
        )
    )

    # 3. Видаляємо довідник
    op.drop_table('write_off_reasons')
