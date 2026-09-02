// ─────────────────────────────────────────────────────────────────────────────
// router_v1 — маршрути /api/v1 (Strangler Fig, етап 1)
// ─────────────────────────────────────────────────────────────────────────────
// Зараз:
//   GET /api/v1/health            → нативний Rust-хендлер (200 {"status":"ok"})
//   активні роути (0 CRIT, 0 ALIAS) → Rust-гілки ПІД feature-flag
//     TORGASHKA_RUST_*=1 (дефолт у Tauri: 1; інакше шляхи йдуть у fallback → 410)
//   все інше (LEGACY)             → fallback → 410 Gone (Python дезактивовано)
// JWT-валідація — на весь роутер (middleware), /health пропускається всередині.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    http::{header, HeaderName, HeaderValue, Method},
    middleware,
    routing::{delete, get, post, put},
    Router,
};

use tower_http::cors::CorsLayer;

use crate::{
    auth, auth_routes, categories_v2, crud, debtors, documents, invoices, ledger, ocr, pos,
    print_templates, products_v2, proxy, prro, purchase_orders, readdirs, return_invoices, setup,
    store_context, stores, suppliers, sync, AppState,
};

/// Збирає роутер v1 зі станом.
pub fn build_router(state: AppState) -> Router {
    // CORS-шар (GUI Tauri webview: tauri://localhost → http://127.0.0.1:8000).
    // Найзовніший шар: preflight OPTIONS обробляється до auth_middleware.
    let cors = CorsLayer::new()
        .allow_origin([
            HeaderValue::from_static("tauri://localhost"),
            HeaderValue::from_static("http://tauri.localhost"),
            HeaderValue::from_static("http://localhost:5173"),
            HeaderValue::from_static("http://127.0.0.1:5173"),
            HeaderValue::from_static("http://localhost:8000"),
            HeaderValue::from_static("http://127.0.0.1:8000"),
        ])
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
            Method::PATCH,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::ORIGIN,
            HeaderName::from_static("x-requested-with"),
            HeaderName::from_static("x-store-id"),
        ])
        .allow_credentials(true);

    let mut router = Router::new().route("/api/v1/health", get(health));

    // Rust-гілка довідників — лише коли feature-flag увімкнено (readdirs Some).
    // Інакше ці шляхи потрапляють у fallback → 410 (дезактивація).
    if state.readdirs.is_some() {
        router = router
            .route("/api/v1/products", get(readdirs::list_products))
            .route("/api/v1/categories", get(readdirs::list_categories))
            .route("/api/v1/suppliers", get(readdirs::list_suppliers));
    }

    // Rust-гілка категорій v2 (дезактивація Python, CRIT) — під TORGASHKA_RUST_READDIRS.
    // Порядок: /tree ПЕРЕД /:category_id — як FastAPI.
    if state.readdirs.is_some() {
        router = router
            .route(
                "/api/v2/categories",
                get(categories_v2::list).post(categories_v2::create),
            )
            .route("/api/v2/categories/tree", get(categories_v2::tree))
            .route(
                "/api/v2/categories/:category_id",
                get(categories_v2::get)
                    .put(categories_v2::update)
                    .delete(categories_v2::delete),
            );
    }

    // Rust-гілка CRUD (етап 2) — під тим самим feature-flag.
    // Порядок: статичні сегменти (barcode, all, counts) ПЕРЕД :id.
    if state.readdirs.is_some() {
        router = router
            // Products
            .route(
                "/api/v1/products/barcode/:barcode",
                get(crud::get_product_by_barcode),
            )
            .route(
                "/api/v1/products/:id",
                get(crud::get_product)
                    .put(crud::update_product)
                    .delete(crud::delete_product),
            )
            .route("/api/v1/products", post(crud::create_product))
            // Categories
            .route(
                "/api/v1/categories/:id",
                get(crud::get_category)
                    .put(crud::update_category)
                    .delete(crud::delete_category),
            )
            .route("/api/v1/categories", post(crud::create_category))
            // Suppliers
            .route("/api/v1/suppliers/all", get(crud::list_all_suppliers))
            .route(
                "/api/v1/suppliers/:id",
                get(crud::get_supplier)
                    .put(crud::update_supplier)
                    .delete(crud::delete_supplier),
            )
            .route("/api/v1/suppliers", post(crud::create_supplier))
            // Inventory
            .route(
                "/api/v1/inventory",
                get(crud::list_inventories).post(crud::create_inventory),
            )
            .route("/api/v1/inventory/counts", get(crud::inventory_counts))
            .route(
                "/api/v1/inventory/:id",
                get(crud::get_inventory)
                    .put(crud::update_inventory)
                    .delete(crud::delete_inventory),
            )
            .route(
                "/api/v1/inventory/:id/confirm",
                post(crud::confirm_inventory),
            );
    }

    // Rust-гілка товарів постачальника та руху (дезактивація Python, CRIT)
    // — під тим самим feature-flag. Статичні сегменти ПЕРЕД /:id (як FastAPI).
    if state.readdirs.is_some() {
        router = router
            .route(
                "/api/v1/suppliers/:supplier_id/products/:product_id/movements",
                get(suppliers::movements),
            )
            .route(
                "/api/v1/suppliers/:supplier_id/products",
                get(suppliers::products),
            );
    }

    // Rust-гілка sync (ЕТАП 3 offline-first) — pull майстер-даних.
    if state.readdirs.is_some() {
        router = router.route("/api/v1/sync/master", get(sync::master));
    }
    // Rust-гілка sync push (ЕТАП 4 offline-first) — каса → сервер. Потребує
    // POS-гілки (створення чеків з client_uuid) + sync_meta/sync_log (0011).
    if state.pos.is_some() {
        router = router.route("/api/v1/sync/push", post(sync::push));
    }

    // Rust-гілка ledger (етап 4) — під тим самим feature-flag.
    if state.ledger.is_some() {
        // v1 (статичний /balance ПЕРЕД /:supplier_id — як FastAPI).
        router = router
            .route(
                "/api/v1/ledger/balance/:supplier_id",
                get(ledger::balance_v1),
            )
            .route("/api/v1/ledger/:supplier_id", get(ledger::history_v1))
            .route("/api/v1/ledger", post(ledger::create_entry_v1))
            // v2 (entries/balances ПЕРЕД balance/:supplier_id).
            .route("/api/v2/ledger/entries", get(ledger::list_entries_v2))
            .route("/api/v2/ledger/entries", post(ledger::create_entry_v2))
            .route("/api/v2/ledger/balances", get(ledger::all_balances_v2))
            .route(
                "/api/v2/ledger/balance/:supplier_id",
                get(ledger::balance_v2),
            );
    }

    // Rust-гілка POS (етап 3) — під тим самим feature-flag.
    if state.pos.is_some() {
        // Чеки v2 (статичні сегменти ПЕРЕД :id).
        router = router
            .route("/api/v2/receipts/stats/today", get(pos::today_stats))
            .route("/api/v2/receipts/search", get(pos::search_receipts))
            .route(
                "/api/v2/receipts/by-product/:query/recent-sales",
                get(pos::recent_sales),
            )
            .route(
                "/api/v2/receipts/products/:product_id/returnable-quantity",
                get(pos::returnable_quantity),
            )
            .route("/api/v2/receipts", get(pos::list_receipts))
            .route("/api/v1/receipts", post(pos::create_receipt_v1))
            .route("/api/v2/receipts/sale", post(pos::create_sale))
            .route("/api/v2/receipts/return", post(pos::create_return))
            .route(
                "/api/v2/receipts/:receipt_id/items",
                get(pos::receipt_items),
            )
            .route("/api/v2/receipts/:receipt_id", get(pos::get_receipt))
            // v1 ALIAS (1:1 Python deprecated): list/get/items/recent — свої
            // хендлери; search/stats/returnable — ті самі v2 (формат той самий).
            .route("/api/v1/receipts/stats/today", get(pos::today_stats))
            .route("/api/v1/receipts/search", get(pos::search_receipts_v1))
            .route(
                "/api/v1/receipts/by-product/:query/recent-sales",
                get(pos::recent_sales_v1),
            )
            .route(
                "/api/v1/receipts/products/:product_id/returnable-quantity",
                get(pos::returnable_quantity),
            )
            .route("/api/v1/receipts", get(pos::list_receipts_v1))
            .route(
                "/api/v1/receipts/:receipt_id/items",
                get(pos::receipt_items_v1),
            )
            .route("/api/v1/receipts/:receipt_id", get(pos::get_receipt_v1))
            // Робочі сесії
            .route("/api/v1/work-sessions/my", get(pos::my_sessions))
            .route("/api/v1/work-sessions/report", get(pos::work_report))
            .route(
                "/api/v1/work-sessions/user/:user_id",
                get(pos::user_sessions),
            )
            // Списання
            .route(
                "/api/v1/write-offs",
                get(pos::list_write_offs).post(pos::create_write_off),
            )
            .route(
                "/api/v1/write-offs/:id",
                get(pos::get_write_off)
                    .put(pos::update_write_off)
                    .delete(pos::delete_write_off),
            )
            .route(
                "/api/v1/write-offs/:id/confirm",
                post(pos::confirm_write_off),
            )
            // Довідник причин списання
            .route(
                "/api/v1/write-off-reasons",
                get(pos::list_write_off_reasons).post(pos::create_write_off_reason),
            )
            // Переміщення
            .route(
                "/api/v1/transfers",
                get(pos::list_transfers).post(pos::create_transfer),
            )
            .route(
                "/api/v1/transfers/:id",
                get(pos::get_transfer)
                    .put(pos::update_transfer)
                    .delete(pos::delete_transfer),
            )
            .route("/api/v1/transfers/:id/confirm", post(pos::confirm_transfer))
            // Готівкові операції (внесення/інкасація)
            .route(
                "/api/v1/cash-operations",
                get(pos::list_cash_operations).post(pos::create_cash_operation),
            )
            // Зміни ПРРО (X/Z) — локальні (етап 3)
            .route("/api/v2/prro/shifts", get(pos::list_shifts))
            .route("/api/v2/prro/shift/open", post(pos::open_shift))
            .route("/api/v2/prro/shift/close", post(pos::close_shift));
    }

    // Rust-гілка ФІСКАЛЬНОГО ПРРО (етап 7.3) — TORGASHKA_RUST_PRRO=1|shadow.
    // Окремий префікс /fiscal/* — не конфліктує з локальними X/Z (pos.rs).
    if state.prro.is_some() {
        router = router
            .route("/api/v2/prro/fiscal/shift/open", post(prro::open_shift))
            .route("/api/v2/prro/fiscal/shift/close", post(prro::close_shift))
            .route("/api/v2/prro/fiscal/shifts", get(prro::list_shifts))
            .route("/api/v2/prro/fiscal/sync", post(prro::sync_queue))
            .route("/api/v2/prro/fiscal/queue", get(prro::queue))
            .route("/api/v2/prro/fiscal/status", get(prro::status))
            // v2 ALIAS без /fiscal — реальні шляхи фронтенду (prroService).
            .route("/api/v2/prro/sync", post(prro::sync_queue))
            .route("/api/v2/prro/queue", get(prro::queue))
            .route("/api/v2/prro/status", get(prro::status));
    }

    // Rust-гілка ПРРО v2 (група 8/9) — TORGASHKA_RUST_PRRO_V2=1: settings +
    // test-connection + fiscalize під ОРИГІНАЛЬНИМИ URL Python (1:1 parity).
    if state.prro.is_some()
        && matches!(
            std::env::var(crate::RUST_PRRO_V2_ENV)
                .unwrap_or_default()
                .trim()
                .to_lowercase()
                .as_str(),
            "1" | "true"
        )
    {
        router = router
            .route(
                "/api/v2/prro/settings",
                get(prro::settings_get).put(prro::settings_put),
            )
            .route("/api/v2/prro/test-connection", post(prro::test_connection))
            .route(
                "/api/v2/prro/receipts/:receipt_id/fiscalize",
                post(prro::fiscalize_receipt),
            );
    }

    // Rust-гілка OCR (група 9/9) — TORGASHKA_RUST_OCR=1: /api/v1/ocr/invoice +
    // /api/v1/invoice-ocr/analyze під ОРИГІНАЛЬНИМИ URL Python (1:1 parity).
    if state.ocr.is_some() {
        router = router
            .route("/api/v1/ocr/invoice", post(ocr::analyze_invoice))
            .route(
                "/api/v1/invoice-ocr/analyze",
                post(ocr::analyze_with_matching),
            );
    }

    // Setup (Частина 1+2): перший власник + персональна БД — ПУБЛІЧНІ шляхи
    // (без JWT; у auth.rs/store_context.rs is_public_path додано /api/v1/setup).
    if state.setup.is_some() {
        router = router
            .route("/api/v1/setup/status", get(setup::status))
            .route("/api/v1/setup", post(setup::setup));
    }

    // Rust-гілка auth/users/settings/RBAC (етап 6) — під TORGASHKA_RUST_AUTH=1.
    // Порядок: статичні сегменти (users-list, users/me, permissions/list)
    // ПЕРЕД :user_id — як FastAPI.
    if state.auth.is_some() {
        router = router
            // Auth (публічні + JWT).
            .route("/api/v1/auth/login", post(auth_routes::login))
            .route("/api/v1/auth/login-pin", post(auth_routes::login_pin))
            .route("/api/v1/auth/refresh", post(auth_routes::refresh))
            .route("/api/v1/auth/logout", post(auth_routes::logout))
            .route("/api/v1/auth/verify", get(auth_routes::verify))
            .route("/api/v1/auth/users-list", get(auth_routes::users_list))
            // /auth/me + /auth/users/me (JWT) — поточний користувач.
            // Статичні сегменти ПЕРЕД :user_id; /auth/me — фронтенд (useAuth),
            // users/me — сумісність із коментарем і Python-еталоном.
            .route("/api/v1/auth/me", get(auth_routes::me))
            .route("/api/v1/auth/users/me", get(auth_routes::me))
            // Users (всі require_admin).
            .route(
                "/api/v1/users/permissions/list",
                get(auth_routes::permissions_list),
            )
            .route(
                "/api/v1/users/:user_id/permissions",
                put(auth_routes::update_permissions),
            )
            .route(
                "/api/v1/users/:user_id/hourly-rate",
                put(auth_routes::update_hourly_rate),
            )
            .route(
                "/api/v1/users/:user_id",
                get(auth_routes::get_user)
                    .put(auth_routes::update_user)
                    .delete(auth_routes::delete_user),
            )
            .route(
                "/api/v1/users",
                get(auth_routes::list_users).post(auth_routes::create_user),
            )
            // Settings.
            .route(
                "/api/v1/settings",
                get(auth_routes::settings_all).put(auth_routes::settings_batch_update),
            )
            .route(
                "/api/v1/settings/:name",
                get(auth_routes::settings_by_module),
            )
            .route(
                "/api/v1/settings/:name",
                put(auth_routes::settings_update_key),
            );
    }

    // Rust-гілка боржників (етап 8, група 1) — TORGASHKA_RUST_DEBTORS=1.
    // Порядок: статичні сегменти (search) ПЕРЕД :debtor_id — як FastAPI.
    if state.debtors.is_some() {
        router = router
            .route("/api/v1/debtors/search", get(debtors::search))
            .route("/api/v1/debtors/:debtor_id/pay", post(debtors::pay))
            .route(
                "/api/v1/debtors/:debtor_id",
                get(debtors::get).put(debtors::update).post(debtors::pay),
            )
            .route("/api/v1/debtors", get(debtors::list).post(debtors::create))
            .route(
                "/api/v1/debtors/:debtor_id/receipts",
                get(debtors::receipts),
            )
            .route(
                "/api/v1/debtors/:debtor_id/payments",
                get(debtors::payments),
            );
    }

    // Rust-гілка документів (етап 8, група 2) — TORGASHKA_RUST_DOCUMENTS=1.
    // Порядок: статичні сегменти (batch-confirm, export) ПЕРЕД :document_id — як FastAPI.
    if state.documents.is_some() {
        router = router
            .route(
                "/api/v1/documents/batch-confirm",
                post(documents::batch_confirm),
            )
            .route("/api/v1/documents/export", get(documents::export))
            .route("/api/v1/documents/:document_id/copy", post(documents::copy))
            .route(
                "/api/v1/documents/:document_id/print",
                get(documents::print),
            )
            .route("/api/v1/documents/:document_id", delete(documents::delete))
            .route("/api/v1/documents", get(documents::list));
    }

    // Rust-гілка інвойсів (етап 8, група 3) — TORGASHKA_RUST_INVOICES=1 (v1+v2).
    // Порядок: статичний /confirm ПЕРЕД :invoice_id — як FastAPI.
    if state.invoices_v1.is_some() || state.invoices_v2.is_some() {
        router = router
            .route(
                "/api/v1/invoices",
                get(invoices::v1_list).post(invoices::v1_create),
            )
            .route(
                "/api/v1/invoices/:invoice_id",
                get(invoices::v1_get)
                    .put(invoices::v1_update)
                    .delete(invoices::v1_delete),
            )
            .route(
                "/api/v1/invoices/:invoice_id/payment-info",
                get(invoices::v1_payment_info),
            )
            .route(
                "/api/v1/invoices/:invoice_id/confirm",
                post(invoices::v1_confirm),
            )
            .route(
                "/api/v1/invoices/:invoice_id/price-changes",
                get(invoices::v1_price_changes),
            )
            .route(
                "/api/v1/invoices/:invoice_id/print-items",
                post(invoices::v1_print_items),
            )
            .route(
                "/api/v2/invoices",
                get(invoices::v2_list).post(invoices::v2_create),
            )
            .route("/api/v2/invoices/confirm", post(invoices::v2_confirm))
            .route(
                "/api/v2/invoices/:invoice_id",
                get(invoices::v2_get)
                    .put(invoices::v2_update)
                    .delete(invoices::v2_delete),
            )
            .route(
                "/api/v2/invoices/:invoice_id/payment-info",
                get(invoices::v2_payment_info),
            )
            .route(
                "/api/v2/invoices/:invoice_id/price-changes",
                get(invoices::v2_price_changes),
            )
            .route(
                "/api/v2/invoices/:invoice_id/print-items",
                post(invoices::v2_print_items),
            )
            .route(
                "/api/v2/invoices/:invoice_id/cancel",
                post(invoices::v2_cancel),
            );
    }

    // Повернення постачальнику (етап 8, група 4) — TORGASHKA_RUST_RETURN_INVOICES=1.
    if state.return_invoices.is_some() {
        router = router.merge(return_invoices::router());
    }

    // Замовлення постачальнику (етап 8, група 5) — TORGASHKA_RUST_PURCHASE_ORDERS=1.
    if state.purchase_orders.is_some() {
        router = router
            .route(
                "/api/v1/purchase-orders",
                get(purchase_orders::list).post(purchase_orders::create),
            )
            .route(
                "/api/v1/purchase-orders/:order_id",
                get(purchase_orders::get)
                    .put(purchase_orders::update)
                    .delete(purchase_orders::delete),
            )
            .route(
                "/api/v1/purchase-orders/:order_id/confirm",
                post(purchase_orders::confirm),
            );
    }

    if state.print_templates.is_some() {
        router = router
            .route(
                "/api/v1/print/price-tags/render",
                post(print_templates::price_tags_render),
            )
            .route(
                "/api/v1/print/labels/render",
                post(print_templates::labels_render),
            )
            .route("/api/v1/print/printers", get(print_templates::printers))
            .route("/api/v1/print/test", post(print_templates::test_print))
            .route(
                "/api/v1/print-templates",
                get(print_templates::list_templates).post(print_templates::create_template),
            )
            .route(
                "/api/v1/print-templates/all",
                get(print_templates::list_all),
            )
            .route(
                "/api/v1/print-templates/default",
                get(print_templates::get_default),
            )
            .route(
                "/api/v1/print-templates/:template_id",
                get(print_templates::get_template)
                    .put(print_templates::update_template)
                    .delete(print_templates::delete_template),
            )
            .route(
                "/api/v1/print-templates/:template_id/set-default",
                post(print_templates::set_default),
            )
            .route(
                "/api/v1/print-templates/:template_id/render",
                post(print_templates::render_template),
            );
    }

    // Товари v2 (етап 8, група 7) — TORGASHKA_RUST_PRODUCTS_V2=1.
    if state.products_v2.is_some() {
        router = router
            .route(
                "/api/v2/products",
                get(products_v2::list_products).post(products_v2::create_product),
            )
            .route(
                "/api/v2/products/barcode/:barcode",
                get(products_v2::get_by_barcode),
            )
            .route(
                "/api/v2/products/:product_id/images",
                post(products_v2::upload_image).layer(products_v2::upload_body_limit()),
            )
            .route(
                "/api/v2/products/:product_id/images/:image_id",
                delete(products_v2::delete_image),
            )
            .route(
                "/api/v2/products/:product_id/barcodes",
                post(products_v2::add_barcode),
            )
            .route(
                "/api/v2/products/:product_id/barcodes/:barcode_id",
                delete(products_v2::delete_barcode),
            )
            // v1 ALIAS — ті самі хендлери (Python v1 deprecated).
            .route(
                "/api/v1/products/:product_id/barcodes",
                post(products_v2::add_barcode),
            )
            .route(
                "/api/v1/products/:product_id/barcodes/:barcode_id",
                delete(products_v2::delete_barcode),
            )
            .route(
                "/api/v1/products/:product_id/images",
                post(products_v2::upload_image).layer(products_v2::upload_body_limit()),
            )
            .route(
                "/api/v1/products/:product_id/images/:image_id",
                delete(products_v2::delete_image),
            )
            .route(
                "/api/v2/products/:product_id",
                get(products_v2::get_product)
                    .put(products_v2::update_product)
                    .delete(products_v2::delete_product),
            )
            // Static serve завантажених зображень (Python app.mount /uploads).
            .route(
                "/uploads/products/:product_id/:filename",
                get(products_v2::serve_upload),
            );
    }

    // Rust-гілка торговельних точок (Етап 3) — під тим самим feature-flag.
    // /api/v1/inventory/availability — статичний сегмент, пріоритет над :id.
    if state.stores.is_some() {
        router = router
            .route(
                "/api/v1/stores",
                get(stores::list_stores).post(stores::create_store),
            )
            .route("/api/v1/user-stores", post(stores::assign_user_store))
            .route("/api/v1/inventory/availability", get(stores::availability));
    }

    // Усе, що не health і не Rust-гілка, — у Python sidecar (метод/шлях/тіло/заголовки).
    // Порядок шарів: cors → auth (JWT) → store (X-Store-Id + RLS-контекст) → handler.
    // StoreContext ПІСЛЯ auth: Claims доступні в extensions; контекст точки
    // проставляється в task-local для всіх запитів хендлера (RLS set_config).
    router
        .fallback(proxy::proxy_handler)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            store_context::store_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(cors)
        .with_state(state)
}

/// GET /api/v1/health → 200 {"status":"ok"} (без JWT — відкритий).
pub async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(crate::health_payload())
}

// ─── Тести: /api/v1/auth/me та /api/v1/auth/users/me (регресія 410 Gone) ────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use std::sync::Arc;
    use torgashka_domain::{
        AuthError, AuthService, LoginPinRequest, LoginRequest, SettingDto, SettingsBatchInput,
        UserCreateInput, UserDto, UserListDto, UserUpdateInput,
    };
    use tower::ServiceExt;
    use uuid::Uuid;

    /// Мок auth-сервісу: живий тільки get_user_by_id (шлях /auth/me).
    struct MockAuth;

    #[async_trait::async_trait]
    impl AuthService for MockAuth {
        async fn login(
            &self,
            _input: &LoginRequest,
        ) -> Result<torgashka_domain::LoginResult, AuthError> {
            unimplemented!()
        }
        async fn login_pin(
            &self,
            _input: &LoginPinRequest,
        ) -> Result<torgashka_domain::LoginResult, AuthError> {
            unimplemented!()
        }
        async fn refresh(
            &self,
            _user_id: Uuid,
        ) -> Result<torgashka_domain::LoginResult, AuthError> {
            unimplemented!()
        }
        async fn logout(&self, _user_id: Uuid) -> Result<(), AuthError> {
            unimplemented!()
        }
        async fn get_user_by_id(&self, user_id: Uuid) -> Result<UserDto, AuthError> {
            Ok(UserDto {
                id: user_id,
                name: "Тест".into(),
                login: "test".into(),
                role: "admin".into(),
                is_active: true,
                onboarding_completed: true,
                permissions: Some(vec!["admin".into()]),
                created_at: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
                updated_at: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            })
        }
        async fn users_list_public(
            &self,
        ) -> Result<Vec<torgashka_domain::PublicUserDto>, AuthError> {
            unimplemented!()
        }
        async fn list_users(&self, _page: i64, _size: i64) -> Result<UserListDto, AuthError> {
            unimplemented!()
        }
        async fn create_user(&self, _input: &UserCreateInput) -> Result<UserDto, AuthError> {
            unimplemented!()
        }
        async fn update_user(
            &self,
            _user_id: Uuid,
            _input: &UserUpdateInput,
        ) -> Result<UserDto, AuthError> {
            unimplemented!()
        }
        async fn update_permissions(
            &self,
            _user_id: Uuid,
            _permissions: &[String],
        ) -> Result<UserDto, AuthError> {
            unimplemented!()
        }
        async fn update_hourly_rate(
            &self,
            _user_id: Uuid,
            _hourly_rate: f64,
        ) -> Result<serde_json::Value, AuthError> {
            unimplemented!()
        }
        async fn delete_user(
            &self,
            _user_id: Uuid,
            _current_user_id: Uuid,
        ) -> Result<(), AuthError> {
            unimplemented!()
        }
        async fn settings_all(&self) -> Result<torgashka_domain::SettingsModulesDto, AuthError> {
            unimplemented!()
        }
        async fn settings_by_module(&self, _module: &str) -> Result<Vec<SettingDto>, AuthError> {
            unimplemented!()
        }
        async fn settings_batch_update(
            &self,
            _settings: &[(String, Option<String>)],
        ) -> Result<torgashka_domain::SettingsModulesDto, AuthError> {
            unimplemented!()
        }
        async fn settings_update_key(
            &self,
            _key: &str,
            _value: Option<String>,
        ) -> Result<SettingDto, AuthError> {
            unimplemented!()
        }
    }

    fn test_state() -> AppState {
        let mut state = crate::AppState {
            jwt_secret: Arc::new("test-secret-для-юніт-тесту".to_string()),
            readdirs: None,
            write: None,
            write_pool: None,
            pos: None,
            ledger: None,
            auth: None,
            prro: None,
            debtors: None,
            documents: None,
            documents_pool: None,
            invoices_v1: None,
            invoices_v2: None,
            invoices_pool: None,
            return_invoices: None,
            return_invoices_pool: None,
            purchase_orders: None,
            purchase_orders_pool: None,
            print_templates: None,
            print_pool: None,
            products_v2: None,
            products_v2_pool: None,
            ocr: None,
            ocr_pool: None,
            uploads_dir: std::path::PathBuf::from("uploads"),
            store_pool: None,
            stores: None,
            setup: None,
        };
        state.auth = Some(Arc::new(MockAuth) as Arc<dyn AuthService + Send + Sync>);
        state
    }

    fn valid_token() -> String {
        crate::auth::create_access_token(
            "11111111-1111-1111-1111-111111111111",
            "admin",
            &["admin".to_string()],
            "test-secret-для-юніт-тесту",
        )
        .expect("токен має створитись")
    }

    async fn get(path: &str, token: Option<&str>) -> StatusCode {
        let app = build_router(test_state());
        let mut builder = Request::builder().method("GET").uri(path);
        if let Some(t) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let resp = app
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        resp.status()
    }

    #[tokio::test]
    async fn auth_me_with_valid_token_not_gone_and_200() {
        let status = get("/api/v1/auth/me", Some(&valid_token())).await;
        assert_ne!(
            status,
            StatusCode::GONE,
            "/api/v1/auth/me не має падати в fallback (410)"
        );
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_users_me_with_valid_token_not_gone_and_200() {
        let status = get("/api/v1/auth/users/me", Some(&valid_token())).await;
        assert_ne!(
            status,
            StatusCode::GONE,
            "/api/v1/auth/users/me не має падати в fallback (410)"
        );
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_me_without_token_is_401_not_410() {
        let status = get("/api/v1/auth/me", None).await;
        assert_ne!(status, StatusCode::GONE);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
}
