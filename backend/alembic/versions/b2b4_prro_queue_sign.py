"""add prro_queue check_sign/id_offline (B2/B4)

Додає колонки до prro_queue_items:
  - check_sign  (B2: ідемпотентність sync — повний підписаний XML as-is)
  - id_offline  (B4: офлайн-ідентифікатор offline-чека)

Revision ID: b2b4_prro_queue_sign
Revises: 0005_onboarding_completed
Create Date: 2026-08-27
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa

# revision identifiers, used by Alembic.
revision: str = "b2b4_prro_queue_sign"
down_revision: Union[str, None] = "0005_onboarding_completed"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    op.add_column(
        "prro_queue_items",
        sa.Column(
            "check_sign",
            sa.Text(),
            nullable=True,
            comment="B2: повний підписаний XML (RQ+MAC+підпис) — sync as-is",
        ),
    )
    op.add_column(
        "prro_queue_items",
        sa.Column(
            "id_offline",
            sa.Text(),
            nullable=True,
            comment='B4: офлайн-ідентифікатор offline-чека (offline-{n})',
        ),
    )


def downgrade() -> None:
    op.drop_column("prro_queue_items", "id_offline")
    op.drop_column("prro_queue_items", "check_sign")
