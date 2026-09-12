"""ADR-0008 §7.1-B5/C3/D2/D3 (E5-частина B): ціна точки як СПІЛЬНА сутність +
локальний маркер касира + зв'язок журналу пропозицій із канонічною версією.

Рішення Творця, які ця міграція закриває (зафіксовані, не перепитуються):

  * **Б1 (2026-09-12)** — `store_product_prices` це атрибут МЕРЕЖІ, не точки:
    мережева ціна — єдина канонічна ціна для всіх точок. Шлях такий самий, як
    у `products.price` (`sync.rs:278-311`): вузол надсилає ПРОПОЗИЦІЮ → хаб
    арбітрує ЄДИНИЙ `server_version` → роздає всім вузлам. Тому таблиця
    отримує версійний тригер + рядок `sync_meta` (C3) — без цього хаб не мав
    би чим просувати дельту, а `apply_proposal` не мав би версії для відповіді;
  * **C3** — `server_version` + entity (тут) — той самий механізм, що для
    `products`/`suppliers` (Alembic 0012) і довідників частини A (0023);
  * **D3** — локальний маркер на вузлі: `users.sync_state
    CHECK(local|pending_hub|confirmed) DEFAULT 'confirmed'`. Розрізняє
    «локально створене, ще не підтверджене хабом» (offline-first, ADR §2.2)
    від канонічного. DEFAULT `confirmed` — свідомо: рядок, що ПРИЙШОВ із хаба
    (pull) або створений на самому хабі, канонічний за визначенням, і жоден
    шлях не мусить явно його позначати;
  * **D2** — `catalog_change_requests.server_version` (створена в 0022) УЖЕ
    виконує роль `applied_server_version` («моя правка прийнята як версія N») —
    див. `catalog_proposal.rs::decide_one` (UPDATE ... server_version = $2).
    Другої колонки НЕ додаємо (дубль двох джерел істини про одну величину);
    натомість фіксуємо відповідність коментарем у БД, щоб розбіжність назв
    не виглядала як діра в контракті.

Що НЕ робиться тут (свідомо):

  * окремий push-kind `store_product_price` — НЕ потрібен і шкідливий: `push`
    приймає ОПЕРАЦІЙНІ факти («це сталося — прийми»), а ціна мережі вимагає
    АРБІТРАЖУ (ADR §4.2/§10 №1: хаб присвоює ЄДИНИЙ `server_version`, дві
    правки на один рядок стають видимим конфліктом). Двері `push` = last-write
    -wins в обхід арбітражу → мовчазна втрата правки іншого вузла;
  * ретеншн `catalog_change_requests` (Б6) і бекап хаба (Б7).

Idempotent-форма (CREATE ... IF NOT EXISTS / DROP TRIGGER IF EXISTS) —
патерн 0012/0013/0021–0023: безпечно на БД, де шар уже створено вручну.

Revision ID: 0024_store_prices_sync_state
Revises: 0023_directory_pull_versions
⚠ Довжина revision id ≤ 32 символів: `alembic_version.version_num` — varchar(32),
  інакше `upgrade` падає з `StringDataRightTruncationError: value too long`
  (первісний id `0024_store_prices_users_sync_state` був 34 символи — виправлено).
Create Date: 2026-09-30
"""

from typing import Sequence, Union

from alembic import op


# revision identifiers, used by Alembic.
revision: str = "0024_store_prices_sync_state"
down_revision: Union[str, Sequence[str], None] = "0023_directory_pull_versions"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None

# C3/B5: (таблиця, sync_meta entity) — server_version + BEFORE-тригер bump.
# entity = назва самої сутності у pull (як `products`/`write_off_reasons`).
VERSION_TABLES: list[tuple[str, str]] = [
    ("store_product_prices", "store_product_prices"),
]

# §7.1-D3: локальний маркер вузла (офлайн-створене ≠ канонічне).
SYNC_STATE_TABLE = "users"
SYNC_STATE_VALUES = ("local", "pending_hub", "confirmed")
SYNC_STATE_DEFAULT = "confirmed"
SYNC_STATE_CONSTRAINT = "users_sync_state_check"

# §7.1-D2: колонка, що УЖЕ виконує роль `applied_server_version` (0022).
D2_TABLE = "catalog_change_requests"
D2_COLUMN = "server_version"


def upgrade() -> None:
    # ── C3: server_version на спільній ціні мережі ────────────────────────
    for table, _entity in VERSION_TABLES:
        op.execute(
            f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS "
            "server_version bigint NOT NULL DEFAULT 0"
        )
        # Tombstone-делеція: «перевизначення ціни знято» ≠ «рядок зник».
        op.execute(
            f"ALTER TABLE {table} ADD COLUMN IF NOT EXISTS "
            "is_deleted boolean NOT NULL DEFAULT false"
        )

    # ── C3: рядки sync_meta + тригери bump (функція створена в 0012) ──────
    for table, entity in VERSION_TABLES:
        op.execute(
            f"INSERT INTO sync_meta (entity) VALUES ('{entity}') "
            "ON CONFLICT (entity) DO NOTHING"
        )
        op.execute(f"DROP TRIGGER IF EXISTS trg_{table}_bump ON {table}")
        op.execute(
            f"CREATE TRIGGER trg_{table}_bump "
            f"BEFORE INSERT OR UPDATE OR DELETE ON {table} "
            f"FOR EACH ROW EXECUTE FUNCTION bump_sync_version('{entity}')"
        )

    # ── C3: backfill існуючих рядків (як у 0012/0023) ────────────────────
    # Наслідок свідомий: перший pull після міграції віддасть вузлам усі наявні
    # ціни мережі — саме цього вимагає рішення Б1 («єдина канонічна ціна»).
    for table, entity in VERSION_TABLES:
        op.execute(
            f"""
            UPDATE sync_meta SET version = version + (SELECT count(*) FROM {table})
            WHERE entity = '{entity}'
            """
        )
        op.execute(
            f"""
            UPDATE {table} SET server_version = sub.new_ver
            FROM (
                SELECT id,
                       (SELECT version FROM sync_meta WHERE entity = '{entity}')
                       - row_number() OVER (ORDER BY created_at, id) + 1 AS new_ver
                FROM {table}
            ) sub
            WHERE {table}.id = sub.id
            """
        )

    # ── D3: локальний маркер вузла ───────────────────────────────────────
    op.execute(
        f"ALTER TABLE {SYNC_STATE_TABLE} ADD COLUMN IF NOT EXISTS "
        f"sync_state text NOT NULL DEFAULT '{SYNC_STATE_DEFAULT}'"
    )
    allowed = ", ".join(f"'{v}'" for v in SYNC_STATE_VALUES)
    op.execute(
        f"""
        DO $$
        BEGIN
            IF NOT EXISTS (
                SELECT 1 FROM pg_constraint WHERE conname = '{SYNC_STATE_CONSTRAINT}'
            ) THEN
                ALTER TABLE {SYNC_STATE_TABLE} ADD CONSTRAINT {SYNC_STATE_CONSTRAINT}
                    CHECK (sync_state IN ({allowed}));
            END IF;
        END $$;
        """
    )

    # ── D2: зафіксувати, що наявна колонка І Є `applied_server_version` ───
    op.execute(
        f"COMMENT ON COLUMN {D2_TABLE}.{D2_COLUMN} IS "
        "'ADR-0008 §7.1-D2 applied_server_version: канонічна версія, яку хаб "
        "присвоїв прийнятій пропозиції (те саме джерело, що sync_meta.version). "
        "Окремої колонки applied_server_version немає — роль виконує ця (0022).'"
    )


def downgrade() -> None:
    """Відкат: зняти тригери/колонки/маркер і прибрати рядок sync_meta.

    Версії в `sync_meta` назад не відмотуються (як у 0012/0023): backfill не
    зворотний. Безпечно — дельта без entity у коді не запитується.
    """
    for table, entity in VERSION_TABLES:
        op.execute(f"DROP TRIGGER IF EXISTS trg_{table}_bump ON {table}")
        op.execute(f"ALTER TABLE {table} DROP COLUMN IF EXISTS is_deleted")
        op.execute(f"ALTER TABLE {table} DROP COLUMN IF EXISTS server_version")
        op.execute(f"DELETE FROM sync_meta WHERE entity = '{entity}'")
    op.execute(f"COMMENT ON COLUMN {D2_TABLE}.{D2_COLUMN} IS NULL")
    op.execute(
        f"ALTER TABLE {SYNC_STATE_TABLE} DROP CONSTRAINT IF EXISTS {SYNC_STATE_CONSTRAINT}"
    )
    op.execute(f"ALTER TABLE {SYNC_STATE_TABLE} DROP COLUMN IF EXISTS sync_state")
