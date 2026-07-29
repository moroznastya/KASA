"""Додає нові опції заокруглення до system_settings

Revision ID: f89706f0cc13
Revises: f89706f0cc12
Create Date: 2026-07-25
"""
from alembic import op
from sqlalchemy import text

revision = 'f89706f0cc13'
down_revision = 'f89706f0cc12'
branch_labels = None
depends_on = None


def upgrade() -> None:
    # Оновлюємо options та description для price_rounding
    op.execute(
        text("""
        UPDATE system_settings 
        SET options = '["1","10","50","100","500"]',
            description = 'До якого номіналу заокруглювати суму в чеку'
        WHERE key = 'price_rounding'
        """)
    )


def downgrade() -> None:
    op.execute(
        text("""
        UPDATE system_settings 
        SET options = '["1","10","50"]',
            description = 'До якого номіналу заокруглювати ціну в чеку'
        WHERE key = 'price_rounding'
        """)
    )
