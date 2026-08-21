//! Репозиторії довідників (етап 1 — read, sqlx/PostgreSQL).
//!
//! Відтворюють логіку Python-еталону ТОЧНО:
//! - `list_products` — `ProductService.search_products`:
//!   ILIKE по title/barcode/sku/додаткових barcodes (LEFT JOIN + DISTINCT),
//!   фільтри (barcode exact, category, supplier, price, is_weight),
//!   сортування в пам'яті (relevance або title.lower()), пагінація в пам'яті.
//! - `list_categories` / `list_suppliers` — `ORDER BY name` + LIMIT/OFFSET,
//!   balance через підзапит SUM(supplier_ledger.amount).

use chrono::NaiveDateTime;
use sqlx::{QueryBuilder, Row};
use crate::store_ctx::StorePool;
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
        let rows = self.fetch_product_rows(filters).await?;

        // Сортування в пам'яті — точно як Python (після SQL, без ORDER BY у запиті).
        let mut rows = rows;
        match effective_query(filters) {
            Some(q) => {
                let q = q.to_lowercase();
                rows.sort_by_key(|p| relevance_sort_key(&q, p));
            }
            None => rows.sort_by_key(|p| p.title.to_lowercase()),
        }

        // Пагінація в пам'яті — як Python (offset/slice).
        let offset = ((filters.page - 1) * filters.size).max(0) as usize;
        let page_rows: Vec<ProductRow> = rows
            .iter()
            .skip(offset)
            .take(filters.size.max(0) as usize)
            .cloned()
            .collect();

        let total = rows.len() as i64;
        let pages = total_pages(total, filters.size);

        // Зв'язки (images, barcodes) — для товарів поточної сторінки.
        let ids: Vec<Uuid> = page_rows.iter().map(|r| r.id).collect();
        let (images, barcodes) = if ids.is_empty() {
            (
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
            )
        } else {
            let images = self.fetch_images(&ids).await?;
            let barcodes = self.fetch_barcodes(&ids).await?;
            (images, barcodes)
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
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM categories")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;

        let offset = ((page - 1) * size).max(0);
        let rows = sqlx::query(
            "SELECT id, name, description, parent_id, created_at, updated_at
             FROM categories
             ORDER BY name
             LIMIT $1 OFFSET $2",
        )
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

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
        let total: i64 = sqlx::query_scalar("SELECT count(*) FROM suppliers")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;

        let offset = ((page - 1) * size).max(0);
        let rows = sqlx::query(
            "SELECT s.id, s.name, s.edrpou, s.phone, s.email, s.address, s.notes,
                    COALESCE((SELECT SUM(amount) FROM supplier_ledger sl
                              WHERE sl.supplier_id = s.id), 0)::numeric(12,2)::text AS current_balance,
                    s.created_at, s.updated_at
             FROM suppliers s
             ORDER BY s.name
             LIMIT $1 OFFSET $2",
        )
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

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
                    p.price::text, p.cost_price::text, p.markup::text, p.stock::text,
                    p.recommended_qty::text, p.uktzed, p.scan_excise, p.tax_rate::text,
                    p.tax_group, p.is_weight, p.unit, p.category_id, p.supplier_id,
                    p.created_at, p.updated_at
             FROM products p WHERE p.id = $1",
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
        let images = self.fetch_images(&ids).await?;
        let barcodes = self.fetch_barcodes(&ids).await?;
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
            "SELECT p.id, p.barcode, p.sku, p.title, p.price::text,
                    p.cost_price::text, p.stock::text, p.unit,
                    c.name AS category_name
             FROM products p
             LEFT JOIN categories c ON c.id = p.category_id
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
            "SELECT p.id, p.barcode, p.sku, p.title, p.price::text,
                    p.cost_price::text, p.stock::text, p.unit,
                    c.name AS category_name
             FROM products p
             LEFT JOIN categories c ON c.id = p.category_id
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
    /// Повний SELECT продуктів з фільтрами (без пагінації/сортування — як Python).
    async fn fetch_product_rows(
        &self,
        filters: &ProductFilters,
    ) -> Result<Vec<ProductRow>, DirectoryError> {
        let mut qb = QueryBuilder::new(
            "SELECT DISTINCT p.id, p.barcode, p.sku, p.title, p.description,
                    p.price::text, p.cost_price::text, p.markup::text, p.stock::text,
                    p.recommended_qty::text, p.uktzed, p.scan_excise, p.tax_rate::text,
                    p.tax_group, p.is_weight, p.unit, p.category_id, p.supplier_id,
                    p.created_at, p.updated_at
             FROM products p",
        );

        // Пошук: LEFT JOIN barcodes лише коли є текстовий запит.
        let query = effective_query(filters);
        if query.is_some() {
            qb.push(" LEFT JOIN barcodes b ON b.product_id = p.id");
        }

        let mut conditions = 0usize;
        if let Some(q) = &query {
            let pattern = format!("%{q}%");
            qb.push(" WHERE (p.title ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR p.barcode ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR p.sku ILIKE ");
            qb.push_bind(pattern.clone());
            qb.push(" OR b.barcode ILIKE ");
            qb.push_bind(pattern);
            qb.push(")");
            conditions += 1;
        }
        if let Some(bc) = &filters.barcode {
            push_where(&mut qb, conditions > 0);
            qb.push(" p.barcode = ");
            qb.push_bind(bc);
            conditions += 1;
        }
        if let Some(cid) = filters.category_id {
            push_where(&mut qb, conditions > 0);
            qb.push(" p.category_id = ");
            qb.push_bind(cid);
            conditions += 1;
        }
        if let Some(sid) = filters.supplier_id {
            push_where(&mut qb, conditions > 0);
            qb.push(" p.supplier_id = ");
            qb.push_bind(sid);
            conditions += 1;
        }
        if let Some(min_p) = filters.min_price {
            push_where(&mut qb, conditions > 0);
            qb.push(" p.price >= ");
            qb.push_bind(min_p);
            conditions += 1;
        }
        if let Some(max_p) = filters.max_price {
            push_where(&mut qb, conditions > 0);
            qb.push(" p.price <= ");
            qb.push_bind(max_p);
            conditions += 1;
        }
        if let Some(w) = filters.is_weight {
            push_where(&mut qb, conditions > 0);
            qb.push(" p.is_weight = ");
            qb.push_bind(w);
        }

        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;

        Ok(rows
            .iter()
            .map(|r| ProductRow {
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
            })
            .collect())
    }

    /// Зображення товарів сторінки (product_id → список, ORDER BY id).
    async fn fetch_images(
        &self,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Vec<ProductImageDto>>, DirectoryError> {
        let rows = sqlx::query(
            "SELECT id, product_id, url, is_main, sort_order, created_at
             FROM product_images WHERE product_id = ANY($1) ORDER BY id",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut map: std::collections::HashMap<Uuid, Vec<ProductImageDto>> =
            std::collections::HashMap::new();
        for r in &rows {
            let pid: Uuid = r.get("product_id");
            map.entry(pid).or_default().push(ProductImageDto {
                id: r.get("id"),
                url: r.get("url"),
                is_main: r.get("is_main"),
                sort_order: r.get("sort_order"),
                created_at: r.get("created_at"),
            });
        }
        Ok(map)
    }

    /// Додаткові штрих-коди товарів сторінки (product_id → список).
    async fn fetch_barcodes(
        &self,
        ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Vec<BarcodeDto>>, DirectoryError> {
        let rows = sqlx::query(
            "SELECT id, product_id, barcode, is_primary, created_at
             FROM barcodes WHERE product_id = ANY($1) ORDER BY id",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut map: std::collections::HashMap<Uuid, Vec<BarcodeDto>> =
            std::collections::HashMap::new();
        for r in &rows {
            let pid: Uuid = r.get("product_id");
            map.entry(pid).or_default().push(BarcodeDto {
                id: r.get("id"),
                barcode: r.get("barcode"),
                is_primary: r.get("is_primary"),
                created_at: r.get("created_at"),
            });
        }
        Ok(map)
    }
}

// ─── Допоміжні ─────────────────────────────────────────────────────────────

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

/// Ключ сортування релевантності — точна копія `_relevance_sort_key` Python.
///
/// Пріоритет: 0 — title починається з q; 1 — title містить " q";
/// 2 — title містить q; 3 — barcode/sku містить q; 4 — інше.
fn relevance_sort_key(q: &str, p: &ProductRow) -> (u8, String) {
    let title = p.title.to_lowercase();
    let barcode = p
        .barcode
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(p.sku.as_deref())
        .unwrap_or("")
        .to_lowercase();

    if title.starts_with(q) {
        (0, title)
    } else if title.contains(&format!(" {q}")) {
        (1, title)
    } else if title.contains(q) {
        (2, title)
    } else if barcode.contains(q) {
        (3, title)
    } else {
        (4, title)
    }
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
