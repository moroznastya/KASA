"""E9 (ADR-0008 §10 №8, блокер Б5) — протокол сумісності major-версії схеми
хаб↔вузол. РІШЕННЯ ТВОРЦЯ 2026-09-16: **варіант A** — major-версія схеми
зберігається в `schema_revision`; хаб ВІДХИЛЯЄ push від вузла з іншою major.

Що робить міграція: додає до локальної технічної таблиці `schema_revision`
(§5 рядок 29; кожна БД мігрує себе сама, DDL по мережі не їде) дві колонки —
`major` (несумісні зміни схеми) і `minor` (сумісні, адитивні).

ЧОМУ САМЕ ЦІ КОЛОНКИ, А НЕ НОВА ТАБЛИЦЯ:

  * `schema_revision` уже існує як маркер ревізії схеми (`fingerprint` усіх
    DDL-констант `ensure_schema`, Фаза 2.2) — рівно один рядок `id = 1`;
  * fingerprint відповідає на питання «чи застосовано ЦЕЙ DDL у ЦІЙ БД», а
    major/minor — «чи сумісна схема цієї БД зі схемою співрозмовника». Обидві
    відповіді — про ту саму річ (версію локальної схеми), тому друге джерело
    істини було б дефектом, а не гнучкістю.

DEFAULT'и: `major = 1`, `minor = 0` — базова версія (та сама, що константа
`torgashka_infrastructure::sync_schema::SCHEMA_MAJOR`). Обидві колонки
NOT NULL: «версія невідома» — стан, який не має права існувати (`NULL` у
порівнянні дав би тихе прийняття замість явної відмови).

Idempotent-форма (ADD COLUMN IF NOT EXISTS / DROP COLUMN IF EXISTS) — патерн
0012/0013/0021–0024: безпечно на БД, де колонки вже додав Rust-DDL
(`ensure_schema`, `SCHEMA_REVISION_DDL`), і навпаки.

⚠ Довжина revision id ≤ 32 символи (`alembic_version.version_num` — varchar(32)).

Revision ID: 0025_schema_revision_major
Revises: 0024_store_prices_sync_state
Create Date: 2026-10-02
"""

from alembic import op

# revision identifiers, used by Alembic.
revision = "0025_schema_revision_major"
down_revision = "0024_store_prices_sync_state"
branch_labels = None
depends_on = None

# Версія схеми, яку фіксує ця міграція (синхронно з
# `torgashka-infrastructure/src/sync_schema.rs: SCHEMA_MAJOR`/`SCHEMA_MINOR`).
SCHEMA_MAJOR = 1
SCHEMA_MINOR = 0


def upgrade() -> None:
    """Додати major/minor; наявний рядок маркера отримує базову версію."""
    # Таблицю створює Rust-DDL (`ensure_schema`, `SCHEMA_REVISION_DDL`), а не
    # Alembic: на БД, де Alembic виконується ПЕРШИМ (свіжа інсталяція), ALTER
    # упав би з 42P01. `IF NOT EXISTS` робить міграцію самодостатньою і
    # безпечною в обидвох порядках — форма 1:1 з Rust-DDL.
    op.execute(
        """
        CREATE TABLE IF NOT EXISTS schema_revision (
            id          integer PRIMARY KEY,
            fingerprint text NOT NULL,
            applied_at  timestamptz NOT NULL DEFAULT now()
        )
        """
    )
    op.execute("ALTER TABLE schema_revision ADD COLUMN IF NOT EXISTS major integer")
    op.execute("ALTER TABLE schema_revision ADD COLUMN IF NOT EXISTS minor integer")
    # Наявний рядок (id = 1) не має NULL-версій: ставимо базову.
    op.execute(
        f"UPDATE schema_revision SET major = {SCHEMA_MAJOR} WHERE major IS NULL"
    )
    op.execute(
        f"UPDATE schema_revision SET minor = {SCHEMA_MINOR} WHERE minor IS NULL"
    )
    # Лише після заповнення — NOT NULL + DEFAULT (щоб ADD COLUMN не падав на
    # таблиці з рядком у суворішому режимі PG).
    op.execute("ALTER TABLE schema_revision ALTER COLUMN major SET DEFAULT 1")
    op.execute("ALTER TABLE schema_revision ALTER COLUMN minor SET DEFAULT 0")
    op.execute("ALTER TABLE schema_revision ALTER COLUMN major SET NOT NULL")
    op.execute("ALTER TABLE schema_revision ALTER COLUMN minor SET NOT NULL")
    op.execute(
        "COMMENT ON COLUMN schema_revision.major IS "
        "'E9 (ADR-0008 §10 №8, варіант A): major-версія схеми; хаб відхиляє push "
        "від вузла з іншою major (error_class=VALIDATION, HTTP 409)'"
    )
    op.execute(
        "COMMENT ON COLUMN schema_revision.minor IS "
        "'E9: minor-версія схеми (адитивні, сумісні зміни); на прийом push не впливає'"
    )


def downgrade() -> None:
    """Прибрати версійні колонки (локальна таблиця — губити нічого бізнесового)."""
    op.execute("COMMENT ON COLUMN schema_revision.minor IS NULL")
    op.execute("COMMENT ON COLUMN schema_revision.major IS NULL")
    op.execute("ALTER TABLE schema_revision DROP COLUMN IF EXISTS minor")
    op.execute("ALTER TABLE schema_revision DROP COLUMN IF EXISTS major")
