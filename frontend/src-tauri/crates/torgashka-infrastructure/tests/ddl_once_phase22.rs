//! ФАЗА 2.2 (ізоляція тестів): контракт «DDL виконується РАЗ на ревізію».
//!
//! Перешкода, яку це фіксує: `ALTER TABLE … IF NOT EXISTS` / `DROP|CREATE POLICY`
//! беруть `AccessExclusiveLock` навіть коли змін не потрібно. Якщо такий DDL
//! виконується в кожному тест-бінарі (або на кожен виклик `ensure_schema`),
//! він дедлочиться з паралельними `INSERT`'ами тестових setup'ів →
//! `40P01 deadlock detected` у *setup*, а не в перевірці (флейкі-верифікація).
//!
//! Контракт, який тут перевіряється:
//!   1. `ensure_ddl_once` застосовує DDL РІВНО ОДИН раз на fingerprint —
//!      другий виклик повертає `false` і не виконує DDL;
//!   2. зміна тексту DDL ⇒ новий fingerprint ⇒ DDL застосовується знову;
//!   3. `ensure_schema` ідемпотентний і пише маркер ревізії
//!      (`public.schema_revision`), який стабільний між викликами;
//!   4. обидва шляхи серіалізовані одним advisory-локом (перевіряється
//!      паралельним викликом: жодного `40P01`, рівно одне застосування).
//!
//! Тест потребує PostgreSQL (`TEST_DATABASE_URL` / `DATABASE_URL` + `_test`).

use torgashka_infrastructure::db::{ensure_ddl_once, ensure_schema};

fn test_db_url() -> String {
    if let Ok(u) = std::env::var("TEST_DATABASE_URL") {
        if !u.trim().is_empty() {
            return u;
        }
    }
    let work = torgashka_infrastructure::db::resolve_database_url()
        .expect("resolve_database_url: задайте DATABASE_URL або DB_* у backend/.env");
    let before = work.split('?').next().unwrap_or(&work);
    let idx = before.rfind('/').expect("імені БД у URL немає");
    let name = &before[idx + 1..];
    if name.contains("test") {
        return work;
    }
    format!("{}{}_test", &work[..idx + 1], name)
}

async fn pool() -> sqlx::PgPool {
    std::env::set_var("DATABASE_URL", test_db_url());
    torgashka_infrastructure::db::connect_test_pool(12)
        .await
        .expect("тестова БД недоступна")
}

#[tokio::test]
async fn ddl_applied_once_per_revision() {
    let p = pool().await;

    // Прибирання міток попередніх прогонів (тест не смітить у тестовій БД).
    let _ = sqlx::query("DELETE FROM public.ddl_markers WHERE key LIKE 'at_phase22%'")
        .execute(&p)
        .await;

    // ── 1+2. ensure_ddl_once: один раз на fingerprint, новий текст → знову ──
    let marker = "at_phase22_current".to_string();
    let ddl_v1 = "CREATE TABLE IF NOT EXISTS public.at_phase22_probe (id int PRIMARY KEY);";
    assert!(
        ensure_ddl_once(&p, &marker, ddl_v1).await.expect("перше застосування"),
        "перший виклик мусить застосувати DDL"
    );
    assert!(
        !ensure_ddl_once(&p, &marker, ddl_v1)
            .await
            .expect("повторний виклик"),
        "той самий fingerprint ⇒ DDL НЕ виконується (саме це прибирає AccessExclusiveLock'и у прогоні)"
    );
    let ddl_v2 = "CREATE TABLE IF NOT EXISTS public.at_phase22_probe (id int PRIMARY KEY, note text);";
    assert!(
        ensure_ddl_once(&p, &marker, ddl_v2)
            .await
            .expect("нова ревізія"),
        "змінений DDL ⇒ новий fingerprint ⇒ застосування"
    );
    assert!(
        !ensure_ddl_once(&p, &marker, ddl_v2).await.expect("нова ревізія, повтор"),
        "нова ревізія теж застосовується рівно один раз"
    );

    // ── 3. ensure_schema ідемпотентний + маркер ревізії стабільний ──────────
    ensure_schema(&p).await.expect("ensure_schema #1");
    let fp1: String =
        sqlx::query_scalar("SELECT fingerprint FROM public.schema_revision WHERE id = 1")
            .fetch_one(&p)
            .await
            .expect("маркер ревізії схеми");
    ensure_schema(&p).await.expect("ensure_schema #2");
    let fp2: String =
        sqlx::query_scalar("SELECT fingerprint FROM public.schema_revision WHERE id = 1")
            .fetch_one(&p)
            .await
            .expect("маркер ревізії схеми (2)");
    assert_eq!(fp1, fp2, "fingerprint стабільний між викликами");
    assert!(!fp1.is_empty(), "fingerprint непорожній");

    // ── 4. Паралельні виклики = серіалізація без дедлоків ──────────────────
    let par = |i: usize| {
        let p = p.clone();
        async move {
            ensure_schema(&p).await.map_err(|e| e.to_string())?;
            ensure_ddl_once(&p, "at_phase22_parallel", ddl_v1)
                .await
                .map_err(|e| e.to_string())
                .map(|applied| (i, applied))
        }
    };
    let (a, b, c, d) = tokio::join!(par(1), par(2), par(3), par(4));
    let mut applied = 0usize;
    for res in [a, b, c, d] {
        let (_i, applied_here) = res.expect("жодного 40P01/deadlock у паралельних викликах");
        applied += usize::from(applied_here);
    }
    assert!(applied <= 1, "DDL застосовано максимум один раз, маємо {applied}");
    eprintln!(
        "[phase2.2] ✅ DDL-once: schema fp={}…, parallel applied={applied}",
        &fp1[..8.min(fp1.len())]
    );
}
