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

use crate::{auth, crud, proxy, readdirs, AppState};

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
