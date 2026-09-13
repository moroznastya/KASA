import io
import re

BASE = "frontend/src-tauri/crates/"
DIRS = BASE + "torgashka-infrastructure/src/repositories/directories.rs"

s = io.open(DIRS, encoding="utf-8").read()
orig = s

# ─────────────────────────────────────────────────────────────────────────────
# 1. Пошук: для патернів ≥3 символів — UNION гілок (trgm-індекси), інакше OR.
# ─────────────────────────────────────────────────────────────────────────────
old_search = """        if let Some(q) = effective_query(filters) {
            let pattern = format!("%{q}%");
            qb.push(" WHERE (p.title ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR p.barcode ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR p.sku ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR EXISTS (SELECT 1 FROM barcodes b WHERE b.product_id = p.id AND b.barcode ILIKE ");
            qb.push_bind(pattern);
            qb.push("))");
            conditions += 1;
        }"""
new_search = """        if let Some(q) = effective_query(filters) {
            let pattern = format!("%{q}%");
            push_where(qb, conditions > 0);
            if search_trgm_applicable(&q) {
                // Той самий набір товарів, але кожна гілка може піти
                // Bitmap Index Scan по pg_trgm-індексах
                // (ix_products_title_trgm / ix_products_barcode_trgm).
                // Виміряно на хабі (4014 товарів, «молоко»): 9.8 мс проти
                // 74.4 мс для OR-форми, де EXISTS-підзапит змушує суцільний скан.
                qb.push(" p.id IN (SELECT id FROM products WHERE title ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" UNION SELECT id FROM products WHERE barcode ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" UNION SELECT id FROM products WHERE sku ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" UNION SELECT product_id FROM barcodes WHERE barcode ILIKE ");
                qb.push_bind(pattern);
                qb.push(")");
            } else {
                // Патерн коротший за 3 символи: триграми не застосовні —
                // класична OR-форма (один суцільний скан, без другого проходу).
                qb.push(" (p.title ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR p.barcode ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR p.sku ILIKE ");
                qb.push_bind(pattern.clone());
                qb.push(" OR EXISTS (SELECT 1 FROM barcodes b WHERE b.product_id = p.id AND b.barcode ILIKE ");
                qb.push_bind(pattern);
                qb.push("))");
            }
            conditions += 1;
        }"""
assert old_search in s
s = s.replace(old_search, new_search, 1)

# ─────────────────────────────────────────────────────────────────────────────
# 2. product_rows_query: режим total — Window або скалярний підзапит.
# ─────────────────────────────────────────────────────────────────────────────
old_rows = """    /// `with_total` додає в SELECT `count(*) OVER ()` — загальну кількість
    /// матчів без окремого скану (для сторінкового запиту).
    fn product_rows_query<'a>(
        filters: &'a ProductFilters,
        with_total: bool,
    ) -> QueryBuilder<'a, sqlx::Postgres> {"""
new_rows = """    /// Додає в SELECT `total` для сторінкового запиту.
    fn product_rows_query<'a>(
        filters: &'a ProductFilters,
        total_mode: TotalMode,
    ) -> QueryBuilder<'a, sqlx::Postgres> {"""
assert old_rows in s
s = s.replace(old_rows, new_rows, 1)

old_total = """        if with_total {
            qb.push(", count(*) OVER () AS total_count");
        }"""
new_total = """        match total_mode {
            TotalMode::Window => qb.push(", count(*) OVER () AS total_count"),
            TotalMode::ScalarSubquery => {
                // Некорельований скалярний підзапит: PostgreSQL виконує його
                // один раз (InitPlan) і не змушує сторінковий запит
                // матеріалізувати ВЕСЬ матчинг. Виміряно на хабі (4014 товарів,
                // каталог без фільтрів): 3.2 мс проти 35 мс із `count(*) OVER ()`
                // (вікно тягне всі 4014 рядків крізь скан, LIMIT не рятує).
                // Бінди підзапиту пушаться ПЕРШИМИ — у тому ж порядку, що й текст.
                qb.push(", (SELECT count(*) FROM products p LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = ");
                Self::push_store_scope(&mut qb);
                Self::push_product_filters(&mut qb, filters);
                qb.push(") AS total_count");
            }
        }"""
assert old_total in s
s = s.replace(old_total, new_total, 1)

# ─────────────────────────────────────────────────────────────────────────────
# 3. product_page_query: режим обирається за фільтрами.
# ─────────────────────────────────────────────────────────────────────────────
old_page = """    fn product_page_query<'a>(filters: &'a ProductFilters) -> QueryBuilder<'a, sqlx::Postgres> {
        let mut qb = Self::product_rows_query(filters, true);"""
new_page = """    fn product_page_query<'a>(filters: &'a ProductFilters) -> QueryBuilder<'a, sqlx::Postgres> {
        let mut qb = Self::product_rows_query(filters, total_mode(filters));"""
assert old_page in s
s = s.replace(old_page, new_page, 1)

# ─────────────────────────────────────────────────────────────────────────────
# 4. list_products: один запит на зв'язки (images + barcodes) замість двох.
# ─────────────────────────────────────────────────────────────────────────────
old_rel = """        let (images, barcodes) = if ids.is_empty() {
            (
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
            )
        } else {
            let images = self.fetch_images(&ids).await?;
            let barcodes = self.fetch_barcodes(&ids).await?;
            (images, barcodes)
        };"""
new_rel = """        let (images, barcodes) = if ids.is_empty() {
            (
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
            )
        } else {
            self.fetch_relations(&ids).await?
        };"""
assert old_rel in s
s = s.replace(old_rel, new_rel, 1)

# ─────────────────────────────────────────────────────────────────────────────
# 5. fetch_images + fetch_barcodes → один fetch_relations.
# ─────────────────────────────────────────────────────────────────────────────
start = s.index("    /// Зображення товарів сторінки (product_id → список, ORDER BY id).")
end = s.index("}\n\n// ─── Допоміжні ─")
old_block = s[start:end]
new_block = '''    /// Зв'язки товарів сторінки: зображення + додаткові штрих-коди — ОДНИМ
    /// запитом (UNION ALL), щоб зекономити мережевий круг (≈29 мс на касі).
    ///
    /// Порядок елементів усередині кожного списку — за `id` (як у Python-еталоні
    /// й у попередніх двох запитах): UNION ALL порядку між гілками не гарантує,
    /// тож сортуємо в пам'яті (списки по 20 товарах — мізерні).
    async fn fetch_relations(
        &self,
        ids: &[Uuid],
    ) -> Result<
        (
            std::collections::HashMap<Uuid, Vec<ProductImageDto>>,
            std::collections::HashMap<Uuid, Vec<BarcodeDto>>,
        ),
        DirectoryError,
    > {
        let rows = sqlx::query(
            "SELECT 'image' AS kind, id, product_id, url, is_main, sort_order,
                    NULL::text AS barcode, NULL::bool AS is_primary, created_at
               FROM product_images WHERE product_id = ANY($1)
             UNION ALL
             SELECT 'barcode', id, product_id, NULL::text, NULL::bool, NULL::int,
                    barcode, is_primary, created_at
               FROM barcodes WHERE product_id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut images: std::collections::HashMap<Uuid, Vec<ProductImageDto>> =
            std::collections::HashMap::new();
        let mut barcodes: std::collections::HashMap<Uuid, Vec<BarcodeDto>> =
            std::collections::HashMap::new();
        for r in &rows {
            let pid: Uuid = r.get("product_id");
            if r.get::<String, _>("kind") == "image" {
                images.entry(pid).or_default().push(ProductImageDto {
                    id: r.get("id"),
                    url: r.get("url"),
                    is_main: r.get("is_main"),
                    sort_order: r.get("sort_order"),
                    created_at: r.get("created_at"),
                });
            } else {
                barcodes.entry(pid).or_default().push(BarcodeDto {
                    id: r.get("id"),
                    barcode: r.get("barcode"),
                    is_primary: r.get("is_primary"),
                    created_at: r.get("created_at"),
                });
            }
        }
        for list in images.values_mut() {
            list.sort_by_key(|i| i.id);
        }
        for list in barcodes.values_mut() {
            list.sort_by_key(|b| b.id);
        }
        Ok((images, barcodes))
    }
'''
s = s[:start] + new_block + s[end:]

# ─────────────────────────────────────────────────────────────────────────────
# 6. Допоміжні: TotalMode, total_mode, search_trgm_applicable.
# ─────────────────────────────────────────────────────────────────────────────
anchor = """/// `WHERE` або ` AND ` залежно від наявності попередніх умов."""
helpers = '''/// Спосіб отримання `total` для сторінки товарів (обидва дають однакове
/// значення — різниця лише в плані запиту, виміряно на хабі).
#[derive(Clone, Copy)]
enum TotalMode {
    /// `count(*) OVER ()` — вікно поверх того самого скану.
    Window,
    /// Скалярний підзапит у SELECT — одноразовий InitPlan.
    ScalarSubquery,
}

/// Чи застосовні pg_trgm-індекси до пошукового патерну (потрібно ≥3 символи).
fn search_trgm_applicable(query: &str) -> bool {
    query.trim().chars().count() >= 3
}

/// Обирає спосіб отримання `total` за фільтрами (виміряно на хабі):
///   • пошук ≥3 символів — набір малий і добувається trgm-індексами, тож
///     другий (скалярний) прохід по ньому майже безкоштовний → 9.8 мс;
///   • пошук коротший — триграми не діють, набір шукається суцільним сканом,
///     і другий прохід коштував би ще один скан (137 мс проти 74 мс) → вікно;
///   • без пошуку — сторінка йде по індексу `ix_products_title_lower_c`,
///     а `count(*)` — дешевий InitPlan → 3.2 мс проти 35 мс із вікном.
fn total_mode(filters: &ProductFilters) -> TotalMode {
    match effective_query(filters) {
        Some(q) if search_trgm_applicable(&q) => TotalMode::ScalarSubquery,
        Some(_) => TotalMode::Window,
        None => TotalMode::ScalarSubquery,
    }
}

/// `WHERE` або ` AND ` залежно від наявності попередніх умов.'''
assert anchor in s
s = s.replace(anchor, helpers, 1)

assert s != orig
io.open(DIRS, "w", encoding="utf-8").write(s)
print("directories.rs: зміни застосовано")
print("  fetch_relations:", s.count("fetch_relations"))
print("  TotalMode:", s.count("TotalMode"))
print("  fetch_images(залишок):", s.count("fn fetch_images"))
