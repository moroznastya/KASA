"""ADR-0008 §7.1-B (етап E1): push-kinds БАТЬКІВСЬКИХ сутностей —
ідемпотентність `debtors` і `prro_shifts` (client_uuid + partial UNIQUE).

Вузол (рівноправний read-write, ADR-0008) створює БАТЬКІВСЬКІ сутності
локально і віддає їх у хаб через ``POST /api/v1/sync/push``. Без батька
дитина не приймається: ``debtor_payments.debtor_id`` — FK на ``debtors.id``,
тому перший же платіж боржника, створеного на вузлі, відхилявся
(«Боржника … не знайдено в цій точці»).

Дві таблиці отримують ідемпотентний ключ каси:

  * ``debtors.client_uuid``     + partial UNIQUE ``uq_debtors_client_uuid``
    — боржник, створений на вузлі (батько оплат боргу);
  * ``prro_shifts.client_uuid`` + partial UNIQUE ``uq_prro_shifts_client_uuid``
    — фіскальна зміна точки (аудит фіскалізації по точках, §7.1-B3).

``work_sessions`` (B2) міграції НЕ потребує: ``client_uuid`` +
``uq_work_sessions_client_uuid`` уже створені шаром 0013 (модель
``persistence/models/work_session.py``).

Патерн DROP IF EXISTS + CREATE — точно як 0013/0016/0017/0018 (захист від
рудиментарного індексу з тим самим ім'ям на іншій колонці).

Ім'я ревізії — 27 символів: ``alembic_version.version_num`` це ``varchar(32)``
(див. docstring 0017).

Revision ID: 0019_peer_parents_push_kinds
Revises: 0018_return_debtor_ledger_idem
Create Date: 2026-09-12
"""

from typing import Sequence, Union

from alembic import op


# revision identifiers, used by Alembic.
revision: str = "0019_peer_parents_push_kinds"
down_revision: Union[str, Sequence[str], None] = "0018_return_debtor_ledger_idem"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# (таблиця, ім'я partial UNIQUE-індексу) — однакова форма для обох батьків.
TARGETS: tuple[tuple[str, str], ...] = (
    ("debtors", "uq_debtors_client_uuid"),
    ("prro_shifts", "uq_prro_shifts_client_uuid"),
)


def upgrade() -> None:
    """client_uuid + partial UNIQUE на батьківських сутностях вузла."""
    for table, index in TARGETS:
        op.execute(f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS client_uuid uuid")
        op.execute(f"DROP INDEX IF EXISTS {index}")
        op.execute(
            f"CREATE UNIQUE INDEX {index} ON {table} (client_uuid) "
            "WHERE client_uuid IS NOT NULL"
        )


def downgrade() -> None:
    """Прибрати client_uuid та partial UNIQUE з батьківських сутностей."""
    for table, index in TARGETS:
        op.execute(f"DROP INDEX IF EXISTS {index}")
        op.execute(f"ALTER TABLE {table} DROP COLUMN IF EXISTS client_uuid")
