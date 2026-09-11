//! ІНВАРІАНТ ДЕФЕКТУ 5: «порт біндиться → HTTP-відповідь на /api/v1/setup/status
//! віддається НАВІТЬ якщо БД недоступна / ініціалізація триває».
//!
//! Було: `serve_listener` виконував СИНХРОННИЙ bootstrap ДО `axum::serve` —
//! TCP-handshake завершувався (порт LISTENING), а HTTP-відповіді не було
//! (curl: `(28) Operation timed out ... 0 bytes received`).
//!
//! Доводиться реальним HTTP на ефемерному порту, у дві фази:
//!   ФАЗА A — DATABASE_URL на локальний TCP-«тарпит» (приймає з'єднання й
//!            мовчить → PG-handshake не завершується → ініціалізація триває):
//!            відповідь мусить прийти НЕГАЙНО, зі станом `starting`.
//!   ФАЗА B — DATABASE_URL на закритий порт (з'єднання відкидається): фасад
//!            переходить на справжній роутер і віддає ЧЕСНИЙ стан
//!            `db_unavailable` (а не таймаут і не 404-невідомість).
use std::time::{Duration, Instant};

/// TCP-«тарпит»: приймає з'єднання і тримає їх відкритими, не відповідаючи.
fn spawn_tarpit() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("тарпит-порт");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming() {
            match conn {
                Ok(c) => held.push(c),
                Err(_) => break,
            }
        }
    });
    port
}

/// Підняти фасад на ефемерному порту; повертає (адреса, handle).
async fn start_facade() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("бінд ефемерного порту");
    let addr = listener.local_addr().expect("local_addr").to_string();
    let handle = tokio::spawn(async move {
        let _ = torgashka_api::serve_listener(listener).await;
    });
    (addr, handle)
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .expect("http-клієнт")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn facade_answers_while_db_is_unavailable_or_initializing() {
    // Ізоляція від конфігів машини (жодних db_sources.toml у CWD).
    std::env::remove_var("TORGASHKA_DB_SOURCES");

    // ── ФАЗА A: ініціалізація триває (тарпит) ────────────────────────────────
    let tarpit_port = spawn_tarpit();
    std::env::set_var(
        "DATABASE_URL",
        format!("postgresql://postgres@127.0.0.1:{tarpit_port}/torgashka"),
    );
    let (addr, handle_a) = start_facade().await;
    let client = http_client();

    // Проба готовності ОДРАЗУ: ініціалізація точно триває (PG-handshake висить).
    let t0 = Instant::now();
    let resp = client
        .get(format!("http://{addr}/api/v1/setup/status"))
        .send()
        .await
        .expect("HTTP-відповідь мусить прийти — таймаут = дефект 5 не виправлено");
    let elapsed = t0.elapsed();
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp.json().await.expect("JSON-тіло");
    assert!(
        elapsed < Duration::from_secs(3),
        "відповідь мусить бути негайною (<3 с), а була {elapsed:?}"
    );
    assert_eq!(status, 503, "{body}");
    assert_eq!(
        body["status"], "starting",
        "під час ініціалізації — чесний стан 'starting': status={status} {body}"
    );

    // /health (readiness-проба e2e-тестів) — теж відповідає, а не мовчить.
    let t1 = Instant::now();
    let health = client
        .get(format!("http://{addr}/api/v1/health"))
        .send()
        .await
        .expect("HTTP-відповідь на /health");
    assert!(t1.elapsed() < Duration::from_secs(3), "{:?}", t1.elapsed());
    assert_eq!(
        health.status().as_u16(),
        503,
        "під час ініціалізації: not ready"
    );
    handle_a.abort();

    // ── ФАЗА B: БД недоступна → чесний стан після ініціалізації, не таймаут ──
    // DSN, який неможливо розібрати: пули відмовляють МИТТЄВО (без 5-секундних
    // acquire-таймаутів) — тому фаза швидка й детермінована. TCP-недоступність
    // БД (закритий порт / тарпит) фаза A уже покриває: там ініціалізація триває
    // хвилину (11 пулів × acquire_timeout=5 с), і саме тоді HTTP відповідає.
    std::env::set_var("DATABASE_URL", "postgresql://не-dsn");
    let (addr_b, handle_b) = start_facade().await;

    let deadline = Instant::now() + Duration::from_secs(30);
    let last = loop {
        let t = Instant::now();
        let r = client
            .get(format!("http://{addr_b}/api/v1/setup/status"))
            .send()
            .await
            .expect("HTTP-відповідь після ініціалізації");
        assert!(
            t.elapsed() < Duration::from_secs(3),
            "handler не має блокуватись: {:?}",
            t.elapsed()
        );
        let status = r.status().as_u16();
        let body: serde_json::Value = r.json().await.expect("JSON-тіло");
        let msg = format!("{status} {body}");
        if body["status"] == "db_unavailable" {
            assert_eq!(status, 503, "{msg}");
            break msg;
        }
        assert!(
            Instant::now() < deadline,
            "фасад не має вічно віддавати 'starting' при недоступній БД: {msg}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(last.contains("db_unavailable"), "{last}");
    handle_b.abort();
}
