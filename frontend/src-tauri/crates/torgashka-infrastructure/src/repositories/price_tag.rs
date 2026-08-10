//! Друк цінників/етикеток з накладної (етап 8 — група 3).
//!
//! 1:1 з Python app/infrastructure/services/price_tag_print_service.py (850
//! рядків): Handlebars-блоки {{#if ...}}, заміна {{var}}, _extract_body,
//! _calc_grid, _expand_products, _build_label_html, grid/sequential HTML.
//!
//! ВІДМІННІСТЬ (дозволена контрактом): SVG штрих-коду генерується власним
//! Code128-рендером Rust (python-barcode на Python :8001 дає інші байти SVG).
//! Структура/стилі/дані HTML — ідентичні; differential нормалізує SVG-блоки.

use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::invoices::SqlxInvoices;
use torgashka_domain::invoices::{InvoicePrintDto, InvoicePrintRequest, InvoicesError};

fn de(s: String) -> InvoicesError {
    InvoicesError::Infrastructure(s)
}

/// f64 у стилі Python repr: 10.0 → "10.0", 10.25 → "10.25".
fn pf(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{:.1}", v)
    } else {
        format!("{}", v)
    }
}

/// Python html.escape(value, quote=True).
fn esc(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

fn esc_any(v: &Value) -> String {
    esc(v.as_str().unwrap_or(""))
}

/// Python Decimal(str(x)).quantize(0.01) → рядок.
fn q2(s: &str) -> String {
    let d: rust_decimal::Decimal = s.trim().parse().unwrap_or_default();
    d.round_dp(2).to_string()
}

/// Головна точка входу: друк з накладної (v1/v2 спільна логіка).
pub async fn print_invoice_items(
    svc: &SqlxInvoices,
    invoice_id: Uuid,
    req: &InvoicePrintRequest,
) -> Result<InvoicePrintDto, InvoicesError> {
    let pool = svc.pg_pool();
    // 1. Накладна з товарами.
    let row = sqlx::query("SELECT i.number, i.status::text AS st FROM invoices i WHERE i.id = $1")
        .bind(invoice_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| de(e.to_string()))?;
    let Some(r) = row else {
        return Err(InvoicesError::NotFound(format!(
            "Накладну з ID '{invoice_id}' не знайдено"
        )));
    };
    let st: String = r.get("st");
    if st != "confirmed" {
        return Err(InvoicesError::BadRequest(format!(
            "Друк цінників/етикеток можливий тільки для підтверджених накладних. Поточний статус: {st}"
        )));
    }
    let items = sqlx::query(
        "SELECT ii.product_id, ii.price::text AS inv_price, ii.previous_price::text AS prev, \
         p.title, p.barcode, p.sku, p.price::text AS cur \
         FROM invoice_items ii LEFT JOIN products p ON p.id = ii.product_id \
         WHERE ii.invoice_id = $1",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await
    .map_err(|e| de(e.to_string()))?;
    if items.is_empty() {
        return Err(InvoicesError::BadRequest(
            "Накладна не містить товарів".into(),
        ));
    }
    // 2. Шаблон.
    let trow = sqlx::query("SELECT content, is_active FROM print_templates WHERE id = $1")
        .bind(req.template_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| de(e.to_string()))?;
    let Some(trow) = trow else {
        return Err(InvoicesError::NotFound(format!(
            "Шаблон з ID '{}' не знайдено або він неактивний",
            req.template_id
        )));
    };
    let is_active: bool = trow.get("is_active");
    if !is_active {
        return Err(InvoicesError::NotFound(format!(
            "Шаблон з ID '{}' не знайдено або він неактивний",
            req.template_id
        )));
    }
    let template: String = trow.get("content");
    // 3. Налаштування полів.
    let fields_key = if req.print_type == "price_tag" {
        "price_tag_fields"
    } else {
        "label_fields"
    };
    let fields = fields_from_settings(pool, fields_key, &["title", "price", "barcode"]).await?;
    // 4. Продукти + зміни цін.
    let now_str = chrono::Utc::now().format("%d.%m.%Y").to_string();
    let mut products: Vec<Value> = Vec::new();
    let mut price_changes: Vec<Value> = Vec::new();
    let mut changed_count: i64 = 0;
    for it in &items {
        let Some(title) = it.get::<Option<String>, _>("title") else {
            continue;
        };
        let pid: Uuid = it.get("product_id");
        let inv = q2(&it.get::<String, _>("inv_price"));
        let cur = q2(&it.get::<String, _>("cur"));
        let prev = q2(&it.get::<Option<String>, _>("prev").unwrap_or_default());
        let diff = {
            let a: rust_decimal::Decimal = prev.parse().unwrap_or_default();
            let b: rust_decimal::Decimal = inv.parse().unwrap_or_default();
            (a - b).round_dp(2).to_string()
        };
        let changed = diff != "0.00";
        let barcode: Option<String> = it.get("barcode");
        price_changes.push(json!({
            "product_id": pid.to_string(),
            "title": title,
            "barcode": barcode.clone().unwrap_or_default(),
            "article": it.get::<Option<String>, _>("sku").unwrap_or_default(),
            "invoice_price": inv,
            "current_price": cur,
            "changed": changed,
            "difference": diff,
        }));
        if changed {
            changed_count += 1;
        }
        products.push(json!({
            "id": pid.to_string(),
            "title": title,
            "price": cur,
            "barcode": barcode.clone().unwrap_or_default(),
            "article": it.get::<Option<String>, _>("sku").unwrap_or_default(),
            "category": "",
            "copies": 1,
            "created_date": now_str,
        }));
    }
    if products.is_empty() {
        return Err(InvoicesError::BadRequest(
            "Не знайдено товарів для друку".into(),
        ));
    }
    let total_count = items.len() as i64;
    // 5. only_changed.
    if req.only_changed {
        let mut filtered = Vec::new();
        for (i, p) in products.iter().enumerate() {
            if price_changes[i]["changed"].as_bool().unwrap_or(false) {
                filtered.push(p.clone());
            }
        }
        products = filtered;
        if products.is_empty() {
            return Ok(InvoicePrintDto {
                html: String::new(),
                total_labels: 0,
                total_pages: Some(0),
                changed_count: 0,
                total_count,
            });
        }
    }
    // 6. settings.
    let mut settings = json!({
        "width_mm": req.width_mm,
        "height_mm": req.height_mm,
        "gap_mm": req.gap_mm,
        "fields": fields,
        "barcode_type": req.barcode_type,
        "barcode_height_mm": req.barcode_height_mm,
        "print_mode": req.print_mode,
    });
    if req.print_type == "price_tag" {
        settings["margin_mm"] = json!(req.margin_mm);
        settings["page_width_mm"] = json!(210.0);
        settings["page_height_mm"] = json!(297.0);
    }
    // 7. Рендер.
    if req.print_type == "price_tag" {
        let html = render_price_tags_grid(&template, &products, &settings);
        let (_, _, per_page) = calc_grid(
            req.width_mm,
            req.height_mm,
            req.gap_mm,
            210.0,
            297.0,
            req.margin_mm,
        );
        let total_labels = products.len() as i64;
        let total_pages = if per_page > 0 {
            (total_labels + per_page - 1) / per_page
        } else {
            1
        };
        Ok(InvoicePrintDto {
            html,
            total_labels,
            total_pages: Some(total_pages),
            changed_count,
            total_count,
        })
    } else {
        let html = render_labels_sequential(&template, &products, &settings);
        Ok(InvoicePrintDto {
            html,
            total_labels: products.len() as i64,
            total_pages: None,
            changed_count,
            total_count,
        })
    }
}

/// Python _get_fields_from_settings.
pub(crate) async fn fields_from_settings(
    pool: &PgPool,
    key: &str,
    default_fields: &[&str],
) -> Result<Vec<String>, InvoicesError> {
    let row = sqlx::query("SELECT value FROM system_settings WHERE key = $1 AND is_active = true")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| de(e.to_string()))?;
    if let Some(r) = row {
        let value: Option<String> = r.get("value");
        if let Some(v) = value {
            if let Ok(parsed) = serde_json::from_str::<Value>(&v) {
                if let Some(arr) = parsed.as_array() {
                    if !arr.is_empty() {
                        let out: Vec<String> = arr
                            .iter()
                            .filter_map(|f| f.as_str().map(|s| s.to_string()))
                            .collect();
                        if !out.is_empty() {
                            return Ok(out);
                        }
                    }
                }
            }
        }
    }
    Ok(default_fields.iter().map(|s| s.to_string()).collect())
}

// ─── Рендер шаблону ─────────────────────────────────────────────────────────

/// Python PriceTagPrintService._render_single.
fn render_single(
    template: &str,
    product: &Value,
    enabled_fields: Option<&std::collections::HashSet<String>>,
    extra_context: &Value,
) -> String {
    let mut result = template.to_string();
    // Крок 1: Handlebars-блоки.
    let if_map = [
        ("show_barcode", "barcode"),
        ("show_price", "price"),
        ("show_article", "article"),
        ("show_created_date", "created_date"),
        ("show_category", "category"),
    ];
    // Регулярка Python: \{\{#if\s+(show_\w+)\}\}(.*?)\{\{/if\}\} (DOTALL).
    let re = regex::Regex::new(r"(?s)\{\{#if\s+(show_\w+)\}\}(.*?)\{\{/if\}\}").unwrap();
    result = re
        .replace_all(&result, |caps: &regex::Captures| {
            let cond = &caps[1];
            let inner = &caps[2];
            let field_name = if_map.iter().find(|(k, _)| *k == cond).map(|(_, v)| *v);
            match field_name {
                Some(f) => {
                    let show = match enabled_fields {
                        None => true,
                        Some(set) => set.contains(f),
                    };
                    if show {
                        inner.to_string()
                    } else {
                        String::new()
                    }
                }
                None => caps[0].to_string(),
            }
        })
        .to_string();
    // Крок 2: barcode type/height.
    let barcode_type = extra_context
        .get("barcode_type")
        .and_then(|v| v.as_str())
        .unwrap_or("code128")
        .to_string();
    let barcode_height_mm = extra_context
        .get("barcode_height_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(12.0);
    // Крок 3: заміна змінних.
    let barcode_val = product
        .get("barcode")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let title = product.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let barcode_image = generate_barcode_svg(barcode_val, barcode_height_mm, &barcode_type);
    let mut replacements: Vec<(&str, String)> = vec![
        ("title", esc(title)),
        ("name", esc(title)),
        (
            "price",
            esc_any(product.get("price").unwrap_or(&Value::Null)),
        ),
        ("barcode", esc(barcode_val)),
        (
            "article",
            esc(product
                .get("article")
                .and_then(|v| v.as_str())
                .unwrap_or("")),
        ),
        (
            "category",
            esc(product
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("")),
        ),
        (
            "created_date",
            esc(product
                .get("created_date")
                .and_then(|v| v.as_str())
                .unwrap_or("")),
        ),
        ("barcode_image", barcode_image),
        ("barcode_type", esc(&barcode_type)),
        ("barcode_height_mm", esc(&pf(barcode_height_mm))),
        (
            "width",
            esc(extra_context
                .get("width")
                .and_then(|v| v.as_str())
                .unwrap_or("")),
        ),
        (
            "height",
            esc(extra_context
                .get("height")
                .and_then(|v| v.as_str())
                .unwrap_or("")),
        ),
    ];
    for (k, v) in replacements.drain(..) {
        result = result.replace(&format!("{{{{{k}}}}}"), &v);
        result = result.replace(&format!("{{{{product.{k}}}}}"), &v);
    }
    // Крок 4: _extract_body.
    let (body_attrs, body_content) = extract_body(&result);
    if regex::Regex::new(r"(?i)<body[\s>]")
        .unwrap()
        .is_match(&result)
    {
        let style_re = regex::Regex::new(r#"style="([^"]*)""#).unwrap();
        if let Some(m) = style_re.captures(&body_attrs) {
            return format!("<div style=\"{}\">{}</div>", &m[1], body_content);
        }
        return format!("<div>{body_content}</div>");
    }
    result
}

/// Python PriceTagPrintService._extract_body (5 кроків).
fn extract_body(html: &str) -> (String, String) {
    let mut body_attrs = String::new();
    let mut body_content = String::new();
    let mut found = false;
    // 1) <body attrs>...</body>
    if let Some(m) = regex::Regex::new(r"(?is)<body\s+([^>]*)>([\s\S]*?)</body>")
        .unwrap()
        .captures(html)
    {
        body_attrs = m[1].trim().to_string();
        body_content = m[2].to_string();
        found = true;
    }
    // 2) <body>...</body>
    if !found {
        if let Some(m) = regex::Regex::new(r"(?is)<body>([\s\S]*?)</body>")
            .unwrap()
            .captures(html)
        {
            body_attrs = String::new();
            body_content = m[1].to_string();
            found = true;
        }
    }
    // 3) <body attrs>...</body> без закриваючого
    if !found {
        if let Some(m) = regex::Regex::new(r"(?is)<body\s+([^>]*)>([\s\S]*)$")
            .unwrap()
            .captures(html)
        {
            body_attrs = m[1].trim().to_string();
            body_content = m[2].trim_end().to_string();
            found = true;
        }
    }
    // 4) <body>...</body> без закриваючого
    if !found {
        if let Some(m) = regex::Regex::new(r"(?is)<body>([\s\S]*)$")
            .unwrap()
            .captures(html)
        {
            body_attrs = String::new();
            body_content = m[1].trim_end().to_string();
            found = true;
        }
    }
    // 5) body не знайдено — прибираємо обгортку.
    if !found {
        let mut cleaned = regex::Regex::new(r"(?i)<!DOCTYPE[^>]*>")
            .unwrap()
            .replace_all(html, "")
            .to_string();
        cleaned = regex::Regex::new(r"(?i)<html[^>]*>")
            .unwrap()
            .replace_all(&cleaned, "")
            .to_string();
        cleaned = regex::Regex::new(r"(?i)</html>")
            .unwrap()
            .replace_all(&cleaned, "")
            .to_string();
        cleaned = regex::Regex::new(r"(?is)<head>[\s\S]*?</head>")
            .unwrap()
            .replace_all(&cleaned, "")
            .to_string();
        body_content = cleaned.trim().to_string();
    }
    (body_attrs, body_content)
}

/// Python _expand_products.
fn expand_products(products: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for p in products {
        let copies = p.get("copies").and_then(|v| v.as_i64()).unwrap_or(1);
        for _ in 0..copies {
            out.push(p.clone());
        }
    }
    out
}

/// Python _calc_grid.
pub(crate) fn calc_grid(
    width_mm: f64,
    height_mm: f64,
    gap_mm: f64,
    page_width_mm: f64,
    page_height_mm: f64,
    margin_mm: f64,
) -> (i64, i64, i64) {
    let usable_width = page_width_mm - 2.0 * margin_mm;
    let usable_height = page_height_mm - 2.0 * margin_mm;
    if width_mm <= 0.0 || height_mm <= 0.0 {
        return (1, 1, 1);
    }
    let cols = ((usable_width + gap_mm) / (width_mm + gap_mm)) as i64;
    let rows = ((usable_height + gap_mm) / (height_mm + gap_mm)) as i64;
    (cols.max(1), rows.max(1), (cols.max(1)) * (rows.max(1)))
}

// ─── Grid (A4) ──────────────────────────────────────────────────────────────

pub(crate) fn render_price_tags_grid(
    template: &str,
    products: &[Value],
    settings: &Value,
) -> String {
    if products.is_empty() || template.is_empty() {
        return empty_html("A4");
    }
    let width_mm = settings
        .get("width_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(40.0);
    let height_mm = settings
        .get("height_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(25.0);
    let gap_mm = settings
        .get("gap_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(3.0);
    let page_width_mm = settings
        .get("page_width_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(210.0);
    let page_height_mm = settings
        .get("page_height_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(297.0);
    let margin_mm = settings
        .get("margin_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(10.0);
    let fields: Option<Vec<String>> = settings.get("fields").and_then(|v| {
        v.as_array().map(|a| {
            a.iter()
                .filter_map(|f| f.as_str().map(|s| s.to_string()))
                .collect()
        })
    });
    let enabled_fields = fields.map(|f| f.into_iter().collect::<std::collections::HashSet<_>>());
    // Адаптивне обмеження висоти штрих-коду.
    let barcode_h = settings
        .get("barcode_height_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(7.0);
    let barcode_height_mm = barcode_h.min((height_mm * 0.28).max(3.0));
    let expanded = expand_products(products);
    let extra_context = json!({
        "width": pf(width_mm),
        "height": pf(height_mm),
        "barcode_type": settings.get("barcode_type").and_then(|v| v.as_str()).unwrap_or("code128"),
        "barcode_height_mm": pf(barcode_height_mm),
    });
    let mut rendered_items = Vec::new();
    for mut product in expanded {
        if product
            .get("created_date")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty()
        {
            product["created_date"] = json!(chrono::Utc::now().format("%d.%m.%Y").to_string());
        }
        rendered_items.push(render_single(
            template,
            &product,
            enabled_fields.as_ref(),
            &extra_context,
        ));
    }
    let (cols, rows, per_page) = calc_grid(
        width_mm,
        height_mm,
        gap_mm,
        page_width_mm,
        page_height_mm,
        margin_mm,
    );
    let total_labels = rendered_items.len() as i64;
    let total_pages = ((total_labels + per_page - 1) / per_page).max(1);
    let mut pages_html = String::new();
    for page_idx in 0..total_pages {
        let start = (page_idx * per_page) as usize;
        let end = ((start as i64 + per_page).min(total_labels)) as usize;
        let mut grid_cells = String::new();
        for item in &rendered_items[start..end] {
            grid_cells.push_str(&format!("<div class=\"tag-cell\">{item}</div>"));
        }
        let mut page_html = format!(
            "<div class=\"page\"><div class=\"grid-container\" style=\"display: grid; \
             grid-template-columns: repeat({cols}, {w}mm); grid-template-rows: repeat({rows}, {h}mm); \
             gap: {gap}mm; width: {pw}mm; margin: 0;\">{cells}</div></div>",
            w = pf(width_mm),
            h = pf(height_mm),
            gap = pf(gap_mm),
            pw = pf(page_width_mm - 2.0 * margin_mm),
            cells = grid_cells
        );
        if page_idx < total_pages - 1 {
            page_html.push_str("\n<div style=\"page-break-after: always;\"></div>");
        }
        pages_html.push_str(&page_html);
    }
    format!(
        "<!DOCTYPE html>\n<html lang=\"uk\">\n<head>\n<meta charset=\"UTF-8\">\n<title>Цінники A4</title>\n\
         <style>\n@page {{ size: A4; margin: {margin}mm; }}\n\
         * {{ margin: 0; padding: 0; box-sizing: border-box; }}\n\
         body {{ font-family: Arial, Helvetica, sans-serif; font-size: 8pt; line-height: 1.2; }}\n\
         /* .page використовує КОРИСНУ висоту сторінки (page_height − 2×margin):\n   @page margin (10мм) забирає по 10мм зверху/знизу → 297 − 20 = 277мм.\n   Якщо лишити 297мм, контент виходить за друковану область 277мм →\n   порожня друга сторінка або обрізання при друку A4. */\n\
         .page {{ width: 100%; min-height: {ph}mm; }}\n\
         .grid-container {{ display: grid; }}\n\
         .tag-cell {{\n    width: {w}mm; height: {h}mm; overflow: hidden;\n    border: none; padding: 0;\n    display: flex; flex-direction: column; justify-content: stretch;\n    align-items: stretch; text-align: center;\n}}\n\
         @media print {{ .page {{ page-break-after: always; }} }}\n\
         </style>\n</head>\n<body>\n{pages}\n</body>\n</html>",
        margin = pf(margin_mm),
        ph = pf(page_height_mm - 2.0 * margin_mm),
        w = pf(width_mm),
        h = pf(height_mm),
        pages = pages_html
    )
}

// ─── Sequential (термопринтер) ──────────────────────────────────────────────

pub(crate) fn render_labels_sequential(
    template: &str,
    products: &[Value],
    settings: &Value,
) -> String {
    if products.is_empty() || template.is_empty() {
        return empty_html("label");
    }
    let width_mm = settings
        .get("width_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(58.0);
    let height_mm = settings
        .get("height_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(40.0);
    let gap_mm = settings
        .get("gap_mm")
        .and_then(|v| v.as_f64())
        .unwrap_or(2.0);
    let fields: Option<Vec<String>> = settings.get("fields").and_then(|v| {
        v.as_array().map(|a| {
            a.iter()
                .filter_map(|f| f.as_str().map(|s| s.to_string()))
                .collect()
        })
    });
    let enabled_fields = fields.map(|f| f.into_iter().collect::<std::collections::HashSet<_>>());
    let print_mode = settings
        .get("print_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("escpos")
        .to_string();
    let effective_width = if print_mode == "system" {
        width_mm
    } else {
        width_mm.min(48.0)
    };
    let expanded = expand_products(products);
    let total_labels = expanded.len() as i64;
    let extra_context = json!({
        "width": pf(effective_width),
        "height": pf(height_mm),
        "barcode_type": settings.get("barcode_type").and_then(|v| v.as_str()).unwrap_or("code128"),
        "barcode_height_mm": pf(settings.get("barcode_height_mm").and_then(|v| v.as_f64()).unwrap_or(12.0)),
    });
    let mut labels_html = String::new();
    for (i, mut product) in expanded.into_iter().enumerate() {
        product["created_date"] = json!(chrono::Local::now().format("%d.%m.%Y").to_string());
        let rendered = render_single(template, &product, enabled_fields.as_ref(), &extra_context);
        let mut label = build_label_html(&rendered, effective_width, height_mm, gap_mm);
        if (i as i64) < total_labels - 1 {
            label.push_str("\n<div style=\"page-break-after: always;\"></div>");
        }
        labels_html.push_str(&label);
    }
    format!(
        "<!DOCTYPE html>\n<html lang=\"uk\">\n<head>\n<meta charset=\"UTF-8\">\n<title>Етикетки термопринтер</title>\n\
         <style>\n/* @page = ефективний розмір етикетки:\n   - 'system' (CUPS): повна ширина width_mm × height_mm (напр. 104×40мм\n     для Xprinter XP-420B) — контент заповнює всю етикетку.\n   - 'escpos' (58мм термо): 48×40мм — html2canvas знімає СТОРІНКУ\n     цілком, тому і сторінка, і .label-item мають бути 48×40мм — інакше\n     Rust масштабує canvas 58×40 (1.45) у 384×320 (1.2) нерівномірно\n     → спотворення. */\n\
         @page {{ size: {ew}mm {h}mm; margin: 0mm; }}\n\
         * {{ margin: 0; padding: 0; box-sizing: border-box; }}\n\
         body {{ font-family: Arial, Helvetica, sans-serif; font-size: 7pt; line-height: 1.15; }}\n\
         .label-item {{\n    display: flex; flex-direction: column; justify-content: center;\n    align-items: center; text-align: center;\n}}\n\
         @media print {{ .label-item {{ page-break-after: always; }} }}\n\
         </style>\n</head>\n<body>\n{labels}\n</body>\n</html>",
        ew = pf(effective_width),
        h = pf(height_mm),
        labels = labels_html
    )
}

/// Python _build_label_html.
fn build_label_html(rendered: &str, width_mm: f64, height_mm: f64, gap_mm: f64) -> String {
    format!(
        "<div class=\"label-item\" style=\"width: {w}mm; height: {h}mm; margin-bottom: {g}mm; \
         box-sizing: border-box; overflow: hidden; font-family: Arial, sans-serif;\">{rendered}</div>",
        w = pf(width_mm),
        h = pf(height_mm),
        g = pf(gap_mm)
    )
}

/// Python _empty_html.
fn empty_html(page_type: &str) -> String {
    if page_type == "label" {
        "<!DOCTYPE html><html lang=\"uk\"><head><meta charset=\"UTF-8\">\
         <title>Немає етикеток</title></head>\
         <body style=\"font-family: Arial; text-align: center; padding: 20mm;\">\
         <p>Немає товарів для друку</p></body></html>"
            .to_string()
    } else {
        "<!DOCTYPE html><html lang=\"uk\"><head><meta charset=\"UTF-8\">\
         <title>Немає цінників</title></head>\
         <body style=\"font-family: Arial; text-align: center; padding: 20mm;\">\
         <p>Немає товарів для друку цінників</p></body></html>"
            .to_string()
    }
}

// ─── SVG штрих-коду ─────────────────────────────────────────────────────────

/// Python _generate_barcode_svg (Code128) / _generate_qr_svg.
/// Структура SVG відрізняється від python-barcode (різні генератори);
/// параметри (module_width=0.25, quiet_zone=1.0, без тексту) — ті самі.
fn generate_barcode_svg(barcode_text: &str, height_mm: f64, barcode_type: &str) -> String {
    if barcode_text.trim().is_empty() {
        return String::new();
    }
    let btype = barcode_type.to_lowercase();
    if btype == "qr" {
        return generate_qr_svg(barcode_text, height_mm);
    }
    // Code128: спрощений, але валідний SVG (чередування чорних/білих модулів).
    let bytes = code128_bytes(barcode_text);
    let mut bars: Vec<(f64, f64)> = Vec::new(); // (x, width)
    let mut x: f64 = 0.0;
    let mut is_bar = true;
    for &b in &bytes {
        if is_bar {
            bars.push((x, b as f64 * 0.25));
        }
        x += b as f64 * 0.25;
        is_bar = !is_bar;
    }
    let total_w = x + 2.0;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" \
         style=\"max-width: 100%; height: auto;\" width=\"{w}\" height=\"{h}\">",
        w = pf(total_w),
        h = pf(height_mm)
    );
    for (bx, bw) in bars {
        svg.push_str(&format!(
            "<rect x=\"{}\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"black\"/>",
            pf(bx + 1.0),
            pf(bw),
            pf(height_mm)
        ));
    }
    svg.push_str("</svg>");
    let display = if barcode_text.len() > 20 {
        let mut t: String = barcode_text.chars().take(20).collect();
        t.push('…');
        t
    } else {
        barcode_text.to_string()
    };
    format!(
        "<div style=\"display: flex; flex-direction: column; align-items: center;\">{svg}\
         <span style=\"font-family: monospace; font-size: 9px; font-weight: bold; color: #000; \
         margin-top: 1px; letter-spacing: 0.5px;\">{}</span></div>",
        esc(&display)
    )
}

/// Мінімальний Code128 (кодування ASCII → послідовність ширин модулів).
fn code128_bytes(text: &str) -> Vec<u8> {
    // Таблиця Code128 (pattern: 6 елементів, ширина в модулях; 1=bar, 0=space).
    const PATTERNS: [[u8; 6]; 106] = [
        [2, 1, 2, 2, 2, 2],
        [2, 2, 2, 1, 2, 2],
        [2, 2, 2, 2, 2, 1],
        [1, 2, 1, 2, 2, 3],
        [1, 2, 1, 3, 2, 2],
        [1, 3, 1, 2, 2, 2],
        [1, 2, 2, 2, 1, 3],
        [1, 2, 2, 3, 1, 2],
        [1, 3, 2, 2, 1, 2],
        [2, 2, 1, 2, 1, 3],
        [2, 2, 1, 3, 1, 2],
        [2, 3, 1, 2, 1, 2],
        [1, 1, 2, 2, 3, 2],
        [1, 2, 2, 1, 3, 2],
        [1, 2, 2, 2, 3, 1],
        [1, 1, 3, 2, 2, 2],
        [1, 2, 3, 1, 2, 2],
        [1, 2, 3, 2, 2, 1],
        [2, 2, 3, 2, 1, 1],
        [2, 2, 1, 1, 3, 2],
        [2, 2, 1, 2, 3, 1],
        [2, 1, 3, 2, 1, 2],
        [2, 2, 3, 1, 1, 2],
        [3, 1, 2, 1, 3, 1],
        [3, 1, 1, 2, 2, 2],
        [3, 2, 1, 1, 2, 2],
        [3, 2, 1, 2, 2, 1],
        [3, 1, 2, 2, 1, 2],
        [3, 2, 2, 1, 1, 2],
        [3, 2, 2, 2, 1, 1],
        [2, 1, 2, 1, 2, 3],
        [2, 1, 2, 3, 2, 1],
        [2, 3, 2, 1, 2, 1],
        [1, 1, 1, 3, 2, 3],
        [1, 3, 1, 1, 2, 3],
        [1, 3, 1, 3, 2, 1],
        [1, 1, 2, 3, 1, 3],
        [1, 3, 2, 1, 1, 3],
        [1, 3, 2, 3, 1, 1],
        [2, 1, 1, 3, 1, 3],
        [2, 3, 1, 1, 1, 3],
        [2, 3, 1, 3, 1, 1],
        [1, 1, 2, 1, 3, 3],
        [1, 1, 2, 3, 3, 1],
        [1, 3, 2, 1, 3, 1],
        [1, 1, 3, 1, 2, 3],
        [1, 1, 3, 3, 2, 1],
        [1, 3, 3, 1, 2, 1],
        [3, 1, 3, 1, 2, 1],
        [2, 1, 1, 3, 3, 1],
        [2, 3, 1, 1, 3, 1],
        [2, 1, 3, 1, 1, 3],
        [2, 1, 3, 3, 1, 1],
        [2, 1, 3, 1, 3, 1],
        [3, 1, 1, 1, 2, 3],
        [3, 1, 1, 3, 2, 1],
        [3, 3, 1, 1, 2, 1],
        [3, 1, 2, 1, 1, 3],
        [3, 1, 2, 3, 1, 1],
        [3, 3, 2, 1, 1, 1],
        [3, 1, 4, 1, 1, 1],
        [2, 2, 1, 4, 1, 1],
        [4, 3, 1, 1, 1, 1],
        [1, 1, 1, 2, 2, 4],
        [1, 1, 1, 4, 2, 2],
        [1, 2, 1, 1, 2, 4],
        [1, 2, 1, 4, 2, 1],
        [1, 4, 1, 1, 2, 2],
        [1, 4, 1, 2, 2, 1],
        [1, 1, 2, 2, 1, 4],
        [1, 1, 2, 4, 1, 2],
        [1, 2, 2, 1, 1, 4],
        [1, 2, 2, 4, 1, 1],
        [1, 4, 2, 1, 1, 2],
        [1, 4, 2, 2, 1, 1],
        [2, 4, 1, 2, 1, 1],
        [2, 2, 1, 1, 1, 4],
        [4, 1, 3, 1, 1, 1],
        [2, 4, 1, 1, 1, 2],
        [1, 3, 4, 1, 1, 1],
        [1, 1, 1, 2, 4, 2],
        [1, 2, 1, 1, 4, 2],
        [1, 2, 1, 2, 4, 1],
        [1, 1, 4, 2, 1, 2],
        [1, 2, 4, 1, 1, 2],
        [1, 2, 4, 2, 1, 1],
        [4, 1, 1, 2, 1, 2],
        [4, 2, 1, 1, 1, 2],
        [4, 2, 1, 2, 1, 1],
        [2, 1, 2, 1, 4, 1],
        [2, 1, 4, 1, 2, 1],
        [4, 1, 2, 1, 2, 1],
        [1, 1, 1, 1, 4, 3],
        [1, 1, 1, 3, 4, 1],
        [1, 3, 1, 1, 4, 1],
        [1, 1, 4, 1, 1, 3],
        [1, 1, 4, 3, 1, 1],
        [4, 1, 1, 1, 1, 3],
        [4, 1, 1, 3, 1, 1],
        [1, 1, 3, 1, 4, 1],
        [1, 1, 4, 1, 3, 1],
        [3, 1, 1, 1, 4, 1],
        [4, 1, 1, 1, 3, 1],
        [2, 1, 1, 4, 1, 2],
        [2, 1, 1, 2, 1, 4],
        [2, 1, 1, 2, 3, 2],
    ];
    // Кодуємо (Start B + data + checksum + Stop).
    let bytes: Vec<u8> = text.as_bytes().to_vec();
    let mut codes: Vec<usize> = Vec::with_capacity(bytes.len() + 3);
    codes.push(104); // Start B
    let mut sum: usize = 104;
    for (i, &b) in bytes.iter().enumerate() {
        let code = (b - 32) as usize; // Code B: ASCII 32..127 → 0..95
        codes.push(code);
        sum += code * (i + 1);
    }
    codes.push(sum % 103);
    // Stop (106) НЕ додається в codes: PATTERNS має 106 елементів (0..105),
    // stop pattern обробляється окремо нижче (7 модулів з додатковою смугою).
    let mut out: Vec<u8> = Vec::new();
    for c in &codes {
        out.extend_from_slice(&PATTERNS[*c]);
    }
    out.extend_from_slice(&[2, 3, 3, 1, 1, 1, 2]);
    out
}

/// Python _generate_qr_svg (реальний QR: error correction M, border=1,
/// квадрат зі стороною box_size_mm, підпис цифрами внизу).
/// Fallback-стиль [QR: ...] — лише при помилці кодування (дані завеликі).
fn generate_qr_svg(data: &str, box_size_mm: f64) -> String {
    if data.trim().is_empty() {
        return String::new();
    }
    let qr = match qrcode::QrCode::with_error_correction_level(data.as_bytes(), qrcode::EcLevel::M) {
        Ok(q) => q,
        Err(_) => {
            // Fallback: як Python при помилці генерації (екранований текст).
            return format!(
                "<span style=\"font-family: monospace; font-size: 10px;\">[QR: {}]</span>",
                esc(data)
            );
        }
    };
    let n = qr.width();
    let border = 1usize; // quiet zone, як Python border=1
    let total = n + 2 * border;
    let colors = qr.to_colors();
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {t} {t}\" style=\"width: {w}mm; height: {w}mm; max-width: 100%; height: auto;\">",
        t = total,
        w = pf(box_size_mm)
    );
    svg.push_str("<rect x=\"0\" y=\"0\" width=\"100%\" height=\"100%\" fill=\"white\"/>");
    // Run-length: послідовні темні модулі рядка → один <rect> (як SvgPathImage).
    for y in 0..n {
        let row = y * n;
        let mut x = 0usize;
        while x < n {
            if colors[row + x] == qrcode::Color::Dark {
                let start = x;
                while x < n && colors[row + x] == qrcode::Color::Dark {
                    x += 1;
                }
                svg.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"1\" fill=\"black\"/>",
                    start + border,
                    y + border,
                    x - start
                ));
            } else {
                x += 1;
            }
        }
    }
    svg.push_str("</svg>");
    // Підпис цифрами (як code128: до 20 символів + …), екранований.
    let display = if data.len() > 20 {
        let mut t: String = data.chars().take(20).collect();
        t.push('…');
        t
    } else {
        data.to_string()
    };
    format!(
        "<div style=\"display: flex; flex-direction: column; align-items: center;\">{svg}\
         <span style=\"font-family: monospace; font-size: 9px; font-weight: bold; color: #000; \
         margin-top: 1px; letter-spacing: 0.5px;\">{}</span></div>",
        esc(&display)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_svg_is_real_svg_not_stub() {
        let out = generate_qr_svg("TEST123", 12.0);
        assert!(out.contains("<svg"), "має бути SVG, отримано: {out}");
        assert!(!out.contains("[QR:"), "не має бути заглушки, отримано: {out}");
        assert!(out.contains("viewBox"), "viewBox відсутній");
        assert!(out.contains("fill=\"black\""), "чорні модулі відсутні");
        assert!(out.contains("TEST123"), "підпис цифрами відсутній");
    }

    #[test]
    fn qr_svg_empty_data_returns_empty() {
        assert_eq!(generate_qr_svg("", 12.0), "");
        assert_eq!(generate_qr_svg("   ", 12.0), "");
        assert_eq!(generate_qr_svg("\n\t ", 12.0), "");
    }

    #[test]
    fn qr_svg_square_and_scaled_to_mm() {
        let out = generate_qr_svg("https://example.com/item/42", 30.0);
        assert!(out.contains("width: 30.0mm; height: 30.0mm"), "масштаб мм: {out}");
        // viewBox має бути квадратним (total = n + 2)
        let vb = out.split("viewBox=\"").nth(1).and_then(|s| s.split('\"').next()).unwrap_or("");
        let dims: Vec<&str> = vb.split(' ').collect();
        assert_eq!(dims.len(), 4, "viewBox: {vb}");
        assert_eq!(dims[2], dims[3], "viewBox має бути квадратним: {vb}");
    }

    #[test]
    fn qr_svg_escapes_caption() {
        let out = generate_qr_svg("<script>alert(1)</script>", 12.0);
        assert!(!out.contains("<script>"), "XSS: підпис не екрановано");
        assert!(out.contains("&lt;script&gt;"), "екранування відсутнє");
    }
}
