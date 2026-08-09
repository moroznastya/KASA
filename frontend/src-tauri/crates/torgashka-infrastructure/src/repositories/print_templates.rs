//! Шаблони друку + цінники/етикетки/тестовий друк (етап 8 — група 6).
//!
//! 1:1 з Python:
//!   - api/v1/print.py (4 роути): price-tags/render, labels/render, printers
//!     (lpstat -e), test (receipt/price_tag/label)
//!   - api/v1/print_templates.py (9 роутів): list active, all, default, get,
//!     create, update, delete (soft), set-default, render
//!   - services/print_template_service.py (render = str.replace {{var}}),
//!     services/print_font_service.py (font-family regex + body style)
//!
//! Рендер HTML цінників/етикеток — перевикористання price_tag.rs (група 3):
//! render_price_tags_grid / render_labels_sequential / fields_from_settings /
//! calc_grid — 1:1 з price_tag_print_service.py (850 рядків).
//!
//! ВІДОМА МЕЖА (не ламає default-шлях): Google Font «Bad Script» — Python
//! вбудовує @font-face з base64 (app/assets/fonts/*.woff2, недоступні Rust-
//! бінарнику). Rust застосовує font-family, але не вбудовує @font-face.
//! print_font_family у БД відсутній → default 'Arial, sans-serif' (1:1).

use chrono::NaiveDateTime;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::price_tag::{
    calc_grid, fields_from_settings, render_labels_sequential, render_price_tags_grid,
};
use torgashka_domain::print::{
    LabelRenderDto, LabelRenderInput, PriceTagRenderDto, PriceTagRenderInput, PrintError,
    PrintTemplateCreateInput, PrintTemplateDto, PrintTemplateListDto, PrintTemplateUpdateInput,
    TemplateRenderDto, TestPrintDto, TestPrintInput,
};

fn de(s: String) -> PrintError {
    PrintError::Infrastructure(s)
}

/// Python Pydantic UTC datetime: "2026-07-26T00:06:20.863242Z" — завжди 6
/// цифр мікросекунд (%.6f) + 'Z'.
fn dt_z(dt: NaiveDateTime) -> String {
    let micros = dt.and_utc().timestamp_subsec_micros();
    format!("{}.{:06}Z", dt.format("%Y-%m-%dT%H:%M:%S"), micros)
}

const DT_SELECT: &str = "id, name, type, content, variables, is_default, is_active, \
     created_at AT TIME ZONE 'UTC' AS created_at, updated_at AT TIME ZONE 'UTC' AS updated_at";

fn row_to_dto(r: &sqlx::postgres::PgRow) -> Result<PrintTemplateDto, PrintError> {
    Ok(PrintTemplateDto {
        id: r.try_get("id").map_err(|e| de(e.to_string()))?,
        name: r.try_get("name").map_err(|e| de(e.to_string()))?,
        type_: r.try_get("type").map_err(|e| de(e.to_string()))?,
        content: r.try_get("content").map_err(|e| de(e.to_string()))?,
        variables: r.try_get("variables").map_err(|e| de(e.to_string()))?,
        is_default: r.try_get("is_default").map_err(|e| de(e.to_string()))?,
        is_active: r.try_get("is_active").map_err(|e| de(e.to_string()))?,
        created_at: dt_z(r.try_get("created_at").map_err(|e| de(e.to_string()))?),
        updated_at: dt_z(r.try_get("updated_at").map_err(|e| de(e.to_string()))?),
    })
}

async fn fetch_one(pool: &PgPool, id: Uuid) -> Result<Option<PrintTemplateDto>, PrintError> {
    let row = sqlx::query(&format!(
        "SELECT {DT_SELECT} FROM print_templates WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| de(e.to_string()))?;
    row.map(|r| row_to_dto(&r)).transpose()
}

/// Python PrintTemplateService.get_default_for_type: is_default+active →
/// перший активний (ORDER created_at DESC).
async fn default_for_type(
    pool: &PgPool,
    type_: &str,
) -> Result<Option<PrintTemplateDto>, PrintError> {
    let row = sqlx::query(&format!(
        "SELECT {DT_SELECT} FROM print_templates \
         WHERE type = $1 AND is_default = true AND is_active = true LIMIT 1"
    ))
    .bind(type_)
    .fetch_optional(pool)
    .await
    .map_err(|e| de(e.to_string()))?;
    if let Some(r) = row {
        return row_to_dto(&r).map(Some);
    }
    let row = sqlx::query(&format!(
        "SELECT {DT_SELECT} FROM print_templates \
         WHERE type = $1 AND is_active = true ORDER BY created_at DESC LIMIT 1"
    ))
    .bind(type_)
    .fetch_optional(pool)
    .await
    .map_err(|e| de(e.to_string()))?;
    row.map(|r| row_to_dto(&r)).transpose()
}

/// Python _get_fields_from_settings + _format_price + products_dicts.
fn products_to_dicts(products: &[torgashka_domain::print::PrintProductInput]) -> Vec<Value> {
    products
        .iter()
        .map(|p| {
            json!({
                "id": p.id.to_string(),
                "title": p.title,
                "price": format_price(&p.price),
                "barcode": p.barcode.clone().unwrap_or_default(),
                "article": p.article.clone().unwrap_or_default(),
                "category": p.category.clone().unwrap_or_default(),
                "copies": p.copies,
            })
        })
        .collect()
}

/// Python _format_price: f"{float(price):.2f}", None/нечисловий → "0.00".
fn format_price(price: &str) -> String {
    match price.trim().parse::<f64>() {
        Ok(v) => format!("{v:.2}"),
        Err(_) => "0.00".to_string(),
    }
}

/// Python SettingsService.get_string (кешований) — читає system_settings.
async fn get_setting(pool: &PgPool, key: &str) -> Result<Option<String>, PrintError> {
    let row = sqlx::query("SELECT value FROM system_settings WHERE key = $1 AND is_active = true")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| de(e.to_string()))?;
    Ok(row.and_then(|r| r.get::<Option<String>, _>("value")))
}

/// Python PrintFontService.get_font_family: print_font_family або default;
/// 'custom'/порожній → default.
async fn font_family(pool: &PgPool) -> Result<String, PrintError> {
    let font = get_setting(pool, "print_font_family")
        .await?
        .unwrap_or_default();
    let font = font.trim().to_string();
    if font.is_empty() || font.eq_ignore_ascii_case("custom") {
        return Ok("Arial, sans-serif".to_string());
    }
    Ok(font)
}

/// Python PrintFontService.apply_font_to_html (без Bad Script @font-face —
/// див. відому межу в шапці модуля).
fn apply_font_to_html(html: &str, font_family: &str) -> String {
    if html.is_empty() || font_family.is_empty() {
        return html.to_string();
    }
    // Санітизація: re.sub(r"[^A-Za-z0-9 ,'-]", "", font).strip().
    let mut safe: String = font_family
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == ',' || *c == '-' || *c == '\'')
        .collect();
    safe = safe.trim().to_string();
    if safe.is_empty() {
        safe = "Arial, sans-serif".to_string();
    }
    // _FONT_FAMILY_RE: font-family\s*:\s*((?:[^;"'}]+|"[^"]*"|'[^']*')+)
    let re = regex::Regex::new(r#"(?i)font-family\s*:\s*((?:[^;"'}]+|"[^"]*"|'[^']*')+)"#)
        .expect("font regex");
    let updated = re
        .replace_all(html, |_: &regex::Captures| format!("font-family: {safe}"))
        .to_string();
    let need_body_style = !updated.to_lowercase().contains("font-family");
    if !need_body_style {
        return updated;
    }
    let style_tag = format!("<style>body {{ font-family: {safe}; }}</style>");
    if let Some(pos) = updated.to_lowercase().find("</head>") {
        format!("{}{}{}", &updated[..pos], style_tag, &updated[pos..])
    } else {
        format!("{updated}{style_tag}")
    }
}

/// Python PrintTemplateService.render_template: str.replace("{{key}}", value)
/// у порядку dict. Rust: BTreeMap (serde_json без preserve_order) — порядок
/// за алфавітом ключів; для реальних шаблонів значення не перекриваються.
fn render_replace(content: &str, data: &Value) -> String {
    let mut result = content.to_string();
    if let Some(obj) = data.as_object() {
        for (k, v) in obj {
            let val = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&format!("{{{{{k}}}}}"), &val);
        }
    }
    result
}

/// Python _build_demo_products.
fn demo_products() -> Vec<Value> {
    vec![
        json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "title": "Тестовий товар №1",
            "price": "25.00",
            "barcode": "4820012345678",
            "article": "ТЕСТ-001",
            "category": "Тестова категорія",
            "copies": 1,
        }),
        json!({
            "id": "00000000-0000-0000-0000-000000000002",
            "title": "Тестовий товар №2",
            "price": "45.50",
            "barcode": "4820012345679",
            "article": "ТЕСТ-002",
            "category": "Тестова категорія",
            "copies": 1,
        }),
    ]
}

/// Python test_print receipt test_data (items — фіксований HTML-блок).
fn test_receipt_data() -> Value {
    json!({
        "shop_name": "Мій магазин (ТЕСТ)",
        "shop_address": "вул. Тестова, 1",
        "tax_id": "12345678",
        "receipt_number": "TEST-001",
        "date": "29.07.2026",
        "time": "12:00",
        "cashier": "Тестовий Касир",
        "items": "<div style=\"margin-bottom:4px;\">\
    <div style=\"display:flex;justify-content:space-between;\">\
    <span>Тестовий товар №1</span><span>25.00</span>\
    </div>\
    <div style=\"display:flex;justify-content:space-between;font-size:10px;color:#666;\">\
    <span>1 × 25.00</span><span style=\"font-weight:bold;\">25.00</span>\
    </div>\
    </div>\
    <div style=\"margin-bottom:4px;\">\
    <div style=\"display:flex;justify-content:space-between;\">\
    <span>Тестовий товар №2</span><span>45.50</span>\
    </div>\
    <div style=\"display:flex;justify-content:space-between;font-size:10px;color:#666;\">\
    <span>2 × 22.75</span><span style=\"font-weight:bold;\">45.50</span>\
    </div>\
    </div>",
        "total": "70.50",
        "payment_method": "Готівка",
        "paid": "100.00",
        "change": "29.50",
        "footer": "Дякуємо за покупку!",
    })
}

// ─── Репозиторій ────────────────────────────────────────────────────────────

pub struct SqlxPrintTemplates {
    pool: PgPool,
}

impl SqlxPrintTemplates {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl torgashka_domain::print::PrintTemplatesService for SqlxPrintTemplates {
    async fn list_active(&self, page: i64, size: i64) -> Result<PrintTemplateListDto, PrintError> {
        let offset = (page - 1) * size;
        let rows = sqlx::query(&format!(
            "SELECT {DT_SELECT} FROM print_templates WHERE is_active = true \
             ORDER BY type, name LIMIT $1 OFFSET $2"
        ))
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let mut items = Vec::with_capacity(rows.len());
        for r in &rows {
            items.push(row_to_dto(r)?);
        }
        let count_row =
            sqlx::query("SELECT count(*)::bigint AS c FROM print_templates WHERE is_active = true")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| de(e.to_string()))?;
        let total: i64 = count_row.get("c");
        let pages = if total == 0 {
            1
        } else {
            (total + size - 1) / size
        };
        Ok(PrintTemplateListDto {
            items,
            total,
            page,
            page_size: size,
            pages,
        })
    }

    async fn list_all(&self) -> Result<Vec<PrintTemplateDto>, PrintError> {
        let rows = sqlx::query(&format!(
            "SELECT {DT_SELECT} FROM print_templates ORDER BY type, name"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        rows.iter().map(row_to_dto).collect()
    }

    async fn get_default(&self, type_: &str) -> Result<PrintTemplateDto, PrintError> {
        default_for_type(&self.pool, type_).await?.ok_or_else(|| {
            PrintError::NotFound(format!(
                "Не знайдено жодного активного шаблону для типу '{type_}'"
            ))
        })
    }

    async fn get(&self, id: Uuid) -> Result<PrintTemplateDto, PrintError> {
        fetch_one(&self.pool, id)
            .await?
            .ok_or_else(|| PrintError::NotFound(format!("Шаблон з ID '{id}' не знайдено")))
    }

    async fn create(
        &self,
        input: &PrintTemplateCreateInput,
    ) -> Result<PrintTemplateDto, PrintError> {
        // Python: якщо is_default — знімаємо з інших шаблонів цього типу.
        if input.is_default {
            sqlx::query(
                "UPDATE print_templates SET is_default = false \
                 WHERE type = $1 AND is_default = true",
            )
            .bind(&input.type_)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        // Python SQLAlchemy: id=uuid.uuid4 (default у моделі) — Rust генерує явно.
        let new_id = Uuid::new_v4();
        let row = sqlx::query(&format!(
            "INSERT INTO print_templates (id, name, type, content, variables, is_default, is_active, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, true, now(), now()) RETURNING {DT_SELECT}"
        ))
        .bind(new_id)
        .bind(&input.name)
        .bind(&input.type_)
        .bind(&input.content)
        .bind(&input.variables)
        .bind(input.is_default)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        row_to_dto(&row)
    }

    async fn update(
        &self,
        id: Uuid,
        input: &PrintTemplateUpdateInput,
    ) -> Result<PrintTemplateDto, PrintError> {
        let current = fetch_one(&self.pool, id)
            .await?
            .ok_or_else(|| PrintError::NotFound(format!("Шаблон з ID '{id}' не знайдено")))?;
        // Python: якщо встановлюємо is_default — знімаємо з інших цього типу.
        if input.is_default == Some(true) {
            sqlx::query(
                "UPDATE print_templates SET is_default = false \
                 WHERE type = $1 AND is_default = true AND id != $2",
            )
            .bind(&current.type_)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| de(e.to_string()))?;
        }
        let row = sqlx::query(&format!(
            "UPDATE print_templates SET \
             name = COALESCE($2, name), \
             content = COALESCE($3, content), \
             variables = CASE WHEN $4::boolean THEN $5 ELSE variables END, \
             is_default = COALESCE($6, is_default), \
             is_active = COALESCE($7, is_active), \
             updated_at = now() \
             WHERE id = $1 RETURNING {DT_SELECT}"
        ))
        .bind(id)
        .bind(&input.name)
        .bind(&input.content)
        .bind(input.variables.is_some())
        .bind(&input.variables)
        .bind(input.is_default)
        .bind(input.is_active)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        row_to_dto(&row)
    }

    async fn delete(&self, id: Uuid) -> Result<(), PrintError> {
        let current = fetch_one(&self.pool, id)
            .await?
            .ok_or_else(|| PrintError::NotFound(format!("Шаблон з ID '{id}' не знайдено")))?;
        let _ = current;
        sqlx::query(
            "UPDATE print_templates SET is_active = false, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        Ok(())
    }

    async fn set_default(&self, id: Uuid) -> Result<PrintTemplateDto, PrintError> {
        let current = fetch_one(&self.pool, id)
            .await?
            .ok_or_else(|| PrintError::NotFound(format!("Шаблон з ID '{id}' не знайдено")))?;
        // Python service.set_as_default: знімає is_default з усіх типу.
        sqlx::query(
            "UPDATE print_templates SET is_default = false \
             WHERE type = $1 AND is_default = true",
        )
        .bind(&current.type_)
        .execute(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        let row = sqlx::query(&format!(
            "UPDATE print_templates SET is_default = true, updated_at = now() \
             WHERE id = $1 RETURNING {DT_SELECT}"
        ))
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| de(e.to_string()))?;
        row_to_dto(&row)
    }

    async fn render_template(
        &self,
        id: Uuid,
        data: &Value,
    ) -> Result<TemplateRenderDto, PrintError> {
        let tpl = fetch_one(&self.pool, id)
            .await?
            .ok_or_else(|| PrintError::NotFound(format!("Шаблон з ID '{id}' не знайдено")))?;
        let html = render_replace(&tpl.content, data);
        let font = font_family(&self.pool).await?;
        Ok(TemplateRenderDto {
            html: apply_font_to_html(&html, &font),
        })
    }

    async fn render_price_tags(
        &self,
        input: &PriceTagRenderInput,
    ) -> Result<PriceTagRenderDto, PrintError> {
        // 1. Шаблон (активний).
        let tpl = fetch_one(&self.pool, input.template_id).await?;
        let tpl = tpl.filter(|t| t.is_active).ok_or_else(|| {
            PrintError::NotFound(format!(
                "Шаблон з ID '{}' не знайдено або він неактивний",
                input.template_id
            ))
        })?;
        // 2. Налаштування полів.
        let fields = fields_from_settings(
            &self.pool,
            "price_tag_fields",
            &["title", "price", "barcode"],
        )
        .await
        .map_err(|e| PrintError::Infrastructure(e.to_string()))?;
        // 3. Продукти.
        let products = products_to_dicts(&input.products);
        // 4. Налаштування сервісу.
        let settings = json!({
            "width_mm": input.width_mm,
            "height_mm": input.height_mm,
            "gap_mm": input.gap_mm,
            "margin_mm": input.margin_mm,
            "page_width_mm": 210.0,
            "page_height_mm": 297.0,
            "fields": fields,
            "barcode_type": input.barcode_type,
            "barcode_height_mm": input.barcode_height_mm,
        });
        // 5. Рендер.
        let html = render_price_tags_grid(&tpl.content, &products, &settings);
        // 5а. Шрифт.
        let font = font_family(&self.pool).await?;
        let html = apply_font_to_html(&html, &font);
        // 6. Мета-інформація.
        let total_labels: i64 = input.products.iter().map(|p| p.copies).sum();
        let (_, _, per_page) = calc_grid(
            input.width_mm,
            input.height_mm,
            input.gap_mm,
            210.0,
            297.0,
            input.margin_mm,
        );
        let total_pages = if per_page > 0 {
            (total_labels as f64 / per_page as f64).ceil() as i64
        } else {
            1
        }
        .max(1);
        Ok(PriceTagRenderDto {
            html,
            total_pages,
            total_labels,
        })
    }

    async fn render_labels(&self, input: &LabelRenderInput) -> Result<LabelRenderDto, PrintError> {
        let tpl = fetch_one(&self.pool, input.template_id).await?;
        let tpl = tpl.filter(|t| t.is_active).ok_or_else(|| {
            PrintError::NotFound(format!(
                "Шаблон з ID '{}' не знайдено або він неактивний",
                input.template_id
            ))
        })?;
        let fields =
            fields_from_settings(&self.pool, "label_fields", &["title", "price", "barcode"])
                .await
                .map_err(|e| PrintError::Infrastructure(e.to_string()))?;
        let products = products_to_dicts(&input.products);
        let settings = json!({
            "width_mm": input.width_mm,
            "height_mm": input.height_mm,
            "gap_mm": input.gap_mm,
            "fields": fields,
            "barcode_type": input.barcode_type,
            "barcode_height_mm": input.barcode_height_mm,
            "print_mode": input.print_mode,
        });
        let html = render_labels_sequential(&tpl.content, &products, &settings);
        let font = font_family(&self.pool).await?;
        let html = apply_font_to_html(&html, &font);
        let total_labels: i64 = input.products.iter().map(|p| p.copies).sum();
        Ok(LabelRenderDto { html, total_labels })
    }

    async fn test_print(&self, input: &TestPrintInput) -> Result<TestPrintDto, PrintError> {
        // ─── Тест цінника / етикетки ─────────────────────────────────────
        if input.print_type == "price_tag" || input.print_type == "label" {
            let is_price_tag = input.print_type == "price_tag";
            let template_type = if is_price_tag { "price_tag" } else { "label" };
            // 1. Шаблон: template_id → налаштування → default для типу.
            let mut template = None;
            if let Some(tid) = input.template_id {
                let t = fetch_one(&self.pool, tid).await?;
                if let Some(t) = t {
                    if t.is_active {
                        template = Some(t);
                    }
                }
            }
            if template.is_none() {
                let setting_key = if is_price_tag {
                    "price_tag_template_id"
                } else {
                    "label_template_id"
                };
                if let Some(v) = get_setting(&self.pool, setting_key).await? {
                    if let Ok(tid) = Uuid::parse_str(v.trim()) {
                        if let Some(t) = fetch_one(&self.pool, tid).await? {
                            if t.is_active {
                                template = Some(t);
                            }
                        }
                    }
                }
            }
            if template.is_none() {
                template = default_for_type(&self.pool, template_type).await?;
            }
            let template = template.ok_or_else(|| {
                PrintError::NotFound(format!(
                    "Не знайдено шаблону для типу '{template_type}'. \
                     Створіть шаблон у розділі «Шаблони друку» або передайте template_id."
                ))
            })?;
            // 2. Поля.
            let fields_key = if is_price_tag {
                "price_tag_fields"
            } else {
                "label_fields"
            };
            let fields =
                fields_from_settings(&self.pool, fields_key, &["title", "price", "barcode"])
                    .await
                    .map_err(|e| PrintError::Infrastructure(e.to_string()))?;
            // 3. Демо-товари.
            let products = demo_products();
            // 4. Розміри за замовчуванням.
            let (width_mm, height_mm, gap_mm, margin_mm) = if is_price_tag {
                (
                    input.width_mm.unwrap_or(40.0),
                    input.height_mm.unwrap_or(25.0),
                    input.gap_mm.unwrap_or(3.0),
                    input.margin_mm.unwrap_or(10.0),
                )
            } else {
                (
                    input.width_mm.unwrap_or(58.0),
                    input.height_mm.unwrap_or(40.0),
                    input.gap_mm.unwrap_or(3.0),
                    input.margin_mm.unwrap_or(0.0),
                )
            };
            let settings = json!({
                "width_mm": width_mm,
                "height_mm": height_mm,
                "gap_mm": gap_mm,
                "margin_mm": margin_mm,
                "page_width_mm": 210.0,
                "page_height_mm": 297.0,
                "fields": fields,
                "barcode_type": input.barcode_type,
                "barcode_height_mm": input.barcode_height_mm,
                "print_mode": "system",
            });
            // 5. Рендер.
            let (html, label_word) = if is_price_tag {
                (
                    render_price_tags_grid(&template.content, &products, &settings),
                    "цінник",
                )
            } else {
                (
                    render_labels_sequential(&template.content, &products, &settings),
                    "етикетка",
                )
            };
            let font = font_family(&self.pool).await?;
            let html = apply_font_to_html(&html, &font);
            return Ok(TestPrintDto {
                status: "success".to_string(),
                message: format!(
                    "Тестовий {label_word} згенеровано (шаблон: {}, принтер: {})",
                    template.name,
                    if input.printer_name.is_empty() {
                        "системний"
                    } else {
                        &input.printer_name
                    }
                ),
                preview_html: Some(html),
                template_name: Some(template.name),
            });
        }

        // ─── Тестовий чек (зворотна сумісність) ──────────────────────────
        let template = default_for_type(&self.pool, &input.template_type)
            .await?
            .ok_or_else(|| {
                PrintError::NotFound(format!(
                    "Не знайдено шаблону для типу '{}'",
                    input.template_type
                ))
            })?;
        let test_data = test_receipt_data();
        let html = render_replace(&template.content, &test_data);
        let font = font_family(&self.pool).await?;
        let html = apply_font_to_html(&html, &font);
        Ok(TestPrintDto {
            status: "success".to_string(),
            message: format!(
                "Тестовий чек згенеровано (шаблон: {}, принтер: {})",
                template.name,
                if input.printer_name.is_empty() {
                    "системний"
                } else {
                    &input.printer_name
                }
            ),
            preview_html: Some(html),
            template_name: Some(template.name),
        })
    }
}

/// GET /print/printers: `lpstat -e` (CUPS). Помилка → порожній список.
pub async fn list_printers() -> Vec<String> {
    let Ok(out) = tokio::process::Command::new("lpstat")
        .arg("-e")
        .output()
        .await
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    #[test]
    fn dt_z_format() {
        let dt =
            NaiveDateTime::parse_from_str("2026-07-26 00:06:20.863242", "%Y-%m-%d %H:%M:%S%.f")
                .unwrap();
        assert_eq!(dt_z(dt), "2026-07-26T00:06:20.863242Z");
        let dt0 =
            NaiveDateTime::parse_from_str("2026-07-26 00:06:20", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(dt_z(dt0), "2026-07-26T00:06:20.000000Z");
    }

    #[test]
    fn format_price_py() {
        assert_eq!(format_price("25.00"), "25.00");
        assert_eq!(format_price("45.5"), "45.50");
        assert_eq!(format_price(""), "0.00");
        assert_eq!(format_price("abc"), "0.00");
    }

    #[test]
    fn render_replace_basic() {
        let data = json!({"shop_name": "Torgashka", "total": "100.00"});
        assert_eq!(
            render_replace("<h1>{{shop_name}}</h1><p>{{total}} грн</p>", &data),
            "<h1>Torgashka</h1><p>100.00 грн</p>"
        );
    }

    #[test]
    fn apply_font_basic() {
        let html = "<html><body style=\"font-family: Arial;\">x</body></html>";
        assert_eq!(
            apply_font_to_html(html, "Courier New, monospace"),
            // Python: regex захоплює значення ДО ';' → "font-family: X",
            // оригінальна ';' залишається після збігу → "...monospace;".
            "<html><body style=\"font-family: Courier New, monospace;\">x</body></html>"
        );
        let html2 = "<html><body>x</body></html>";
        let out = apply_font_to_html(html2, "Arial, sans-serif");
        assert!(out.contains("<style>body { font-family: Arial, sans-serif; }</style>"));
    }
}
