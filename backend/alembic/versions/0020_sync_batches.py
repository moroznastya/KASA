"""ADR-0008 §7.1-A2 + §7.1-E1 (етап E2a/E2b): батчі push — ідентичність пакета
агрегатів (`sync_batches` + `sync_log.batch_id`) і машинний клас помилки
агрегата (`sync_log.error_class`).

Проблема, яку закриває міграція (ADR-0008 §4.3, «між агрегатами — НЕ вирішено»):

  * прийом N агрегатів одним запитом не мав єдиної ідентичності: частковий
    збій (документ ліг, рядки — ні; або пакет упав посеред обробки) був
    невидимий у журналі — «пакет у процесі» неможливо відрізнити від
    «загублено»;
  * незворотний `failed` застосовувався до БУДЬ-ЯКОЇ помилки агрегата, тому
    відмова через порядок (дитина приїхала раніше батька — FK) ставала
    «потребує ручного втручання» назавжди, хоча повтор після прибуття батька
    був би успішним.

Що додається:

  1. ``sync_batches(id uuid PK, store_id uuid, node_id uuid, created_at
     timestamptz, items int, status text CHECK(accepted|partial|failed))`` —
     рядок пакета: ``status`` виставляється за ФАКТИЧНИМ результатом обробки
     (усі прийняті → ``accepted``; частина → ``partial``; жодного → ``failed``).
     Рядок створюється ДО обробки з песимістичним ``failed`` і оновлюється
     після неї: пакет, що впав посеред роботи, лишається видимим як невзятий,
     а ``sync_log.batch_id`` завжди має свого батька.
     ``node_id`` — ідентичність ВУЗЛА (device-режим, ``claims.sub``); для
     JWT-каси/адміна це користувач, не вузол → NULL (без вигадування вузла
     з user_id).
     Індекс ``(store_id, created_at DESC)`` — як у ``sync_log`` (ADR §7.1-A2).

  2. ``sync_log.batch_id uuid NULL`` — спільний ідентифікатор для ВСІХ
     агрегатів запиту (і прийнятих, і відкинутих): клієнт/вузол штампує батч
     заголовком ``X-Sync-Batch-Id``, хаб зберігає той самий id (критерій E3:
     «на хабі sync_log має запис із тим самим batch_id»). NULL — сумісність із
     записами drain-у черги після promote (обробка поза батчем).

  3. ``sync_log.error_class text NULL`` — машинний клас помилки агрегата
     (§7.1-E1): ``RETRYABLE_FK`` (SQLSTATE 23503 — батько ще не прийнятий;
     клієнт робить defer, НЕ failed), ``VALIDATION`` (незворотно),
     ``CONFLICT`` (SQLSTATE 23505 — черга конфліктів, етап E5).

CHECK на ``sync_log.status`` НЕ змінюється (``ok|error|already_exists``):
класифікується не статус прийому, а ПРИЧИНА, окремим полем — тому recreate
таблиці (як вимагала б зміна CHECK) не потрібен.

Idempotent-форма: ``ADD COLUMN IF NOT EXISTS`` + ``CREATE TABLE IF NOT EXISTS``
(патерн 0013/0016–0019) — безпечно на БД, де шар уже створено вручну.

Revision ID: 0020_sync_batches
Revises: 0019_peer_parents_push_kinds
Create Date: 2026-09-20
"""

from typing import Sequence, Union

from alembic import op


# revision identifiers, used by Alembic.
revision: str = "0020_sync_batches"
down_revision: Union[str, Sequence[str], None] = "0019_peer_parents_push_kinds"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Батчі push: sync_batches + sync_log.batch_id/error_class."""
    op.execute("ALTER TABLE sync_log ADD COLUMN IF NOT EXISTS batch_id uuid")
    op.execute("ALTER TABLE sync_log ADD COLUMN IF NOT EXISTS error_class text")
    op.execute(
        """
        CREATE TABLE IF NOT EXISTS sync_batches (
            id uuid PRIMARY KEY,
            store_id uuid NOT NULL,
            node_id uuid,
            created_at timestamptz NOT NULL DEFAULT now(),
            items int NOT NULL,
            status text NOT NULL
                CONSTRAINT sync_batches_status_check
                CHECK (status IN ('accepted','partial','failed'))
        )
        """
    )
    op.execute(
        "CREATE INDEX IF NOT EXISTS ix_sync_batches_store "
        "ON sync_batches (store_id, created_at DESC)"
    )
    # Журнал шукає агрегати батча (аудит/розбір часткових збоїв).
    op.execute(
        "CREATE INDEX IF NOT EXISTS ix_sync_log_batch "
        "ON sync_log (batch_id) WHERE batch_id IS NOT NULL"
    )


def downgrade() -> None:
    """Прибрати батчі push (спершу індекси, потім колонки й таблицю)."""
    op.execute("DROP INDEX IF EXISTS ix_sync_log_batch")
    op.execute("DROP TABLE IF EXISTS sync_batches")
    op.execute("ALTER TABLE sync_log DROP COLUMN IF EXISTS error_class")
    op.execute("ALTER TABLE sync_log DROP COLUMN IF EXISTS batch_id")
