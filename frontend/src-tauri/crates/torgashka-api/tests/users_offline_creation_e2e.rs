//! E5-частина B (Б2/D3, ADR-0008 §7.1-D3, §10 №2, §4.2): КАСИР, СТВОРЕНИЙ
//! ЛОКАЛЬНО НА ВУЗЛІ.
//!
//! Рішення Творця Б2 (2026-09-16, варіант A): касир створюється локально на
//! вузлі (offline-first, ADR §2.2), бо каса мусить працювати, коли хаба немає;
//! `sync_state='pending_hub'` (§7.1-D3) розрізняє «локально створене, ще не
//! підтверджене» від канонічного. Стан канонізує хаб — прийняттям ПРОПОЗИЦІЇ
//! (`POST /api/v1/sync/catalog-proposal`, арбітраж §4.2, той самий, що для
//! `products`/`suppliers`) — і ставить рядку `sync_state='confirmed'`.
//!
//! Що доводиться:
//!   1. `cashier_created_offline_confirmed_after_hub_contact` — касир створений
//!      РЕАЛЬНИМ шляхом вузла (`POST /api/v1/users`) без жодного звернення до
//!      хаба; маркер `pending_hub`; касир ВХОДИТЬ по PIN до контакту з хабом
//!      (прапорець `REQUIRE_HUB_CONFIRM_BEFORE_LOGIN` за замовчуванням false —
//!      offline-first); після контакту пропозиція прийнята, хаб-рядок
//!      `confirmed`, касир видимий ДРУГОМУ вузлу дельтою `employees`.
//!   2. `second_proposal_for_same_user_visible_as_conflict` — дві пропозиції на
//!      одного касира: ОБИДВІ в журналі зі `status='conflict'`, до довідника не
//!      застосовано нічого нового (жодного «тихого злиття»).
//!   3. `pending_hub_login_blocked_by_flag` — варіант C (блок входу до
//!      підтвердження) вмикається ОДНИМ прапорцем середовища; рішення
//!      оборотне, код не змінюється.
//!
//! ⚠ Цей бінар виконувати з `--test-threads=1`: тест прапорця пише змінну
//! середовища ПРОЦЕСУ (стан спільний для всіх тестів бінаря).
//!
//! Застосування дельти у локальну БД вузла (дзеркало `offline/sync_pull.rs`) і
//! ПЕРЕСИЛКА пропозиції з вузла — клієнтська частина (Tauri), поза крейтом API;
//! тому тут перевіряється канал хаба, який вузол тягне, а пропозиція надсилається
//! так, як це зробить клієнт (payload — зі свого рядка).

mod common;

use serde_json::json;
use uuid::Uuid;

#[path = "common/hub_env.rs"]
mod hub_env;

use hub_env as env;

fn evidence(line: &str) {
    eprintln!("[e5b][evidence] {line}");
}

/// Три інстанси: хаб + вузол-автор + ДРУГИЙ вузол (споживач) — той самий
/// патерн, що в `catalog_proposal_e2e.rs` (окремі БД, окремі фасади).
async fn three_instances(tag: &str) -> (sqlx::PgPool, sqlx::PgPool, sqlx::PgPool) {
    common::force_test_db();
    let admin = env::pool_to(&env::test_db_url()).await;
    let hub = env::hub_pool(&admin, tag).await;
    let node1 = env::node_pool(&hub, &format!("{tag}1")).await;
    let node2 = env::node_pool(&hub, &format!("{tag}2")).await;
    let seed_product = Uuid::new_v4();
    env::seed(&hub, seed_product, &format!("{tag}-hub")).await;
    env::seed(&node1, seed_product, &format!("{tag}-n1")).await;
    env::seed(&node2, seed_product, &format!("{tag}-n2")).await;
    (hub, node1, node2)
}

/// Створити касира ЛОКАЛЬНО на вузлі реальним шляхом фасаду
/// (`POST /api/v1/users`) → (id, login).
async fn create_cashier_offline(base: &str, token: &str, name: &str) -> (Uuid, String) {
    let (code, body) = env::create_user(
        base,
        token,
        json!({
            "name": name,
            "password": "cashier-pass-1",
            "pin_code": "4321",
            "role": "cashier",
        }),
    )
    .await;
    assert_eq!(code, 201, "створення касира на вузлі: {body}");
    let id = Uuid::parse_str(body["id"].as_str().unwrap_or_default()).expect("id касира");
    let login = body["login"].as_str().unwrap_or_default().to_string();
    assert!(!login.is_empty(), "логін касира: {body}");
    (id, login)
}

#[tokio::test]
async fn cashier_created_offline_confirmed_after_hub_contact() {
    let (hub, node1, node2) = three_instances("usersB").await;

    let (hub_base, _) = env::serve_any(env::app_state_with_readdirs(&hub)).await;
    let (n1_base, _) = env::serve_any(env::app_state_node(&node1)).await;
    let (n2_base, _) = env::serve_any(env::app_state_node(&node2)).await;
    env::wait_ready(&hub_base).await;
    env::wait_ready(&n1_base).await;
    env::wait_ready(&n2_base).await;

    let token = env::owner_token();
    // Вузол знає свій апстрім (це і робить його вузлом) — але жодного запиту в
    // хаба створення касира не робить: воно локальне за визначенням (§2.2).
    env::set_hub_upstream(&node1, &hub_base, &token).await;
    env::set_hub_upstream(&node2, &hub_base, &token).await;

    // ── 1. Локальне створення (offline-first) ─────────────────────────────
    let (cashier_id, login) = create_cashier_offline(&n1_base, &token, "E5-B2 Касир").await;
    let (sync_state, pin_hash, pass_hash, role): (String, Option<String>, String, String) =
        sqlx::query_as(
            "SELECT sync_state, pin_code, password_hash, role::text FROM users WHERE id = $1",
        )
        .bind(cashier_id)
        .fetch_one(&node1)
        .await
        .expect("касир у БД вузла");
    assert_eq!(
        sync_state, "pending_hub",
        "D3: створене на вузлі ще не підтверджене хабом"
    );
    assert_eq!(role, "cashier");
    assert!(pin_hash.is_some(), "PIN-хеш касира: {pin_hash:?}");
    let hub_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE id = $1")
        .bind(cashier_id)
        .fetch_one(&hub)
        .await
        .expect("пошук касира на хабі");
    assert_eq!(
        hub_rows, 0,
        "створення НЕ вимагало хаба: рядка на хабі ще немає"
    );
    evidence(&format!(
        "касир {cashier_id} створений ЛОКАЛЬНО на вузлі: sync_state=pending_hub, на хабі рядків={hub_rows}"
    ));

    // ── 2. Offline-first: PIN-логін ДО контакту з хабом ───────────────────
    // Прапорець REQUIRE_HUB_CONFIRM_BEFORE_LOGIN за замовчуванням = false
    // (ADR §2.2: точка працює без мережі; варіант C — окреме рішення).
    let (code_login, body_login) = env::login_pin(&n1_base, &login, "4321").await;
    assert_eq!(
        code_login, 200,
        "касир мусить входити без підтвердження хабом (default false): {body_login}"
    );
    evidence("offline-first: PIN-логін касира на вузлі працює ДО контакту з хабом (200)");

    // ── 3. Контакт із хабом: пропозиція зі свого рядка ────────────────────
    let cu = Uuid::new_v4();
    let (code_p, body_p) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "users",
            cashier_id,
            "upsert",
            cu,
            json!({
                "name": "E5-B2 Касир",
                "login": login,
                "password_hash": pass_hash,
                "pin_hash": pin_hash,
                "role": role,
            }),
            0,
        )],
    )
    .await;
    assert_eq!(code_p, 200, "вузол → хаб (пропозиція касира): {body_p}");
    assert_eq!(
        body_p["results"][0]["status"], "accepted",
        "хаб приймає касира як канонічного: {body_p}"
    );
    let version = body_p["results"][0]["server_version"]
        .as_i64()
        .unwrap_or_else(|| panic!("хаб мусить присвоїти server_version: {body_p}"));

    // ── 4. Хаб: рядок канонічний (`confirmed`), версія з `sync_meta` ──────
    let (hub_state, hub_version, hub_deleted, hub_name): (String, i64, bool, String) =
        sqlx::query_as(
            "SELECT sync_state, server_version, is_deleted, name FROM users WHERE id = $1",
        )
        .bind(cashier_id)
        .fetch_one(&hub)
        .await
        .expect("касир на хабі після прийняття");
    assert_eq!(
        hub_state, "confirmed",
        "хаб — авторитет (§4.2 п.3): канонічний рядок підтверджено"
    );
    assert_eq!(hub_name, "E5-B2 Касир");
    assert!(!hub_deleted);
    assert_eq!(hub_version, version, "версія рядка = версія з відповіді");
    let meta: i64 = sqlx::query_scalar("SELECT version FROM sync_meta WHERE entity = 'employees'")
        .fetch_one(&hub)
        .await
        .expect("sync_meta.employees (тригер trg_users_bump)");
    assert_eq!(
        meta, version,
        "присвоєна версія = sync_meta.employees (одне джерело канонічної версії)"
    );
    let (j_status, j_version): (String, Option<i64>) = sqlx::query_as(
        "SELECT status, server_version FROM catalog_change_requests WHERE client_uuid = $1",
    )
    .bind(cu)
    .fetch_one(&hub)
    .await
    .expect("журнал пропозицій");
    assert_eq!(j_status, "accepted");
    assert_eq!(j_version, Some(version));
    evidence(&format!(
        "вузол→хаб: касир {cashier_id} прийнято, server_version={version} (= sync_meta.employees), \
         хаб-рядок sync_state=confirmed, журнал accepted"
    ));

    // ── 5. Другий вузол бачить касира дельтою `employees` (наявний pull) ──
    let (code_st, st2) = env::sync_status(&n2_base, &token).await;
    assert_eq!(code_st, 200, "status другого вузла: {st2}");
    assert_eq!(st2["role"], "node", "другий вузол — окремий інстанс: {st2}");

    let (code_d, delta) = env::master_delta(&hub_base, &token, "employees", 0).await;
    assert_eq!(
        code_d, 200,
        "GET /api/v1/sync/master?entity=employees: {delta}"
    );
    let found = delta["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("дельта без changes: {delta}"))
        .iter()
        .find(|c| c["id"] == cashier_id.to_string())
        .unwrap_or_else(|| panic!("касира немає в дельті хаба: {delta}"));
    assert_eq!(found["op"], "upsert");
    assert_eq!(found["version"].as_i64(), Some(version));
    assert_eq!(found["data"]["name"], "E5-B2 Касир");
    assert_eq!(found["data"]["role"], "cashier");
    assert_eq!(
        found["data"]["pin_hash"].as_str(),
        pin_hash.as_deref(),
        "вузол отримує PIN-хеш (каса логінить по PIN): {found}"
    );
    evidence(&format!(
        "другий вузол через наявний pull хаба: у дельті employees є {cashier_id} з version={version}, \
         name/role/pin_hash — касир видимий"
    ));
}

#[tokio::test]
async fn second_proposal_for_same_user_visible_as_conflict() {
    let (hub, node1, node2) = three_instances("usersC").await;

    let (hub_base, _) = env::serve_any(env::app_state_with_readdirs(&hub)).await;
    let (n1_base, _) = env::serve_any(env::app_state_node(&node1)).await;
    let (n2_base, _) = env::serve_any(env::app_state_node(&node2)).await;
    env::wait_ready(&hub_base).await;
    env::wait_ready(&n1_base).await;
    env::wait_ready(&n2_base).await;

    let token = env::owner_token();
    env::set_hub_upstream(&node1, &hub_base, &token).await;
    env::set_hub_upstream(&node2, &hub_base, &token).await;

    // Касир створений локально (на вузлі 1) — предмет двох правок.
    let (cashier_id, login) = create_cashier_offline(&n1_base, &token, "E5-B2 Конфлікт").await;
    let (pin_hash, pass_hash): (Option<String>, String) =
        sqlx::query_as("SELECT pin_code, password_hash FROM users WHERE id = $1")
            .bind(cashier_id)
            .fetch_one(&node1)
            .await
            .expect("касир на вузлі 1");

    // ── Пропозиція №1 (вузол 1) → прийнята ────────────────────────────────
    let cu1 = Uuid::new_v4();
    let (code1, body1) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "users",
            cashier_id,
            "upsert",
            cu1,
            json!({
                "name": "E5-B2 Конфлікт (правка 1)",
                "login": login,
                "password_hash": pass_hash,
                "pin_hash": pin_hash,
                "role": "cashier",
            }),
            0,
        )],
    )
    .await;
    assert_eq!(code1, 200, "пропозиція №1: {body1}");
    assert_eq!(body1["results"][0]["status"], "accepted");

    // ── Пропозиція №2 (ДРУГИЙ вузол) на ТОЙ САМИЙ рядок → конфлікт ───────
    let cu2 = Uuid::new_v4();
    let (code2, body2) = env::propose(
        &hub_base,
        &token,
        &[env::proposal_env(
            "users",
            cashier_id,
            "upsert",
            cu2,
            json!({
                "name": "E5-B2 Конфлікт (правка 2, інший вузол)",
                "login": login,
                "password_hash": pass_hash,
                "pin_hash": pin_hash,
                "role": "store_manager",
            }),
            1,
        )],
    )
    .await;
    assert_eq!(code2, 200, "пропозиція №2: {body2}");
    assert_eq!(
        body2["results"][0]["status"], "conflict",
        "друга правка того самого рядка — конфлікт, а не тихе перезаписування: {body2}"
    );
    assert!(
        body2["results"][0]["server_version"].is_null(),
        "конфлікт не присвоює версії: {body2}"
    );

    // ── ОБИДВІ пропозиції в журналі (жодна не «злита мовчки») ────────────
    let journal: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, count(*) FROM catalog_change_requests \
         WHERE entity = 'users' AND row_id = $1 GROUP BY status ORDER BY status",
    )
    .bind(cashier_id)
    .fetch_all(&hub)
    .await
    .expect("журнал пропозицій users");
    assert_eq!(
        journal,
        vec![("conflict".to_string(), 2)],
        "в журналі РІВНО дві пропозиції, обидві conflict: {journal:?}"
    );
    let cus: Vec<Uuid> = sqlx::query_scalar(
        "SELECT client_uuid FROM catalog_change_requests \
         WHERE entity = 'users' AND row_id = $1 ORDER BY id",
    )
    .bind(cashier_id)
    .fetch_all(&hub)
    .await
    .expect("client_uuid обох пропозицій");
    assert_eq!(cus, vec![cu1, cu2], "обидві пропозиції на місці");

    // ── До довідника НЕ застосовано нічого нового + видно в черзі оператора ──
    let hub_name: String = sqlx::query_scalar("SELECT name FROM users WHERE id = $1")
        .bind(cashier_id)
        .fetch_one(&hub)
        .await
        .expect("рядок касира на хабі");
    assert_eq!(
        hub_name, "E5-B2 Конфлікт (правка 1)",
        "конфлікт НЕ зливає правки: лишається стан пропозиції №1"
    );
    let (code_c, conflicts) = env::admin_conflicts(&hub_base, &token).await;
    assert_eq!(code_c, 200, "черга конфліктів: {conflicts}");
    let group = conflicts["groups"]
        .as_array()
        .unwrap_or_else(|| panic!("групи конфліктів: {conflicts}"))
        .iter()
        .find(|g| g["entity"] == "users" && g["row_id"] == cashier_id.to_string())
        .unwrap_or_else(|| panic!("групи users/{cashier_id} немає в черзі: {conflicts}"));
    assert_eq!(
        group["proposals"].as_array().map(Vec::len),
        Some(2),
        "оператор бачить ОБИДВІ сторони конфлікту: {group}"
    );
    evidence(&format!(
        "КОНФЛІКТ users/{cashier_id}: обидві пропозиції в журналі (status=conflict, {cu1} і {cu2}); \
         до довідника не застосовано нічого (name лишився від №1), черга оператора показує 2 сторони"
    ));
}

/// Варіант C (ADR §10 №2) — блок входу до підтвердження хабом — вмикається
/// ОДНИМ прапорцем середовища. Рішення Творця: default false (offline-first),
/// але оборотність гарантована: щоб перейти на C, код не змінюється.
#[tokio::test]
async fn pending_hub_login_blocked_by_flag() {
    common::force_test_db();
    let admin = env::pool_to(&env::test_db_url()).await;
    let node = env::node_pool(&admin, "usersD").await;
    let seed_product = Uuid::new_v4();
    env::seed(&node, seed_product, "usersD-n1").await;

    let (n_base, _) = env::serve_any(env::app_state_node(&node)).await;
    env::wait_ready(&n_base).await;

    let token = env::owner_token();
    // URL хаба може бути недосяжним: для маркера важлива ЛИШЕ наявність
    // налаштування (роль інстанса — налаштування ЙОГО власної БД).
    env::set_hub_upstream(&node, "http://127.0.0.1:1", &token).await;

    let (cashier_id, login) = create_cashier_offline(&n_base, &token, "E5-B2 Прапорець").await;
    let sync_state: String = sqlx::query_scalar("SELECT sync_state FROM users WHERE id = $1")
        .bind(cashier_id)
        .fetch_one(&node)
        .await
        .expect("маркер касира");
    assert_eq!(sync_state, "pending_hub");

    // 1. Прапорець НЕ встановлено (продакшн-дефолт) → вхід дозволено.
    let (code_default, body_default) = env::login_pin(&n_base, &login, "4321").await;
    assert_eq!(
        code_default, 200,
        "default false: pending_hub входить (offline-first): {body_default}"
    );

    // 2. Вмикаємо варіант C → вхід блокується до підтвердження хабом.
    struct FlagGuard;
    impl Drop for FlagGuard {
        fn drop(&mut self) {
            std::env::remove_var("REQUIRE_HUB_CONFIRM_BEFORE_LOGIN");
        }
    }
    std::env::set_var("REQUIRE_HUB_CONFIRM_BEFORE_LOGIN", "true");
    let _guard = FlagGuard;
    let (code_blocked, body_blocked) = env::login_pin(&n_base, &login, "4321").await;
    assert_eq!(
        code_blocked, 403,
        "варіант C: вхід непідтвердженого касира заблоковано: {body_blocked}"
    );
    assert!(
        body_blocked["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("pending_hub"),
        "причина відмови називає стан: {body_blocked}"
    );
    evidence(&format!(
        "прапорець REQUIRE_HUB_CONFIRM_BEFORE_LOGIN=true → 403 для pending_hub: {}",
        body_blocked["detail"].as_str().unwrap_or_default()
    ));

    // 3. Після підтвердження (канонічний рядок) прапорець не блокує.
    //    Клієнтську реконсиляцію («моя правка прийнята як версія N» → локальний
    //    маркер confirmed) тут МОДЕЛЮЄМО прямим UPDATE: пересилка пропозиції з
    //    вузла — клієнтська частина (Tauri), поза крейтом API.
    sqlx::query("UPDATE users SET sync_state = 'confirmed' WHERE id = $1")
        .bind(cashier_id)
        .execute(&node)
        .await
        .expect("модель підтвердження хабом");
    let (code_ok, body_ok) = env::login_pin(&n_base, &login, "4321").await;
    assert_eq!(
        code_ok, 200,
        "підтверджений касир входить навіть із прапорцем C: {body_ok}"
    );
}
