//! Smoke-тест TLS gRPC-з'єднання з тестовим API ПРРО ДПС — 1:1 Python
//! `smoke_test.py` (docs/prro_phase0_ping.md):
//! - TLS-канал до cabinet.tax.gov.ua:9443 (WebPKI native roots);
//! - ping (local_number=0x7FFFFFFF, check_type=SERVICECHK, check_sign порожній);
//! - очікувано: канал READY, відповідь status=-1 (ERROR_VEREFY) — ідентично Python.
//!
//! Запуск: cargo run -p kasa-prro --example smoke_ping -- [target]
//! (target за замовчуванням: cabinet.tax.gov.ua:9443)

use kasa_prro::grpc::{check_date_time, PrroGrpcClient, TlsConfig};
use kasa_prro::proto::prro::check_response::Status;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "cabinet.tax.gov.ua:9443".to_string());
    let rro_fn = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "0000000000".to_string());

    println!("{}", "=".repeat(64));
    println!("PRRO 7.1: Smoke-тест Rust gRPC (TLS) → {target}");
    println!("{}", "=".repeat(64));

    // 1. TLS-канал
    println!("1. Створення TLS-каналу (native roots)...");
    let client = PrroGrpcClient::connect(&target, TlsConfig::default(), rro_fn).await?;
    println!("   ✅ TLS-канал створено, з'єднання встановлено (Endpoint::connect)");

    // 2. ping
    println!("2. Відправка ping (local_number=0x7FFFFFFF, check_type=SERVICECHK)...");
    println!("   date_time={} (yyyyMMddHHmmss)", check_date_time());
    let resp = client.ping(Vec::new()).await?;

    println!("   ✅ Отримано відповідь від сервера!");
    println!("   id            = {:?}", resp.id);
    println!(
        "   status        = {} ({})",
        resp.status,
        Status::try_from(resp.status)
            .map(|s| s.as_str_name().to_string())
            .unwrap_or_else(|_| "UNKNOWN".to_string())
    );
    println!("   error_message = {:?}", resp.error_message);
    println!("   id_sign_len   = {}", resp.id_sign.len());
    println!("   data_sign_len = {}", resp.data_sign.len());

    if resp.status == Status::Ok as i32 {
        println!("   → Статус OK");
    } else {
        println!("   → Статус != OK (очікувано: ПРРО не зареєстровано)");
        println!("   → gRPC-канал ЖИВИЙ — TLS-з'єднання працює ✅");
    }

    println!("{}", "=".repeat(64));
    println!("SMOKE-ТЕСТ ЗАВЕРШЕНО");
    Ok(())
}
