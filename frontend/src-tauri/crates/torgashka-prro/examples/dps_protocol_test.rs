//! Інтеграційний тест протоколу ПРРО проти тестового API ДПС (cabinet.tax.gov.ua:9443).
//!
//! Перевіряє роботу всіх RPC-методів згідно офіційної документації
//! «Опис API Електронного кабінету для фіскалізації чеків та передачі Z-звітів»:
//!   - sendChkV2 (Check) → CheckResponse
//!   - ping (Check) → CheckResponse            (CT=111, local_number=0x7FFFFFFF)
//!   - lastChk (CheckRequest) → CheckResponse
//!   - delLastChk (CheckRequest) → CheckResponse
//!   - delLastChkId (CheckRequestId) → CheckResponse
//!   - statusRro (CheckRequest) → StatusResponse
//!   - infoRro (CheckRequest) → RroInfoResponse
//!
//! Без валідних ключів КЕП сервер повертає документовані коди помилок
//! (ERROR_VEREFY=-1, ERROR_NOT_REGISTERED_RRO=-13 тощо) — це доводить,
//! що TLS-канал, proto-визначення та маршрутизація методів коректні.
//!
//! Запуск: cargo run -p torgashka-prro --example dps_protocol_test

use torgashka_prro::grpc::{check_date_time, PrroGrpcClient, TlsConfig};
use torgashka_prro::proto::prro::check_response::Status as CheckStatus;
use torgashka_prro::proto::prro::rro_info_response::Status as InfoStatus;
use torgashka_prro::proto::prro::status_response::Status as StatusRroStatus;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "cabinet.tax.gov.ua:9443".to_string());
    let rro_fn = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "0000000000".to_string());

    println!("{}", "=".repeat(72));
    println!("PRRO: Тест протоколу згідно документації ДПС → {target}");
    println!("{}", "=".repeat(72));

    let client = PrroGrpcClient::connect(&target, TlsConfig::default(), rro_fn.clone()).await?;
    println!("✅ TLS-канал до {target} встановлено (native roots)\n");

    let mut passed = 0u32;
    let mut failed = 0u32;

    macro_rules! check {
        ($name:expr, $cond:expr, $detail:expr) => {{
            if $cond {
                println!("  ✅ {} — {}", $name, $detail);
                passed += 1;
            } else {
                println!("  ❌ {} — {}", $name, $detail);
                failed += 1;
            }
        }};
    }

    // ── 1. ping: local_number=0x7FFFFFFF, check_type=SERVICECHK, CT=111 ──
    println!("[1] ping (Check) → CheckResponse [док: local_number=0x7FFFFFFF]");
    let resp = client.ping(Vec::new()).await?;
    let st = CheckStatus::try_from(resp.status)
        .map(|s| s.as_str_name())
        .unwrap_or("UNKNOWN");
    println!(
        "    status={} ({}), error_message={:?}",
        resp.status, st, resp.error_message
    );
    check!(
        "ping: відповідь отримана, gRPC-канал живий",
        true,
        format!("status={}", resp.status)
    );
    check!(
        "ping: статус є документованим enum",
        resp.status == -1 || resp.status == 1,
        st
    );
    check!(
        "ping: дата у форматі yyyyMMddHHmmss (14 цифр)",
        check_date_time().to_string().len() == 14,
        check_date_time().to_string()
    );

    // ── 2. sendChkV2 з порожнім підписом ──
    println!("\n[2] sendChkV2 (Check) → CheckResponse [док: з 01.10.2021 обов'язковий]");
    let resp = client
        .send_chk_v2(torgashka_prro::proto::Check {
            rro_fn: rro_fn.clone(),
            date_time: check_date_time(),
            check_sign: Vec::new(),
            local_number: 1,
            check_type: torgashka_prro::proto::CheckType::Chk as i32,
            id_offline: String::new(),
            id_cancel: String::new(),
        })
        .await?;
    let st = CheckStatus::try_from(resp.status)
        .map(|s| s.as_str_name())
        .unwrap_or("UNKNOWN");
    println!(
        "    status={} ({}), error_message={:?}",
        resp.status, st, resp.error_message
    );
    check!(
        "sendChkV2: метод існує та відповідає (не транспортна помилка)",
        true,
        st
    );
    check!(
        "sendChkV2: статус у документованому діапазоні [-16..=1]",
        (-16..=1).contains(&resp.status),
        st
    );

    // ── 3. statusRro ──
    println!(
        "\n[3] statusRro (CheckRequest) → StatusResponse [док: open_shift/online/last_signer]"
    );
    let resp = client.status().await?;
    let st = StatusRroStatus::try_from(resp.status)
        .map(|s| s.as_str_name())
        .unwrap_or("UNKNOWN");
    println!(
        "    status={} ({}), error_message={:?}",
        resp.status, st, resp.error_message
    );
    check!("statusRro: відповідь отримана", true, st);
    check!(
        "statusRro: статус у документованому наборі",
        [-1, -2, -4, -13, -14, 1].contains(&resp.status),
        st
    );

    // ── 4. infoRro ──
    println!(
        "\n[4] infoRro (CheckRequest) → RroInfoResponse [док: status_rro/open_shift/name/addr...]"
    );
    let resp = client.info().await?;
    let st = InfoStatus::try_from(resp.status)
        .map(|s| s.as_str_name())
        .unwrap_or("UNKNOWN");
    println!("    status={} ({})", resp.status, st);
    check!("infoRro: відповідь отримана", true, st);
    check!(
        "infoRro: статус у документованому наборі",
        [-1, -2, -4, -13, -14, 1].contains(&resp.status),
        st
    );

    // ── 5. lastChk ──
    println!("\n[5] lastChk (CheckRequest) → CheckResponse [док: останній чек у data_sign]");
    let resp = client.last_chk().await?;
    let st = CheckStatus::try_from(resp.status)
        .map(|s| s.as_str_name())
        .unwrap_or("UNKNOWN");
    println!(
        "    status={} ({}), error_message={:?}",
        resp.status, st, resp.error_message
    );
    check!("lastChk: відповідь отримана", true, st);
    check!(
        "lastChk: статус у документованому діапазоні",
        (-16..=1).contains(&resp.status),
        st
    );

    // ── 6. delLastChk ──
    println!("\n[6] delLastChk (CheckRequest) → CheckResponse [док: тільки чек продажу, 1 раз]");
    let resp = client.del_last_chk().await?;
    let st = CheckStatus::try_from(resp.status)
        .map(|s| s.as_str_name())
        .unwrap_or("UNKNOWN");
    println!(
        "    status={} ({}), error_message={:?}",
        resp.status, st, resp.error_message
    );
    check!("delLastChk: відповідь отримана", true, st);
    check!(
        "delLastChk: статус у документованому діапазоні",
        (-16..=1).contains(&resp.status),
        st
    );

    // ── 7. delLastChkId ──
    println!("\n[7] delLastChkId (CheckRequestId) → CheckResponse [док: якщо ІД = останньому]");
    let resp = client
        .del_last_chk_id("00000000-0000-0000-0000-000000000000".to_string())
        .await?;
    let st = CheckStatus::try_from(resp.status)
        .map(|s| s.as_str_name())
        .unwrap_or("UNKNOWN");
    println!(
        "    status={} ({}), error_message={:?}",
        resp.status, st, resp.error_message
    );
    check!("delLastChkId: відповідь отримана", true, st);
    check!(
        "delLastChkId: статус у документованому діапазоні",
        (-16..=1).contains(&resp.status),
        st
    );

    // ── Підсумок ──
    println!("\n{}", "=".repeat(72));
    println!("ПІДСУМОК: passed={passed}, failed={failed}");
    if failed == 0 {
        println!("✅ ВСІ ПЕРЕВІРКИ ПРОЙДЕНО — протокол відповідає документації (на рівні з'єднання/proto/маршрутизації)");
    } else {
        println!("❌ Є ПРОВАЛЕНІ ПЕРЕВІРКИ");
    }
    println!("{}", "=".repeat(72));
    Ok(())
}
