"""ADR-0008 §4.2 + §7.1-D1/D2 (етап E5): арбітраж СПІЛЬНИХ довідників на хабі —
`catalog_change_requests` (журнал ПРОПОЗИЦІЙ вузлів до авторитета).

Проблема, яку закриває міграція:

  * спільні довідники (`products`, `suppliers` — без `store_id`) у моделі
    ADR-0008 пишуться вузлом ЛОКАЛЬНО (offline-first), але канонічну версію
    (`server_version`) може присвоїти лише ХАБ (єдине джерело істини, §4.2
    «hub-as-authority»). До цієї міграції вузол або не міг писати каталог
    зовсім, або його правка лишалась локальною назавжди;
  * конфлікт двох правок на один рядок не мав де бути ВИДИМИМ: last-write-wins
    за локальним часом вузла ADR прямо відкидає («час, мовчазна втрата правки»).

Що додається (`catalog_change_requests`, ADR §7.1-D1/D2):

  * ``entity``/``row_id``/``op`` (upsert|delete) — предмет пропозиції;
  * ``payload jsonb`` — конверт зміни ТІЄЇ САМОЇ ФОРМИ, що віддає pull-дельта
    (`sync.rs::query_products`), щоб вузол будував пропозицію зі свого рядка
    без окремого мапінгу;
  * ``client_uuid uuid UNIQUE`` — ідемпотентність пропозиції: повторна
    доставка того самого запиту повертає РАНІШЕ рішення хабa, а не створює
    другу правку;
  * ``status`` (pending|accepted|conflict|rejected) + ``decided_at`` — стан
    вирішення; ``server_version bigint NULL`` — ЄДИНА канонічна версія, яку
    присвоїв хаб (наявним BEFORE-тригером ``bump_sync_version``, Alembic 0012;
    той самий механізм, що ``products.server_version``) — D2, зв'язок із
    ``sync_meta.version`` для роздачі вузлам наявним pull;
  * ``base_version`` + ``priority`` — ключі детермінованого правила ADR §4.2
    п.5: ``higher server_version / явний пріоритет >> час створення``;
  * ``error`` — причина для ``rejected`` (як ``sync_log.error``): жодна
    відмова не зникає без пояснення;
  * ``decided_by`` — хто вирішив конфлікт (оператор, майбутній E5-крок).

Індекси: ``(status, created_at)`` (черга оператора),
``(entity, row_id, status)`` (пошук конкурентів на той самий рядок).

ОБСЯГ: міграція створює ЛИШЕ журнал пропозицій. Класифікація
``store_product_prices`` (блокер Б1) і політика ``users`` (блокер Б2) НЕ
зачіпаються: перелік довідників під арбітражем живе в коді
(`catalog_proposal::ARBITRATED_ENTITIES`) і розширюється окремо, після рішення
Творця.

Idempotent-форма: ``CREATE TABLE IF NOT EXISTS`` + ``CREATE INDEX IF NOT
EXISTS`` (патерн 0013/0016–0021) — безпечно на БД, де шар уже створено вручну.

Revision ID: 0022_catalog_change_requests
Revises: 0021_hub_forwarding
Create Date: 2026-09-28
"""

from typing import Sequence, Union

from alembic import op


# revision identifiers, used by Alembic.
revision: str = "0022_catalog_change_requests"
down_revision: Union[str, Sequence[str], None] = "0021_hub_forwarding"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    """Журнал пропозицій спільних довідників + індекси арбітражу."""
    op.execute(
        """
        CREATE TABLE IF NOT EXISTS catalog_change_requests (
            id bigserial PRIMARY KEY,
            entity text NOT NULL,
            row_id uuid NOT NULL,
            op text NOT NULL
                CONSTRAINT catalog_change_requests_op_check
                CHECK (op IN ('upsert','delete')),
            payload jsonb NOT NULL DEFAULT '{}'::jsonb,
            client_uuid uuid NOT NULL,
            store_id uuid REFERENCES stores(id) ON DELETE SET NULL,
            status text NOT NULL DEFAULT 'pending'
                CONSTRAINT catalog_change_requests_status_check
                CHECK (status IN ('pending','accepted','conflict','rejected')),
            server_version bigint,
            base_version bigint NOT NULL DEFAULT 0,
            priority integer,
            error text,
            decided_by uuid,
            created_at timestamptz NOT NULL DEFAULT now(),
            decided_at timestamptz
        )
        """
    )
    # Ідемпотентність пропозиції: повтор мережевого запиту → раніше рішення.
    op.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS uq_catalog_change_requests_client_uuid "
        "ON catalog_change_requests (client_uuid)"
    )
    # Черга оператора: найстаріші невирішені — першими.
    op.execute(
        "CREATE INDEX IF NOT EXISTS ix_catalog_change_requests_queue "
        "ON catalog_change_requests (status, created_at)"
    )
    # Пошук конкурентів на той самий рядок (правило ADR §4.2 п.5).
    op.execute(
        "CREATE INDEX IF NOT EXISTS ix_catalog_change_requests_row "
        "ON catalog_change_requests (entity, row_id, status)"
    )


def downgrade() -> None:
    """Прибрати журнал пропозицій (індекси падають разом із таблицею)."""
    op.execute("DROP TABLE IF EXISTS catalog_change_requests")
