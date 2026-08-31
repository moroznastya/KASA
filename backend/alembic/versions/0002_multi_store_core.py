"""Мультиточковість: ядро (stores, user_stores, stock, роль owner).

Зміст:
  1. stores — торговельні точки (магазини) власника
  2. user_stores — зв'язок користувач ↔ точка (роль/права НА РІВНІ ТОЧКИ)
  3. stock — залишки per store (store_id, product_id) — каталог products
     залишається ГЛОБАЛЬНИМ, кількість/ціна виносяться в stock
  4. Роль owner в ENUM user_role
  5. Backfill: «Основна точка» + всі наявні користувачі → user_stores
  6. Копіювання products.stock/products.price → stock (колонки НЕ видаляються —
     видалення після Етапу 3, коли Rust-фасад перестане їх читати)

Revision ID: 0002_multi_store_core
Revises: 20260820_merge_heads
Create Date: 2026-08-20
"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
from sqlalchemy.dialects import postgresql
from sqlalchemy import inspect

# revision identifiers, used by Alembic.
revision: str = "0002_multi_store_core"
down_revision: Union[str, Sequence[str], None] = "0002a_add_owner_role"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    bind = op.get_bind()
    inspector = inspect(bind)

    # ══════════════════════════════════════════════
    # 1. stores — торговельні точки
    # ══════════════════════════════════════════════
    if "stores" not in inspector.get_table_names():
        op.create_table(
            "stores",
            sa.Column("id", postgresql.UUID(as_uuid=True), primary_key=True,
                      server_default=sa.text("uuid_generate_v4()")),
            sa.Column("name", sa.String(255), nullable=False, index=True),
            sa.Column("address", sa.String(500), nullable=True),
            sa.Column("phone", sa.String(50), nullable=True),
            sa.Column("is_active", sa.Boolean(), nullable=False,
                      server_default=sa.text("true")),
            sa.Column("created_at", sa.DateTime(), nullable=False,
                      server_default=sa.func.now()),
            sa.Column("updated_at", sa.DateTime(), nullable=False,
                      server_default=sa.func.now()),
        )

    # ══════════════════════════════════════════════
    # 2. user_stores — користувач ↔ точка
    # ══════════════════════════════════════════════
    if "user_stores" not in inspector.get_table_names():
        op.create_table(
            "user_stores",
            sa.Column("user_id", postgresql.UUID(as_uuid=True), nullable=False),
            sa.Column("store_id", postgresql.UUID(as_uuid=True), nullable=False),
            sa.Column("role", sa.String(16), nullable=False,
                      server_default="cashier"),
            sa.Column("permissions", postgresql.JSONB(astext_type=sa.Text()),
                      nullable=False, server_default=sa.text("'{}'::jsonb")),
            sa.Column("is_default", sa.Boolean(), nullable=False,
                      server_default=sa.text("false")),
            sa.Column("created_at", sa.DateTime(), nullable=False,
                      server_default=sa.func.now()),
            sa.PrimaryKeyConstraint("user_id", "store_id"),
            sa.ForeignKeyConstraint(["user_id"], ["users.id"],
                                    ondelete="CASCADE"),
            sa.ForeignKeyConstraint(["store_id"], ["stores.id"],
                                    ondelete="CASCADE"),
        )
        op.create_index("ix_user_stores_store", "user_stores", ["store_id"])
        op.create_index("ix_user_stores_role", "user_stores", ["role"])

    # ══════════════════════════════════════════════
    # 3. stock — залишки per store
    # ══════════════════════════════════════════════
    if "stock" not in inspector.get_table_names():
        op.create_table(
            "stock",
            sa.Column("store_id", postgresql.UUID(as_uuid=True), nullable=False),
            sa.Column("product_id", postgresql.UUID(as_uuid=True), nullable=False),
            sa.Column("quantity", sa.Numeric(10, 3), nullable=False,
                      server_default=sa.text("0.000")),
            sa.Column("price", sa.Numeric(10, 2), nullable=False,
                      server_default=sa.text("0.00")),
            sa.Column("updated_at", sa.DateTime(), nullable=False,
                      server_default=sa.func.now()),
            sa.PrimaryKeyConstraint("store_id", "product_id"),
            sa.ForeignKeyConstraint(["store_id"], ["stores.id"],
                                    ondelete="CASCADE"),
            sa.ForeignKeyConstraint(["product_id"], ["products.id"],
                                    ondelete="CASCADE"),
        )
        op.create_index("ix_stock_product", "stock", ["product_id"])

    # ══════════════════════════════════════════════
    # 4. Роль owner в ENUM user_role
    # ══════════════════════════════════════════════

    # ══════════════════════════════════════════════
    # 5. Backfill: «Основна точка» + прив'язка користувачів
    # ══════════════════════════════════════════════
    conn = bind
    try:
        # 5.1 Назва основної точки — з system_settings.shop_name, якщо є
        row = conn.execute(
            sa.text(
                "SELECT value FROM system_settings WHERE key = 'shop_name' LIMIT 1"
            )
        ).fetchone()
        shop_name = (row[0] if row and row[0] else "Основна точка").strip() or "Основна точка"

        # 5.2 Створити основну точку (якщо stores ще порожня)
        store_count = conn.execute(
            sa.text("SELECT count(*) FROM stores")
        ).scalar()
        if store_count == 0:
            conn.execute(
                sa.text(
                    "INSERT INTO stores (id, name, address, phone, is_active) "
                    "VALUES (uuid_generate_v4(), :name, NULL, NULL, true)"
                ),
                {"name": shop_name},
            )

        # 5.3 ID основної точки
        main_store_id = conn.execute(
            sa.text("SELECT id FROM stores ORDER BY created_at LIMIT 1")
        ).scalar()

        # 5.4 Усі наявні користувачі: admin → owner; прив'язка до основної точки
        users = conn.execute(
            sa.text("SELECT id, role FROM users WHERE is_active = true")
        ).fetchall()
        for user_id, role in users:
            # пермішени за замовчуванням: owner — все, інші — пусто (рольові правила)
            perms = '{"*": true}' if role in ("admin", "owner") else "{}"
            # чи вже є зв'язок
            exists = conn.execute(
                sa.text(
                    "SELECT 1 FROM user_stores WHERE user_id = :uid AND store_id = :sid"
                ),
                {"uid": user_id, "sid": main_store_id},
            ).fetchone()
            if not exists:
                conn.execute(
                    sa.text(
                        "INSERT INTO user_stores (user_id, store_id, role, permissions, is_default) "
                        "VALUES (:uid, :sid, :role, CAST(:perms AS jsonb), true)"
                    ),
                    {"uid": user_id, "sid": main_store_id,
                     "role": role, "perms": perms},
                )
            # admin → owner (зберігаємо сумісність: власник має всі права)
            if role == "admin":
                conn.execute(
                    sa.text("UPDATE users SET role = 'owner' WHERE id = :uid"),
                    {"uid": user_id},
                )
        # alembic комітить сам (transaction_per_migration=True)
    finally:
        pass

    # ══════════════════════════════════════════════
    # 6. Копіювання products.stock/products.price → stock
    #    (колонки НЕ видаляються — legacy-сумісність до Етапу 3)
    # ══════════════════════════════════════════════
    conn = bind
    try:
        main_store_id = conn.execute(
            sa.text("SELECT id FROM stores ORDER BY created_at LIMIT 1")
        ).scalar()
        conn.execute(
            sa.text(
                "INSERT INTO stock (store_id, product_id, quantity, price) "
                "SELECT :sid, id, COALESCE(stock, 0), COALESCE(price, 0) "
                "FROM products "
                "ON CONFLICT (store_id, product_id) DO NOTHING"
            ),
            {"sid": main_store_id},
        )
        # alembic комітить сам (transaction_per_migration=True)
    finally:
        pass


def downgrade() -> None:
    """Відкат: видалити stock, user_stores, stores."""
    op.drop_table("stock")
    op.drop_table("user_stores")
    op.drop_table("stores")
    # ENUM: PG не дозволяє легко видалити значення — залишаємо 'owner' у типі
    # (зворотна сумісність не порушується)
