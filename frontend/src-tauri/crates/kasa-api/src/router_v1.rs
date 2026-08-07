// ─────────────────────────────────────────────────────────────────────────────
// router_v1 — маршрути /api/v1 (Strangler Fig, етап 1)
// ─────────────────────────────────────────────────────────────────────────────
// Зараз:
//   GET /api/v1/health            → нативний Rust-хендлер (200 {"status":"ok"})
//   GET /api/v1/products|categories|suppliers → Rust-гілка ПІД feature-flag
//     KASA_RUST_READDIRS=1 (інакше ці шляхи йдуть у fallback → Python :8001)
//   все інше                     → reverse proxy на Python sidecar :8001
// JWT-валідація — на весь роутер (middleware), /health пропускається всередині.
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    middleware,
    routing::{get, post},
    Router,
};

use crate::{auth, crud, pos, proxy, readdirs, AppState};

/// Збирає роутер v1 зі станом.
pub fn build_router(state: AppState) -> Router {
    let mut router = Router::new().route("/api/v1/health", get(health));

    // Rust-гілка довідників — лише коли feature-flag увімкнено (readdirs Some).
    // Інакше ці шляхи потрапляють у fallback → проксі на Python :8001.
    if state.readdirs.is_some() {
        router = router
            .route("/api/v1/products", get(readdirs::list_products))
            .route("/api/v1/categories", get(readdirs::list_categories))
            .route("/api/v1/suppliers", get(readdirs::list_suppliers));
    }

    // Rust-гілка CRUD (етап 2) — під тим самим feature-flag.
    // Порядок: статичні сегменти (barcode, all, counts) ПЕРЕД {id}.
    if state.readdirs.is_some() {
        router = router
            // Products
            .route(
                "/api/v1/products/barcode/{barcode}",
                get(crud::get_product_by_barcode),
            )
            .route(
                "/api/v1/products/{id}",
                get(crud::get_product)
                    .put(crud::update_product)
                    .delete(crud::delete_product),
            )
            .route("/api/v1/products", post(crud::create_product))
            // Categories
            .route(
                "/api/v1/categories/{id}",
                get(crud::get_category)
                    .put(crud::update_category)
                    .delete(crud::delete_category),
            )
            .route("/api/v1/categories", post(crud::create_category))
            // Suppliers
            .route("/api/v1/suppliers/all", get(crud::list_all_suppliers))
            .route(
                "/api/v1/suppliers/{id}",
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
                "/api/v1/inventory/{id}",
                get(crud::get_inventory)
                    .put(crud::update_inventory)
                    .delete(crud::delete_inventory),
            )
            .route(
                "/api/v1/inventory/{id}/confirm",
                post(crud::confirm_inventory),
            );
    }

    // Rust-гілка POS (етап 3) — під тим самим feature-flag.
    if state.pos.is_some() {
        // Чеки v2 (статичні сегменти ПЕРЕД {id}).
        router = router
            .route("/api/v2/receipts/stats/today", get(pos::today_stats))
            .route("/api/v2/receipts/search", get(pos::search_receipts))
            .route(
                "/api/v2/receipts/by-product/{query}/recent-sales",
                get(pos::recent_sales),
            )
            .route(
                "/api/v2/receipts/products/{product_id}/returnable-quantity",
                get(pos::returnable_quantity),
            )
            .route("/api/v2/receipts", get(pos::list_receipts))
            .route("/api/v2/receipts/sale", post(pos::create_sale))
            .route("/api/v2/receipts/return", post(pos::create_return))
            .route(
                "/api/v2/receipts/{receipt_id}/items",
                get(pos::receipt_items),
            )
            .route("/api/v2/receipts/{receipt_id}", get(pos::get_receipt))
            // Робочі сесії
            .route("/api/v1/work-sessions/my", get(pos::my_sessions))
            .route("/api/v1/work-sessions/report", get(pos::work_report))
            .route(
                "/api/v1/work-sessions/user/{user_id}",
                get(pos::user_sessions),
            )
            // Списання
            .route(
                "/api/v1/write-offs",
                get(pos::list_write_offs).post(pos::create_write_off),
            )
            .route(
                "/api/v1/write-offs/{id}",
                get(pos::get_write_off)
                    .put(pos::update_write_off)
                    .delete(pos::delete_write_off),
            )
            .route(
                "/api/v1/write-offs/{id}/confirm",
                post(pos::confirm_write_off),
            )
            // Переміщення
            .route(
                "/api/v1/transfers",
                get(pos::list_transfers).post(pos::create_transfer),
            )
            .route(
                "/api/v1/transfers/{id}",
                get(pos::get_transfer)
                    .put(pos::update_transfer)
                    .delete(pos::delete_transfer),
            )
            .route(
                "/api/v1/transfers/{id}/confirm",
                post(pos::confirm_transfer),
            )
            // Зміни ПРРО (X/Z)
            .route("/api/v2/prro/shifts", get(pos::list_shifts))
            .route("/api/v2/prro/shift/open", post(pos::open_shift))
            .route("/api/v2/prro/shift/close", post(pos::close_shift));
    }

    // Усе, що не health і не Rust-гілка, — у Python sidecar (метод/шлях/тіло/заголовки).
    router
        .fallback(proxy::proxy_handler)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .with_state(state)
}

/// GET /api/v1/health → 200 {"status":"ok"} (без JWT — відкритий).
pub async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(crate::health_payload())
}
