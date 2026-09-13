//! Репозиторії довідників (етап 1 — read, sqlx/PostgreSQL).
//!
//! Відтворюють логіку Python-еталону ТОЧНО:
//! - `list_products` — `ProductService.search_products`:
//!   ILIKE по title/barcode/sku/додаткових barcodes (LEFT JOIN + DISTINCT),
//!   фільтри (barcode exact, category, supplier, price, is_weight),
//!   сортування в пам'яті (relevance або title.lower()), пагінація в пам'яті.
//! - `list_categories` / `list_suppliers` — `ORDER BY name` + LIMIT/OFFSET,
//!   balance через підзапит SUM(supplier_ledger.amount).

use crate::store_ctx::StorePool;
use chrono::NaiveDateTime;
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;

use rust_decimal::Decimal as RDecimal;
use std::str::FromStr;
use torgashka_domain::{
    BarcodeDto, CategoryDto, DirectoryError, Page, ProductDto, ProductFilters, ProductImageDto,
    ReadDirectories, SupplierDto, SupplierProductItem, SupplierProductMovement,
    SupplierProductMovementsResponse, SupplierProductsResponse,
};

/// sqlx-реалізація [`ReadDirectories`] (тільки читання).
#[derive(Clone)]
pub struct SqlxDirectories {
    pool: StorePool,
}

impl SqlxDirectories {
    pub fn new(pool: StorePool) -> Self {
        Self { pool }
    }
}

// ─── Проміжні рядки запитів ────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ProductRow {
    id: Uuid,
    barcode: Option<String>,
    sku: Option<String>,
    title: String,
    description: Option<String>,
    price: Option<String>,
    cost_price: Option<String>,
    markup: Option<String>,
    stock: Option<String>,
    recommended_qty: Option<String>,
    uktzed: Option<String>,
    scan_excise: bool,
    tax_rate: Option<String>,
    tax_group: Option<String>,
    is_weight: bool,
    unit: Option<String>,
    category_id: Option<Uuid>,
    supplier_id: Option<Uuid>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

// ─── Реалізація ReadDirectories ────────────────────────────────────────────

#[async_trait::async_trait]

impl ReadDirectories for SqlxDirectories {
    async fn list_products(
        &self,
        filters: &ProductFilters,
    ) -> Result<Page<ProductDto>, DirectoryError> {
        // Сортування й сторінка виконуються в PostgreSQL: у пам'ять потрапляє
        // лише сторінка (20 рядків), а не весь матчинг. Раніше кожен запит
        // матеріалізував УВЕСЬ каталог (4409 товарів): пошук «ов» — 0.6 с,
        // а запит без тексту — 1.2 с (виміряно на касі).
        //   • з пошуковим текстом — наявність → релевантність → назва;
        //   • без тексту — алфавіт за назвою (незмінна семантика каталогу).
        let (rows, window_total) = self.fetch_product_page(filters).await?;
        let total = match window_total {
            Some(t) => t,
            // Порожня сторінка (offset за межами) — total питаємо окремо.
            None => self.count_product_rows(filters).await?,
        };
        let page_rows = rows;

        let pages = total_pages(total, filters.size);

        // Зв'язки (images, barcodes) — для товарів поточної сторінки.
        let ids: Vec<Uuid> = page_rows.iter().map(|r| r.id).collect();
        let (images, barcodes) = if ids.is_empty() {
            (
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
            )
        } else {
            self.fetch_relations(&ids).await?
        };

        let items = page_rows
            .into_iter()
            .map(|r| ProductDto {
                id: r.id,
                barcode: r.barcode,
                sku: r.sku,
                title: r.title,
                description: r.description,
                price: r.price,
                cost_price: r.cost_price,
                markup: r.markup,
                stock: r.stock,
                recommended_qty: r.recommended_qty,
                uktzed: r.uktzed,
                scan_excise: r.scan_excise,
                tax_rate: r.tax_rate,
                tax_group: r.tax_group,
                is_weight: r.is_weight,
                unit: r.unit,
                category_id: r.category_id,
                supplier_id: r.supplier_id,
                images: images.get(&r.id).cloned().unwrap_or_default(),
                barcodes: barcodes.get(&r.id).cloned().unwrap_or_default(),
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect();

        Ok(Page {
            items,
            total,
            page: filters.page,
            page_size: filters.size,
            pages,
        })
    }

    async fn list_categories(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<CategoryDto>, DirectoryError> {
        let offset = ((page - 1) * size).max(0);
        // Один запит: сторінка + загальна кількість (`count(*) OVER ()`) —
        // замість count(*)+LIMIT/OFFSET, тобто -1 мережевий круг на виклик.
        let rows = sqlx::query(
            "SELECT id, name, description, parent_id, created_at, updated_at,
                    count(*) OVER () AS total_count
             FROM categories
             ORDER BY name
             LIMIT $1 OFFSET $2",
        )
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        // Порожня сторінка (offset за межами) — total питаємо окремо.
        let total: i64 = match rows.first() {
            Some(r) => r.get("total_count"),
            None => sqlx::query_scalar("SELECT count(*) FROM categories")
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?,
        };

        let items = rows
            .iter()
            .map(|r| CategoryDto {
                id: r.get("id"),
                name: r.get("name"),
                description: r.get("description"),
                parent_id: r.get("parent_id"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect();

        Ok(Page {
            items,
            total,
            page,
            page_size: size,
            pages: total_pages(total, size),
        })
    }

    async fn search_categories(
        &self,
        page: i64,
        size: i64,
        search: Option<&str>,
    ) -> Result<Page<CategoryDto>, DirectoryError> {
        // Динамічний WHERE (як Python: `if query: stmt = stmt.where(...)`).
        let like = search.map(|q| format!("%{q}%"));
        let (total_sql, rows_sql) = match &like {
            Some(_) => (
                "SELECT count(*) FROM categories WHERE name ILIKE $1",
                "SELECT id, name, description, parent_id, created_at, updated_at
                 FROM categories
                 WHERE name ILIKE $3
                 ORDER BY name
                 LIMIT $1 OFFSET $2",
            ),
            None => (
                "SELECT count(*) FROM categories",
                "SELECT id, name, description, parent_id, created_at, updated_at
                 FROM categories
                 ORDER BY name
                 LIMIT $1 OFFSET $2",
            ),
        };
        let mut q_total = sqlx::query_scalar(total_sql);
        if let Some(l) = &like {
            q_total = q_total.bind(l);
        }
        let total: i64 = q_total.fetch_one(&self.pool).await.map_err(db_err)?;

        let offset = ((page - 1) * size).max(0);
        let mut q_rows = sqlx::query(rows_sql).bind(size).bind(offset);
        if let Some(l) = &like {
            q_rows = q_rows.bind(l);
        }
        let rows = q_rows.fetch_all(&self.pool).await.map_err(db_err)?;

        let items = rows
            .iter()
            .map(|r| CategoryDto {
                id: r.get("id"),
                name: r.get("name"),
                description: r.get("description"),
                parent_id: r.get("parent_id"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect();

        Ok(Page {
            items,
            total,
            page,
            page_size: size,
            pages: total_pages(total, size),
        })
    }

    async fn find_all_categories(&self) -> Result<Vec<CategoryDto>, DirectoryError> {
        let rows = sqlx::query(
            "SELECT id, name, description, parent_id, created_at, updated_at
             FROM categories
             ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        Ok(rows
            .iter()
            .map(|r| CategoryDto {
                id: r.get("id"),
                name: r.get("name"),
                description: r.get("description"),
                parent_id: r.get("parent_id"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    async fn list_suppliers(
        &self,
        page: i64,
        size: i64,
    ) -> Result<Page<SupplierDto>, DirectoryError> {
        let offset = ((page - 1) * size).max(0);
        // Один запит: сторінка + загальна кількість (`count(*) OVER ()`).
        let rows = sqlx::query(
            "SELECT s.id, s.name, s.edrpou, s.phone, s.email, s.address, s.notes,
                    COALESCE((SELECT SUM(amount) FROM supplier_ledger sl
                              WHERE sl.supplier_id = s.id), 0)::numeric(12,2)::text AS current_balance,
                    s.created_at, s.updated_at, count(*) OVER () AS total_count
             FROM suppliers s
             ORDER BY s.name
             LIMIT $1 OFFSET $2",
        )
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        // Порожня сторінка (offset за межами) — total питаємо окремо.
        let total: i64 = match rows.first() {
            Some(r) => r.get("total_count"),
            None => sqlx::query_scalar("SELECT count(*) FROM suppliers")
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?,
        };

        let items = rows
            .iter()
            .map(|r| SupplierDto {
                id: r.get("id"),
                name: r.get("name"),
                edrpou: r.get("edrpou"),
                phone: r.get("phone"),
                email: r.get("email"),
                address: r.get("address"),
                notes: r.get("notes"),
                current_balance: r.get("current_balance"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect();

        Ok(Page {
            items,
            total,
            page,
            page_size: size,
            pages: total_pages(total, size),
        })
    }

    // ─── Етап 2: читання за ID (CRUD) ──────────────────────────────────────
    async fn get_product(&self, id: Uuid) -> Result<ProductDto, DirectoryError> {
        let row = sqlx::query(
            "SELECT DISTINCT p.id, p.barcode, p.sku, p.title, p.description,
                    COALESCE(NULLIF(st.price, 0), p.price)::text AS price, p.cost_price::text,
                    p.markup::text, COALESCE(st.quantity, p.stock)::text AS stock,
                    p.recommended_qty::text, p.uktzed, p.scan_excise, p.tax_rate::text,
                    p.tax_group, p.is_weight, p.unit, p.category_id, p.supplier_id,
                    p.created_at, p.updated_at
             FROM products p
             LEFT JOIN stock st ON st.product_id = p.id
                 AND st.store_id = NULLIF(current_setting('app.store_id', true), '')::uuid
             WHERE p.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let Some(r) = row else {
            return Err(DirectoryError::NotFound(format!(
                "Товар з ID '{id}' не знайдено"
            )));
        };
        let prod = ProductRow {
            id: r.get("id"),
            barcode: r.get("barcode"),
            sku: r.get("sku"),
            title: r.get("title"),
            description: r.get("description"),
            price: r.get("price"),
            cost_price: r.get("cost_price"),
            markup: r.get("markup"),
            stock: r.get("stock"),
            recommended_qty: r.get("recommended_qty"),
            uktzed: r.get("uktzed"),
            scan_excise: r.get("scan_excise"),
            tax_rate: r.get("tax_rate"),
            tax_group: r.get("tax_group"),
            is_weight: r.get("is_weight"),
            unit: r.get("unit"),
            category_id: r.get("category_id"),
            supplier_id: r.get("supplier_id"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        };
        let ids = vec![prod.id];
        let (images, barcodes) = self.fetch_relations(&ids).await?;
        Ok(ProductDto {
            id: prod.id,
            barcode: prod.barcode,
            sku: prod.sku,
            title: prod.title,
            description: prod.description,
            price: prod.price,
            cost_price: prod.cost_price,
            markup: prod.markup,
            stock: prod.stock,
            recommended_qty: prod.recommended_qty,
            uktzed: prod.uktzed,
            scan_excise: prod.scan_excise,
            tax_rate: prod.tax_rate,
            tax_group: prod.tax_group,
            is_weight: prod.is_weight,
            unit: prod.unit,
            category_id: prod.category_id,
            supplier_id: prod.supplier_id,
            images: images.get(&id).cloned().unwrap_or_default(),
            barcodes: barcodes.get(&id).cloned().unwrap_or_default(),
            created_at: prod.created_at,
            updated_at: prod.updated_at,
        })
    }

    async fn get_product_by_barcode(&self, barcode: &str) -> Result<ProductDto, DirectoryError> {
        // Спочатку основний штрих-код (products.barcode).
        let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM products WHERE barcode = $1")
            .bind(barcode)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        let product_id = match id {
            Some(v) => v,
            None => {
                let pid: Option<Uuid> =
                    sqlx::query_scalar("SELECT product_id FROM barcodes WHERE barcode = $1")
                        .bind(barcode)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(db_err)?;
                match pid {
                    Some(v) => v,
                    None => {
                        return Err(DirectoryError::NotFound(format!(
                            "Товар зі штрих-кодом '{barcode}' не знайдено"
                        )));
                    }
                }
            }
        };
        self.get_product(product_id).await
    }

    async fn get_category(&self, id: Uuid) -> Result<CategoryDto, DirectoryError> {
        let row = sqlx::query(
            "SELECT id, name, description, parent_id, created_at, updated_at
             FROM categories WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let Some(r) = row else {
            return Err(DirectoryError::NotFound(format!(
                "Категорію з ID '{id}' не знайдено"
            )));
        };
        Ok(CategoryDto {
            id: r.get("id"),
            name: r.get("name"),
            description: r.get("description"),
            parent_id: r.get("parent_id"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        })
    }

    async fn get_supplier(&self, id: Uuid) -> Result<SupplierDto, DirectoryError> {
        let row = sqlx::query(
            "SELECT s.id, s.name, s.edrpou, s.phone, s.email, s.address, s.notes,
                    COALESCE((SELECT SUM(amount) FROM supplier_ledger sl
                              WHERE sl.supplier_id = s.id), 0)::numeric(12,2)::text AS current_balance,
                    s.created_at, s.updated_at
             FROM suppliers s WHERE s.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let Some(r) = row else {
            return Err(DirectoryError::NotFound(format!(
                "Постачальника з ID '{id}' не знайдено"
            )));
        };
        Ok(SupplierDto {
            id: r.get("id"),
            name: r.get("name"),
            edrpou: r.get("edrpou"),
            phone: r.get("phone"),
            email: r.get("email"),
            address: r.get("address"),
            notes: r.get("notes"),
            current_balance: r.get("current_balance"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        })
    }

    async fn list_all_suppliers(&self) -> Result<Vec<SupplierDto>, DirectoryError> {
        let rows = sqlx::query(
            "SELECT s.id, s.name, s.edrpou, s.phone, s.email, s.address, s.notes,
                    COALESCE((SELECT SUM(amount) FROM supplier_ledger sl
                              WHERE sl.supplier_id = s.id), 0)::numeric(12,2)::text AS current_balance,
                    s.created_at, s.updated_at
             FROM suppliers s ORDER BY s.name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(rows
            .iter()
            .map(|r| SupplierDto {
                id: r.get("id"),
                name: r.get("name"),
                edrpou: r.get("edrpou"),
                phone: r.get("phone"),
                email: r.get("email"),
                address: r.get("address"),
                notes: r.get("notes"),
                current_balance: r.get("current_balance"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    // ─── Дезактивація Python (CRIT): товари постачальника та рух ──────────

    async fn supplier_products(
        &self,
        supplier_id: Uuid,
        search: Option<&str>,
    ) -> Result<SupplierProductsResponse, DirectoryError> {
        // 1. Постачальник (404 з текстом Python-еталону).
        let sup = sqlx::query("SELECT id, name FROM suppliers WHERE id = $1")
            .bind(supplier_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        let Some(sr) = sup else {
            return Err(DirectoryError::NotFound(format!(
                "Постачальника з ID '{supplier_id}' не знайдено"
            )));
        };
        let supplier_name: String = sr.get("name");

        // 2. IDs товарів: UNION трьох джерел (як Python union()).
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT ii.product_id FROM invoice_items ii
             JOIN invoices i ON i.id = ii.invoice_id
             WHERE i.supplier_id = $1 AND i.status = 'confirmed'
             UNION
             SELECT rii.product_id FROM return_invoice_items rii
             JOIN return_invoices ri ON ri.id = rii.return_invoice_id
             WHERE ri.supplier_id = $1 AND ri.status = 'confirmed'
             UNION
             SELECT id FROM products WHERE supplier_id = $1",
        )
        .bind(supplier_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        if ids.is_empty() {
            return Ok(SupplierProductsResponse {
                supplier_id,
                supplier_name,
                total_products: 0,
                total_stock_value: "0.00".to_string(),
                products: Vec::new(),
            });
        }

        // 3. Товари (search ILIKE по title/barcode/sku + ORDER BY title).
        let mut qb = QueryBuilder::new(
            "SELECT p.id, p.barcode, p.sku, p.title,
                    COALESCE(NULLIF(st.price, 0), p.price)::text AS price,
                    p.cost_price::text, COALESCE(st.quantity, p.stock)::text AS stock,
                    p.unit, c.name AS category_name
             FROM products p
             LEFT JOIN categories c ON c.id = p.category_id
             LEFT JOIN stock st ON st.product_id = p.id
                 AND st.store_id = NULLIF(current_setting('app.store_id', true), '')::uuid
             WHERE p.id = ANY(",
        );
        qb.push_bind(&ids);
        qb.push(")");
        if let Some(q) = search {
            let pattern = format!("%{q}%");
            qb.push(" AND (p.title ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR p.barcode ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR p.sku ILIKE ");
            qb.push_bind(pattern);
            qb.push(")");
        }
        qb.push(" ORDER BY p.title");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;

        // 4. total_stock_value: Decimal-множення як Python (scale сумується).
        let mut total = RDecimal::ZERO;
        let mut products = Vec::with_capacity(rows.len());
        for r in &rows {
            let stock = r
                .get::<Option<String>, _>("stock")
                .and_then(|s| RDecimal::from_str(&s).ok())
                .unwrap_or_default();
            let cost = r
                .get::<Option<String>, _>("cost_price")
                .and_then(|s| RDecimal::from_str(&s).ok())
                .unwrap_or_default();
            total += stock * cost;
            products.push(SupplierProductItem {
                id: r.get("id"),
                barcode: r.get("barcode"),
                sku: r.get("sku"),
                title: r.get("title"),
                price: r.get("price"),
                cost_price: r.get("cost_price"),
                stock: r.get("stock"),
                unit: r.get("unit"),
                category_name: r.get("category_name"),
            });
        }

        Ok(SupplierProductsResponse {
            supplier_id,
            supplier_name,
            total_products: products.len() as i64,
            total_stock_value: total.to_string(),
            products,
        })
    }

    async fn product_movements(
        &self,
        supplier_id: Uuid,
        product_id: Uuid,
        limit: i64,
    ) -> Result<SupplierProductMovementsResponse, DirectoryError> {
        // 1. Постачальник (404).
        let sup = sqlx::query("SELECT id, name FROM suppliers WHERE id = $1")
            .bind(supplier_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        if sup.is_none() {
            return Err(DirectoryError::NotFound(format!(
                "Постачальника з ID '{supplier_id}' не знайдено"
            )));
        }

        // 2. Товар (404) з категорією.
        let prod = sqlx::query(
            "SELECT p.id, p.barcode, p.sku, p.title,
                    COALESCE(NULLIF(st.price, 0), p.price)::text AS price,
                    p.cost_price::text, COALESCE(st.quantity, p.stock)::text AS stock,
                    p.unit, c.name AS category_name
             FROM products p
             LEFT JOIN categories c ON c.id = p.category_id
             LEFT JOIN stock st ON st.product_id = p.id
                 AND st.store_id = NULLIF(current_setting('app.store_id', true), '')::uuid
             WHERE p.id = $1",
        )
        .bind(product_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let Some(pr) = prod else {
            return Err(DirectoryError::NotFound(format!(
                "Товар з ID '{product_id}' не знайдено"
            )));
        };
        let product = SupplierProductItem {
            id: pr.get("id"),
            barcode: pr.get("barcode"),
            sku: pr.get("sku"),
            title: pr.get("title"),
            price: pr.get("price"),
            cost_price: pr.get("cost_price"),
            stock: pr.get("stock"),
            unit: pr.get("unit"),
            category_name: pr.get("category_name"),
        };

        let mut movements: Vec<SupplierProductMovement> = Vec::new();

        // 3.1 Прибуткові накладні (прихід) — тільки цього постачальника, CONFIRMED.
        let rows = sqlx::query(
            "SELECT ii.id, i.invoice_date AS d, i.number AS n, i.id AS doc_id,
                    ii.quantity::text AS qty, ii.price::text AS price, ii.total::text AS total
             FROM invoice_items ii
             JOIN invoices i ON i.id = ii.invoice_id
             WHERE ii.product_id = $1 AND i.supplier_id = $2 AND i.status = 'confirmed'
             ORDER BY i.invoice_date DESC
             LIMIT $3",
        )
        .bind(product_id)
        .bind(supplier_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        for r in &rows {
            let number: String = r.get("n");
            movements.push(SupplierProductMovement {
                id: r.get("id"),
                date: r.get("d"),
                document_type: "invoice".to_string(),
                document_number: number.clone(),
                document_id: r.get("doc_id"),
                quantity: r.get("qty"),
                price: r.get("price"),
                total: r.get("total"),
                notes: Some(format!("Прибуткова накладна: {number}")),
            });
        }

        // 3.2 Повернення постачальнику (витрата) — CONFIRMED, знак мінус у SQL.
        let rows = sqlx::query(
            "SELECT rii.id, ri.return_date AS d, ri.number AS n, ri.id AS doc_id,
                    (-(rii.quantity))::text AS qty, rii.price::text AS price,
                    (-(rii.total))::text AS total
             FROM return_invoice_items rii
             JOIN return_invoices ri ON ri.id = rii.return_invoice_id
             WHERE rii.product_id = $1 AND ri.supplier_id = $2 AND ri.status = 'confirmed'
             ORDER BY ri.return_date DESC
             LIMIT $3",
        )
        .bind(product_id)
        .bind(supplier_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        for r in &rows {
            let number: String = r.get("n");
            movements.push(SupplierProductMovement {
                id: r.get("id"),
                date: r.get("d"),
                document_type: "return_invoice".to_string(),
                document_number: number.clone(),
                document_id: r.get("doc_id"),
                quantity: r.get("qty"),
                price: r.get("price"),
                total: r.get("total"),
                notes: Some(format!("Повернення постачальнику: {number}")),
            });
        }

        // 3.3 Чеки (продаж — витрата). БЕЗ фільтру по постачальнику (як Python).
        let rows = sqlx::query(
            "SELECT ri.id, r.created_at AS d, r.receipt_number AS n, r.id AS doc_id,
                    (-(ri.quantity))::text AS qty, ri.price::text AS price,
                    (-(ri.total))::text AS total
             FROM receipt_items ri
             JOIN receipts r ON r.id = ri.receipt_id
             WHERE ri.product_id = $1 AND r.is_return = false
             ORDER BY r.created_at DESC
             LIMIT $2",
        )
        .bind(product_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        for r in &rows {
            let number: String = r.get("n");
            movements.push(SupplierProductMovement {
                id: r.get("id"),
                date: r.get("d"),
                document_type: "receipt".to_string(),
                document_number: number.clone(),
                document_id: r.get("doc_id"),
                quantity: r.get("qty"),
                price: r.get("price"),
                total: r.get("total"),
                notes: Some(format!("Чек: {number}")),
            });
        }

        // 3.4 Списання (витрата). БЕЗ статус-фільтру (як Python).
        //     price/total: Python `item.price or 0` / `item.quantity * (item.price or 0)`
        //     — Decimal-арифметика зі scale, відтворюємо через rust_decimal.
        let rows = sqlx::query(
            "SELECT wi.id, w.created_at AS d, w.number AS n, w.id AS doc_id,
                    wi.quantity::text AS qty, wi.price::text AS price
             FROM write_off_items wi
             JOIN write_offs w ON w.id = wi.write_off_id
             WHERE wi.product_id = $1
             ORDER BY w.created_at DESC
             LIMIT $2",
        )
        .bind(product_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        for r in &rows {
            let number: String = r.get("n");
            let qty_s: String = r.get("qty");
            let price_s: String = r.get("price");
            let price_d = py_or_zero(&price_s);
            let qty_d = RDecimal::from_str(&qty_s).unwrap_or_default();
            let price_decimal = RDecimal::from_str(&price_d).unwrap_or_default();
            movements.push(SupplierProductMovement {
                id: r.get("id"),
                date: r.get("d"),
                document_type: "write_off".to_string(),
                document_number: number.clone(),
                document_id: r.get("doc_id"),
                quantity: format!("-{qty_s}"),
                price: Some(price_d),
                total: Some((-(qty_d * price_decimal)).to_string()),
                notes: Some(format!("Списання: {number}")),
            });
        }

        // 3.5 Переміщення (витрата зі складу) — CONFIRMED.
        let rows = sqlx::query(
            "SELECT ti.id, t.created_at AS d, t.number AS n, t.id AS doc_id,
                    ti.quantity::text AS qty, ti.price::text AS price
             FROM transfer_items ti
             JOIN transfers t ON t.id = ti.transfer_id
             WHERE ti.product_id = $1 AND t.status = 'confirmed'
             ORDER BY t.created_at DESC
             LIMIT $2",
        )
        .bind(product_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        for r in &rows {
            let number: String = r.get("n");
            let qty_s: String = r.get("qty");
            let price_s: String = r.get("price");
            let price_d = py_or_zero(&price_s);
            let qty_d = RDecimal::from_str(&qty_s).unwrap_or_default();
            let price_decimal = RDecimal::from_str(&price_d).unwrap_or_default();
            movements.push(SupplierProductMovement {
                id: r.get("id"),
                date: r.get("d"),
                document_type: "transfer".to_string(),
                document_number: number.clone(),
                document_id: r.get("doc_id"),
                quantity: format!("-{qty_s}"),
                price: Some(price_d),
                total: Some((-(qty_d * price_decimal)).to_string()),
                notes: Some(format!("Переміщення: {number}")),
            });
        }

        // Сортування за датою DESC (стабільне — як Python `sort(reverse=True)`).
        // Python: total_movements=len(movements) ДО обрізання; movements[:limit] після.
        movements.sort_by_key(|m| std::cmp::Reverse(m.date));
        let total_movements = movements.len() as i64;
        movements.truncate(limit as usize);

        Ok(SupplierProductMovementsResponse {
            product,
            movements,
            total_movements,
        })
    }
}

// ─── Продукти: SQL + фільтри ───────────────────────────────────────────────

/// Ефективний пошуковий запит (аліас `search` уже зведений у фільтри
/// на рівні API: `query = query.or(search)` — як Python `query or search`).
fn effective_query(filters: &ProductFilters) -> Option<String> {
    filters.query.clone()
}

impl SqlxDirectories {
    /// WHERE-умови фільтрів товарів — спільні для вибірки рядків і COUNT(*).
    ///
    /// 1:1 Python-еталон `ProductService.search_products` (ILIKE по
    /// title/barcode/sku/додаткових штрих-кодах + фільтри).
    fn push_product_filters<'a>(
        qb: &mut QueryBuilder<'a, sqlx::Postgres>,
        filters: &'a ProductFilters,
    ) {
        let mut conditions = 0usize;
        if let Some(q) = effective_query(filters) {
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
        }
        if let Some(bc) = &filters.barcode {
            push_where(qb, conditions > 0);
            qb.push(" p.barcode = ");
            qb.push_bind(bc);
            conditions += 1;
        }
        if let Some(cid) = filters.category_id {
            push_where(qb, conditions > 0);
            qb.push(" p.category_id = ");
            qb.push_bind(cid);
            conditions += 1;
        }
        if let Some(sid) = filters.supplier_id {
            push_where(qb, conditions > 0);
            qb.push(" p.supplier_id = ");
            qb.push_bind(sid);
            conditions += 1;
        }
        if let Some(min_p) = filters.min_price {
            push_where(qb, conditions > 0);
            qb.push(" COALESCE(NULLIF(st.price, 0), p.price) >= ");
            qb.push_bind(min_p);
            conditions += 1;
        }
        if let Some(max_p) = filters.max_price {
            push_where(qb, conditions > 0);
            qb.push(" COALESCE(NULLIF(st.price, 0), p.price) <= ");
            qb.push_bind(max_p);
            conditions += 1;
        }
        if let Some(w) = filters.is_weight {
            push_where(qb, conditions > 0);
            qb.push(" p.is_weight = ");
            qb.push_bind(w);
        }
    }

    /// Точка в JOIN-умові `stock`: bind із RLS-контексту запиту, якщо він є.
    ///
    /// `current_setting('app.store_id')` у JOIN-умові PostgreSQL обчислює для
    /// КОЖНОГО рядка (план Hash Right Join): на хабі це 134 мс проти 66 мс
    /// Execution Time для 4014 товарів (EXPLAIN ANALYZE, виміряно). Контекст
    /// запиту (task-local) несе той самий `store_id`, тож bind дає ідентичну
    /// семантику — RLS-політики, як і раніше, читають `current_setting`.
    /// Fallback (без контексту — фонові таски, тести) — старий вираз.
    fn push_store_scope<'a>(qb: &mut QueryBuilder<'a, sqlx::Postgres>) {
        match crate::store_ctx::current_store_ctx() {
            Some(ctx) => {
                qb.push_bind(ctx.store_id);
            }
            None => {
                qb.push("NULLIF(current_setting('app.store_id', true), '')::uuid");
            }
        }
    }

    /// Голова вибірки товарів + per-store JOIN stock + WHERE-умови.
    ///
    /// Додає в SELECT `total` для сторінкового запиту.
    fn product_rows_query<'a>(
        filters: &'a ProductFilters,
        total_mode: TotalMode,
    ) -> QueryBuilder<'a, sqlx::Postgres> {
        let mut qb = QueryBuilder::new(
            "SELECT p.id, p.barcode, p.sku, p.title, p.description,
                    COALESCE(NULLIF(st.price, 0), p.price)::text AS price,
                    p.cost_price::text, p.markup::text,
                    COALESCE(st.quantity, p.stock)::text AS stock,
                    p.recommended_qty::text, p.uktzed, p.scan_excise, p.tax_rate::text,
                    p.tax_group, p.is_weight, p.unit, p.category_id, p.supplier_id,
                    p.created_at, p.updated_at",
        );
        match total_mode {
            TotalMode::Window => {
                qb.push(", count(*) OVER () AS total_count");
            }
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
        }
        qb.push(" FROM products p");
        // Ціна/залишок — per-store, як у get_product (еталон): рядок stock
        // поточної точки ПЕРЕКРИВАЄ глобальний дефолт products.
        qb.push(" LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = ");
        Self::push_store_scope(&mut qb);
        Self::push_product_filters(&mut qb, filters);
        qb
    }

    /// Сторінковий запит: той самий SELECT + `COUNT(*) OVER ()` + ORDER BY/LIMIT.
    fn product_page_query<'a>(filters: &'a ProductFilters) -> QueryBuilder<'a, sqlx::Postgres> {
        let mut qb = Self::product_rows_query(filters, total_mode(filters));
        match effective_query(filters) {
            // Пошук: наявність → релевантність → назва (bind-параметри).
            Some(q) => push_search_order(&mut qb, &q),
            // Каталог без пошуку: алфавіт за назвою (як Python-еталон).
            None => {
                qb.push(PLAIN_ORDER_BY);
            }
        }
        qb.push(" LIMIT ");
        qb.push_bind(filters.size.max(0));
        qb.push(" OFFSET ");
        qb.push_bind(((filters.page - 1) * filters.size).max(0));
        qb
    }

    /// Сторінка результатів пошуку: сортування й LIMIT/OFFSET — у PostgreSQL.
    /// Повертає (рядки сторінки, total із `COUNT(*) OVER ()`).
    async fn fetch_product_page(
        &self,
        filters: &ProductFilters,
    ) -> Result<(Vec<ProductRow>, Option<i64>), DirectoryError> {
        let rows = Self::product_page_query(filters)
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;

        let total = rows.first().map(|r| r.get::<i64, _>("total_count"));
        Ok((rows.iter().map(row_to_product).collect(), total))
    }

    /// Кількість товарів за фільтрами (fallback, коли сторінка порожня).
    async fn count_product_rows(&self, filters: &ProductFilters) -> Result<i64, DirectoryError> {
        let mut qb = QueryBuilder::new("SELECT count(*) FROM products p");
        qb.push(" LEFT JOIN stock st ON st.product_id = p.id AND st.store_id = ");
        Self::push_store_scope(&mut qb);
        Self::push_product_filters(&mut qb, filters);
        let row = qb.build().fetch_one(&self.pool).await.map_err(db_err)?;
        Ok(row.get::<i64, _>(0))
    }
}

/// Рядок SQL → [`ProductRow`] (спільний для всіх вибірок товарів).
fn row_to_product(r: &sqlx::postgres::PgRow) -> ProductRow {
    ProductRow {
        id: r.get("id"),
        barcode: r.get("barcode"),
        sku: r.get("sku"),
        title: r.get("title"),
        description: r.get("description"),
        price: r.get("price"),
        cost_price: r.get("cost_price"),
        markup: r.get("markup"),
        stock: r.get("stock"),
        recommended_qty: r.get("recommended_qty"),
        uktzed: r.get("uktzed"),
        scan_excise: r.get("scan_excise"),
        tax_rate: r.get("tax_rate"),
        tax_group: r.get("tax_group"),
        is_weight: r.get("is_weight"),
        unit: r.get("unit"),
        category_id: r.get("category_id"),
        supplier_id: r.get("supplier_id"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

/// Зв'язки товарів (зображення, додаткові штрих-коди) — для сторінки результатів.
impl SqlxDirectories {
    /// Зв'язки товарів сторінки: зображення + додаткові штрих-коди — ОДНИМ
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
}

// ─── Допоміжні ─────────────────────────────────────────────────────────────

/// Спосіб отримання `total` для сторінки товарів (обидва дають однакове
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

/// `WHERE` або ` AND ` залежно від наявності попередніх умов.
fn push_where(qb: &mut QueryBuilder<'_, sqlx::Postgres>, has_conditions: bool) {
    if has_conditions {
        qb.push(" AND");
    } else {
        qb.push(" WHERE");
    }
}

/// `pages` як у Python: max(1, ceil(total/size)) при total>0, інакше 1.
fn total_pages(total: i64, size: i64) -> i64 {
    if total > 0 && size > 0 {
        ((total + size - 1) / size).max(1)
    } else {
        1
    }
}

/// ORDER BY результатів пошуку — той самий порядок, що й у
/// Python-еталоні `_relevance_sort_key`, плюс пріоритет наявності
/// (каса: спершу товар, який реально є в наявності в поточній точці).
///
/// 1) залишок > 0 — першими (`COALESCE(..., 0)`, щоб NULL не сплив уверх);
/// 2) клас релевантності 0..4 (starts_with / position — як Python
///    `startswith` / `in`, тож `%` і `_` у запиті трактуються буквально);
/// 3) алфавіт за назвою (стабільний tie-break) — `COLLATE "C"` дає порядок
///    code points, як Python `sorted` по `str`. Без колації locale-правила PG
///    ігнорують лапки/пунктуацію («хліб білий» перед «хліб "гречаний"»),
///    і порядок розходиться з Python-еталоном у межах одного класу.
// ORDER BY каталогу без пошукового тексту — алфавіт за назвою.
// COLLATE "C" — порядок code points, як Python `sorted` по `str`.
const PLAIN_ORDER_BY: &str = " ORDER BY lower(p.title) COLLATE \"C\"";

/// ORDER BY результатів пошуку — той самий порядок, що й у
/// Python-еталоні `_relevance_sort_key`, плюс пріоритет наявності
/// (каса: спершу товар, який реально є в наявності в поточній точці).
///
/// 1) залишок > 0 — першими (`COALESCE(..., 0)`, щоб NULL не сплив уверх);
/// 2) клас релевантності 0..4 (starts_with / position — як Python
///    `startswith` / `in`, тож `%` і `_` у запиті трактуються буквально);
/// 3) алфавіт за назвою (стабільний tie-break) — `COLLATE "C"` дає порядок
///    code points, як Python `sorted` по `str`. Без колації locale-правила PG
///    ігнорують лапки/пунктуацію («хліб білий» перед «хліб "гречаний"»),
///    і порядок розходиться з Python-еталоном у межах одного класу.
fn push_search_order(qb: &mut QueryBuilder<'_, sqlx::Postgres>, query: &str) {
    let q = query.trim().to_lowercase();
    let space_q = format!(" {q}");
    qb.push(
        " ORDER BY (COALESCE(st.quantity, p.stock, 0) > 0) DESC, CASE
                 WHEN starts_with(lower(p.title), ",
    );
    qb.push_bind(q.clone());
    qb.push(
        ") THEN 0
                 WHEN position(",
    );
    qb.push_bind(space_q);
    qb.push(
        " in lower(p.title)) > 0 THEN 1
                 WHEN position(",
    );
    qb.push_bind(q.clone());
    qb.push(
        " in lower(p.title)) > 0 THEN 2
                 WHEN position(",
    );
    qb.push_bind(q);
    qb.push(
        " in lower(COALESCE(NULLIF(p.barcode, ''), p.sku, ''))) > 0 THEN 3
                 ELSE 4
               END,
             lower(p.title) COLLATE \"C\"",
    );
}

/// Python `Decimal(str(x or 0))`: нульове значення (Decimal('0.00') falsy)
/// стає `0` (int) → `Decimal('0')` → рядок "0". Відтворюємо це для
/// price/total write_off/transfer.
fn py_or_zero(s: &str) -> String {
    match RDecimal::from_str(s) {
        Ok(d) if d.is_zero() => "0".to_string(),
        _ => s.to_string(),
    }
}

/// Мапінг sqlx-помилки у доменну.
fn db_err(e: sqlx::Error) -> DirectoryError {
    DirectoryError::Infrastructure(e.to_string())
}
