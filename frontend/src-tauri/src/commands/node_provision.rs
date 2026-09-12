// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Tauri-команди провіжну вузла зі знімка хаба (ADR-0008 «Варіант B»)
// ─────────────────────────────────────────────────────────────────────────────
// Тонкий шар над ядром `torgashka_infrastructure::provision_from_hub`:
//   1. визначає ЦІЛЬОВУ БД вузла (своя або задана в env) — ядро відновлює саме
//      туди, ін'єкція параметром, а не «десь усередині»;
//   2. передає колбек прогресу, який `emit`-ить подію `node-provision:progress`
//      у фронтенд;
//   3. мапить результат ядра у ЗАМОРОЖЕНИЙ контрактом `ProvisionReport`
//      (camelCase, поля `class`/`stderrTail`/`hubUrl`/`dumpBytes`/`dumpSha256`).
//
// Чому мапінг окремим типом, а не `type ProvisionReport = HubProvisionOutcome`:
// фронтенд (контракт U1) споживає саме цю форму. Явний мапінг + серіалізаційний
// тест (`contract2_report_shape`) роблять розходження неможливим без падіння
// тесту, а не «домовленістю в голові».
// ─────────────────────────────────────────────────────────────────────────────

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;

use torgashka_infrastructure::provision_from_hub::{
    HubProvisionConfig, HubProvisionOutcome, HubProvisionStep,
};

/// Ім'я події прогресу (контракт R2; слухає фронтенд).
pub const PROGRESS_EVENT: &str = "node-provision:progress";

/// Крок звіту (camelCase; `detail` ніколи не порожній — див. ядро).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionStepDto {
    /// Назва кроку: `validate_url|hub_reachable|download|restore|configure`.
    pub step: String,
    /// Чи крок успішний.
    pub ok: bool,
    /// Подробиці (фактичні значення, коди виходу, хвіст виводу інструмента).
    pub detail: String,
}

/// Звіт провіжну — заморожена форма (контракт R2/U1).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionReport {
    /// Чи провіжн завершився успішно.
    pub ok: bool,
    /// `BadUrl|HubUnreachable|DownloadFailed|RestoreFailed|DbUnavailable` або `null`.
    pub class: Option<String>,
    /// Людське повідомлення українською (з фактичними джерелом/розміром/хабом).
    pub message: String,
    /// Хвіст stderr/stdout інструмента (`pg_restore`) — не губиться.
    pub stderr_tail: Option<String>,
    /// НОРМАЛІЗОВАНИЙ URL хаба, фактично записаний у БД вузла.
    pub hub_url: Option<String>,
    /// Джерело дампа: `hub` або `file`.
    pub source: Option<String>,
    /// Розмір використаного дампа, байт.
    pub dump_bytes: Option<u64>,
    /// sha256 фактично використаних байтів.
    pub dump_sha256: Option<String>,
    /// Кроки у порядку виконання.
    pub steps: Vec<ProvisionStepDto>,
}

impl From<HubProvisionStep> for ProvisionStepDto {
    fn from(step: HubProvisionStep) -> Self {
        Self {
            step: step.step,
            ok: step.ok,
            detail: step.detail,
        }
    }
}

impl From<HubProvisionOutcome> for ProvisionReport {
    fn from(outcome: HubProvisionOutcome) -> Self {
        Self {
            ok: outcome.ok,
            class: outcome.class,
            message: outcome.message,
            stderr_tail: outcome.stderr_tail,
            hub_url: outcome.hub_url,
            source: outcome.source,
            dump_bytes: outcome.dump_bytes,
            dump_sha256: outcome.dump_sha256,
            steps: outcome
                .steps
                .into_iter()
                .map(ProvisionStepDto::from)
                .collect(),
        }
    }
}

/// Визначає цільову БД вузла для відновлення.
///
/// Джерело — `DATABASE_URL` ПРОЦЕСУ (його ставить застосунок на старті: або
/// зовнішній URL з конфігурації, або власний embedded PG). Свідомо НЕ
/// використовується `db::resolve_database_url()`, який у дев-режимі дочитує
/// `backend/.env`: `pg_restore --clean` робить відновлення руйнівним для
/// наявних об'єктів, і «випадково відновити знімок хаба поверх робочої БД
/// розробника» — неприпустимий ризик. Немає `DATABASE_URL` → вузол має власну
/// БД, її піднімає ядро (`ensure_local_db = true`).
fn apply_target_db(cfg: &mut HubProvisionConfig) {
    match std::env::var("DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => {
            cfg.target_db_url = url.trim().to_string();
            cfg.ensure_local_db = false;
        }
        _ => {
            cfg.target_db_url = String::new();
            cfg.ensure_local_db = true;
        }
    }
}

/// Провіжн вузла: відновлення БД зі знімка хаба (мережа) АБО з локального
/// `.dump` (офлайн, «перший запуск із USB») + запис `sync.hub_url` і
/// `sync.hub_token` у ЦЮ БД.
///
/// `local_dump_path = Some(...)` вимикає мережеві кроки повністю: офлайн-провіжн
/// не потребує хаба взагалі.
#[tauri::command]
pub async fn provision_node_from_hub(
    app: AppHandle,
    hub_url: String,
    token: String,
    local_dump_path: Option<String>,
) -> Result<ProvisionReport, String> {
    let mut cfg = HubProvisionConfig::new(hub_url, token, local_dump_path.map(PathBuf::from));
    apply_target_db(&mut cfg);

    let emitter = app.clone();
    let outcome =
        torgashka_infrastructure::provision_from_hub::run(cfg, move |step, status, message| {
            let payload = serde_json::json!({
                "step": step,
                "status": status.as_str(),
                "message": message,
            });
            if let Err(e) = emitter.emit(PROGRESS_EVENT, payload) {
                // Подію не доставлено — це не привід валити провіжн: звіт усе одно
                // повернеться викликачу, а прогрес видно у steps[]. Логуємо.
                eprintln!("[node-provision] emit({PROGRESS_EVENT}) не вдався: {e}");
            }
        })
        .await;

    Ok(ProvisionReport::from(outcome))
}

/// Вибір файлу знімка БД (`*.dump`) системним діалогом — Rust-API
/// `tauri-plugin-dialog` (npm-пакета `@tauri-apps/plugin-dialog` у проєкті
/// немає, тож явна команда звільняє фронтенд від нової npm-залежності).
///
/// Діалог БЛОКУЮЧИЙ (нативне вікно): виконується у `spawn_blocking`, інакше він
/// заморозив би потік async-рантайму Tauri — разом із ним і решту команд та
/// HTTP-фасад застосунку.
#[tauri::command]
pub async fn pick_dump_file(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Виберіть знімок БД (pg_dump, *.dump)")
            .add_filter("PostgreSQL dump", &["dump"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("діалог вибору файла впав: {e}"))
    .map(|picked| {
        picked
            .and_then(|file| file.into_path().ok())
            .map(|path| path.to_string_lossy().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use torgashka_infrastructure::provision_from_hub::ProgressStatus;

    /// Контракт R2: форма звіту для фронтенду — camelCase, `class` як рядок,
    /// `steps[].step` несе заморожені назви. Тест ловить будь-яку зміну форми.
    #[test]
    fn contract2_report_shape_is_camel_case_and_frozen() {
        let outcome = HubProvisionOutcome {
            ok: false,
            class: Some("RestoreFailed".to_string()),
            message: "pg_restore завершився з кодом 1".to_string(),
            stderr_tail: Some("pg_restore: помилка".to_string()),
            hub_url: Some("http://hub.local:8080".to_string()),
            source: Some("file".to_string()),
            dump_bytes: Some(716_167),
            dump_sha256: Some("3d2d".to_string()),
            steps: vec![HubProvisionStep {
                step: "restore".to_string(),
                ok: false,
                detail: "код 1".to_string(),
            }],
        };
        let report = ProvisionReport::from(outcome);
        let json = serde_json::to_value(&report).expect("серіалізація звіту");
        for key in [
            "ok",
            "class",
            "message",
            "stderrTail",
            "hubUrl",
            "source",
            "dumpBytes",
            "dumpSha256",
            "steps",
        ] {
            assert!(json.get(key).is_some(), "у звіті немає поля {key}: {json}");
        }
        assert_eq!(json["class"], "RestoreFailed");
        assert_eq!(json["hubUrl"], "http://hub.local:8080");
        assert_eq!(json["dumpBytes"], 716_167);
        assert_eq!(json["steps"][0]["step"], "restore");
        assert_eq!(json["steps"][0]["ok"], false);
        // snake_case-ключів бути не повинно (фронтенд читає camelCase).
        assert!(json.get("hub_url").is_none());
        assert!(json.get("stderr_tail").is_none());
    }

    /// Подія прогресу: рівно три однословні ключі, значення статусу — сталі.
    #[test]
    fn progress_payload_has_frozen_keys_and_status_words() {
        let payload = serde_json::json!({
            "step": "download",
            "status": ProgressStatus::Ok.as_str(),
            "message": "знімок отримано",
        });
        assert_eq!(payload["step"], "download");
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["message"], "знімок отримано");
        assert_eq!(payload.as_object().map(|o| o.len()), Some(3));
        assert_eq!(PROGRESS_EVENT, "node-provision:progress");
    }
}
