// ─────────────────────────────────────────────────────────────────────────────
// torgashka-api — bin/diff.rs: differential CLI
// ─────────────────────────────────────────────────────────────────────────────
// Приймає JSON зі stdin: {"op":"...","args":{...}}
// Викликає відповідну функцію крейту torgashka-api, повертає normalized JSON:
//   {"op":"...","ok":true|false,"result":...,"error":null|"..."}
//
// Операції (етап 0):
//   health  → torgashka_api::health_payload()   → {"status":"ok"}
//   echo    → torgashka_api::echo_payload(args) → args без змін
//
// Приклад:
//   echo '{"op":"echo","args":{"a":1}}' | cargo run -p torgashka-api --bin diff
// ─────────────────────────────────────────────────────────────────────────────

use serde::{Deserialize, Serialize};

/// Вхідний запит differential CLI.
#[derive(Debug, Deserialize)]
struct DiffRequest {
    op: String,
    #[serde(default)]
    args: serde_json::Value,
}

/// Normalized вихід differential CLI.
#[derive(Debug, Serialize)]
struct DiffResponse {
    op: String,
    ok: bool,
    result: serde_json::Value,
    error: Option<String>,
}

fn main() {
    let input = read_stdin();
    let response = match parse_request(&input) {
        Ok(req) => dispatch(&req),
        Err(e) => DiffResponse {
            op: "parse_error".to_string(),
            ok: false,
            result: serde_json::Value::Null,
            error: Some(e),
        },
    };
    println!(
        "{}",
        serde_json::to_string(&response).expect("відповідь завжди серіалізується")
    );
}

/// Читає весь stdin як рядок.
fn read_stdin() -> String {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .expect("помилка читання stdin");
    buf
}

/// Парсить вхідний JSON.
fn parse_request(input: &str) -> Result<DiffRequest, String> {
    serde_json::from_str(input).map_err(|e| format!("невалідний JSON: {e}"))
}

/// Диспетчеризує операцію на функцію крейту.
fn dispatch(req: &DiffRequest) -> DiffResponse {
    match req.op.as_str() {
        "health" => DiffResponse {
            op: req.op.clone(),
            ok: true,
            result: torgashka_api::health_payload(),
            error: None,
        },
        "echo" => DiffResponse {
            op: req.op.clone(),
            ok: true,
            result: torgashka_api::echo_payload(&req.args),
            error: None,
        },
        other => DiffResponse {
            op: req.op.clone(),
            ok: false,
            result: serde_json::Value::Null,
            error: Some(format!("невідома операція: {other}")),
        },
    }
}
