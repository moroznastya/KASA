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
    routing::{get, post, put},
    Router,
};

use crate::{auth, auth_routes, crud, ledger, pos, proxy, readdirs, AppState};

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

    // Rust-гілка ledger (етап 4) — під тим самим feature-flag.
    if state.ledger.is_some() {
        // v1 (статичний /balance ПЕРЕД /{supplier_id} — як FastAPI).
        router = router
            .route(
                "/api/v1/ledger/balance/{supplier_id}",
                get(ledger::balance_v1),
            )
            .route("/api/v1/ledger/{supplier_id}", get(ledger::history_v1))
            .route("/api/v1/ledger", post(ledger::create_entry_v1))
            // v2 (entries/balances ПЕРЕД balance/{supplier_id}).
            .route("/api/v2/ledger/entries", get(ledger::list_entries_v2))
            .route("/api/v2/ledger/entries", post(ledger::create_entry_v2))
            .route("/api/v2/ledger/balances", get(ledger::all_balances_v2))
            .route(
                "/api/v2/ledger/balance/{supplier_id}",
                get(ledger::balance_v2),
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

    // Rust-гілка auth/users/settings/RBAC (етап 6) — під KASA_RUST_AUTH=1.
    // Порядок: статичні сегменти (users-list, users/me, permissions/list)
    // ПЕРЕД {user_id} — як FastAPI.
    if state.auth.is_some() {
        router = router
            // Auth (публічні + JWT).
            .route("/api/v1/auth/login", post(auth_routes::login))
            .route("/api/v1/auth/login-pin", post(auth_routes::login_pin))
            .route("/api/v1/auth/refresh", post(auth_routes::refresh))
            .route("/api/v1/auth/logout", post(auth_routes::logout))
            .route("/api/v1/auth/verify", get(auth_routes::verify))
            .route("/api/v1/auth/users-list", get(auth_routes::users_list))
            // Users (всі require_admin).
            .route(
                "/api/v1/users/permissions/list",
                get(auth_routes::permissions_list),
            )
            .route(
                "/api/v1/users/{user_id}/permissions",
                put(auth_routes::update_permissions),
            )
            .route(
                "/api/v1/users/{user_id}/hourly-rate",
                put(auth_routes::update_hourly_rate),
            )
            .route(
                "/api/v1/users/{user_id}",
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
                "/api/v1/settings/{module}",
                get(auth_routes::settings_by_module),
            )
            .route(
                "/api/v1/settings/{key}",
                put(auth_routes::settings_update_key),
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
