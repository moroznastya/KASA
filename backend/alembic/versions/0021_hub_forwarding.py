"""ADR-0008 §7.1-A1 (етап E3): форвардинг прийнятого вузлом у хаб —
`sync_log.hub_forwarded_at/hub_forward_status` + черга `hub_outbox`.

Проблема, яку закриває міграція:

  * вузол (повний read-write PG точки) приймає документ каси ЛОКАЛЬНО, але
    вгору (в хаб-арбітр мережі) його ніхто не везе: `POST /api/v1/sync/push`
    хабa — шлях «каса → вузол», а «вузол → хаб» треба робити окремо
    (§7.3 п.1, «форвардер node→hub»);
  * стан форвардингу неможливо було ні побачити, ні повторити: журнал
    `sync_log` фіксує ФАКТ прийому (`payload_hash`), але не стан передачі
    вгору, і не має payload — переграти запис із нього не можна.

Що додається:

  1. ``sync_log.hub_forwarded_at timestamptz NULL`` /
     ``sync_log.hub_forward_status text NULL`` — стан форвардингу КОЖНОГО
     прийнятого агрегата (ADR §7.1-A1). ``hub_forwarded_at IS NULL`` = ще не
     передано вгору; ``hub_forward_status`` = ``accepted`` (хаб підтвердив) або
     ``failed`` (вичерпано спроби / незворотна відмова хабa).
     Partial-індекс ``(store_id, hub_forwarded_at) WHERE hub_forwarded_at IS
     NULL`` (дослівно §7.1-A1) — «що ще не поїхало» для моніторингу (E6).

  2. ``hub_outbox`` — ЧЕРГА передачі вгору (ADR §7.1-A1: «…або окрема
     `hub_outbox`»). Вибір саме цієї форми A1 — не свавілля: `sync_log` не має
     payload-колонки (лише ``payload_hash``), тож «взяти з журналу й переграти»
     фізично неможливо; відновлення документа з таблиць-агрегатів було б
     ДРУГОЮ реалізацією конверта каси (ризик тихої розбіжності даних на хабі).
     Тому вузол кладе в чергу ТЕ САМЕ, ЩО ПРИЙНЯВ (``envelope jsonb`` — конверт
     `PushEnvelope` як є), а ``sync_log`` лишається журналом стану (п.1).
     Поля: ``status`` (pending|done|failed), ``attempts``/``next_attempt_at``
     (backoff/MAX_ATTEMPTS — та сама політика, що в черзі каси,
     `offline/sync_push.rs`), ``batch_id`` (штамп батча вузла: хаб збереже
     ТОЙ САМИЙ — критерій E3), ``forwarded_at``/``forward_status``/``error``.
     ``UNIQUE (store_id, client_uuid, entity)`` — ідемпотентність постановки в
     чергу (повторний push того самого агрегата не плодить дублікатів).

Ідентифікація вузла (A3) НЕ додається: наявна ``network_nodes`` покриває роль
вузла-клієнта (``store_id``, ``name``, ``node_token_hash``, ``status``,
``last_seen_at``) — перевірено на БД, нової сутності не створюємо.

Idempotent-форма: ``ADD COLUMN IF NOT EXISTS`` + ``CREATE TABLE IF NOT EXISTS``
(патерн 0013/0016–0020) — безпечно на БД, де шар уже створено вручну.

Revision ID: 0021_hub_forwarding
Revises: 0020_sync_batches
Create Date: 2026-09-27
"""

from typing import Sequence, Union

from alembic import op


# revision identifiers, used by Alembic.
revision: str = "0021_hub_forwarding"
down_revision: Union[str, Sequence[str], None] = "0020_sync_batches"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Стан форвардингу в sync_log + черга hub_outbox."""
    # ── 1. Стан форвардингу в журналі прийому (ADR §7.1-A1) ────────────────
    op.execute("ALTER TABLE sync_log ADD COLUMN IF NOT EXISTS hub_forwarded_at timestamptz")
    op.execute("ALTER TABLE sync_log ADD COLUMN IF NOT EXISTS hub_forward_status text")
    op.execute(
        "CREATE INDEX IF NOT EXISTS ix_sync_log_hub_pending "
        "ON sync_log (store_id, hub_forwarded_at) WHERE hub_forwarded_at IS NULL"
    )

    # ── 2. Черга передачі вгору (payload + політика повторів) ─────────────
    op.execute(
        """
        CREATE TABLE IF NOT EXISTS hub_outbox (
            id bigserial PRIMARY KEY,
            store_id uuid NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
            entity text NOT NULL,
            client_uuid uuid NOT NULL,
            batch_id uuid,
            envelope jsonb NOT NULL,
            status text NOT NULL DEFAULT 'pending'
                CONSTRAINT hub_outbox_status_check
                CHECK (status IN ('pending','done','failed')),
            attempts int NOT NULL DEFAULT 0,
            next_attempt_at timestamptz NOT NULL DEFAULT now(),
            forwarded_at timestamptz,
            forward_status text
                CONSTRAINT hub_outbox_forward_status_check
                CHECK (forward_status IS NULL OR forward_status IN ('accepted','failed')),
            error text,
            created_at timestamptz NOT NULL DEFAULT now()
        )
        """
    )
    # Ідемпотентність постановки: повторний push того самого агрегата (або
    # бекфіл історії після ввімкнення форвардера) не плодить дублікатів.
    op.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS uq_hub_outbox_store_client_entity "
        "ON hub_outbox (store_id, client_uuid, entity)"
    )
    # FIFO-вибірка наступного батча: pending, дозволені за backoff, порядок id.
    op.execute(
        "CREATE INDEX IF NOT EXISTS ix_hub_outbox_pending "
        "ON hub_outbox (status, next_attempt_at, id)"
    )


def downgrade() -> None:
    """Прибрати чергу форвардингу і стан у журналі (спершу залежне)."""
    op.execute("DROP TABLE IF EXISTS hub_outbox")
    op.execute("DROP INDEX IF EXISTS ix_sync_log_hub_pending")
    op.execute("ALTER TABLE sync_log DROP COLUMN IF EXISTS hub_forward_status")
    op.execute("ALTER TABLE sync_log DROP COLUMN IF EXISTS hub_forwarded_at")
