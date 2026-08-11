// ─────────────────────────────────────────────────────────────────────────────
// torgashka-api — фіскальна гілка ПРРО (етап 7.3)
// ─────────────────────────────────────────────────────────────────────────────
// Реалізує РЕАЛЬНУ фіскалізацію через Rust (на відміну від локальних
// X/Z pos.rs-маршрутів): gRPC sendChkV2 + КЕП-підпис + офлайн-черга.
//
// Feature-flag TORGASHKA_RUST_PRRO:
//   "1"      — Rust ВИКОНУЄ open_shift/close_shift/sync (повна гілка)
//   "shadow" — Rust готує чек+підпис і ЛОГУЄ parity, Python виконує (проксі)
//   (відсутній/0) — статус-кво: все йде проксі на Python :8001
//
// Ключ КЕП: env PRRO_KEY_FILE (шлях) + PRRO_KEY_PASSWORD (plaintext).
// Python-сховище пароля (Fernet + PRRO_MASTER_KEY) — поза межами 7.3
// (задокументовано в RUST_MIGRATION_EXECUTION.md як обмеження).
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Json};
use serde::Deserialize;
use serde_json::json;
use torgashka_infrastructure::prro::SqlxPrroRepository;
use torgashka_prro::crypto::{signer_from_key_material, PrroSigner};
use torgashka_prro::grpc::{PrroGrpcClient, TlsConfig};
use torgashka_prro::keystore;
use torgashka_prro::prro::{
    FiscalizeReceiptUseCase, PrroFiscalizeError, PrroKeyStore, PrroOfflineQueue, PrroRepoError,
    PrroRepository, PrroSettingsDto, PrroSettingsError, PrroSettingsUseCase, PrroShiftDto,
    PrroShiftError, PrroShiftUseCase, SyncOfflineQueueUseCase, KEY_LAST_MAC_NUMBER,
    KEY_LAST_PACKET_ID, KEY_PRRO_FN, KEY_PRRO_MODE, KEY_PRRO_TN, KEY_PRRO_URL, KEY_PRRO_ZN,
};
use torgashka_prro::xml::XmlBuilder;

use crate::AppState;

/// Помилка фасаду ПРРО → HTTP 400/502.
#[derive(Debug, thiserror::Error)]
pub enum PrroApiError {
    #[error("ПРРО не налаштовано: {0}")]
    Config(String),
    #[error("операція ПРРО: {message}")]
    Shift {
        message: String,
        #[source]
        source: PrroShiftError,
    },
    #[error("репозиторій: {0}")]
    Repo(#[from] PrroRepoError),
    #[error("gRPC: {0}")]
    Grpc(#[from] torgashka_prro::grpc::PrroGrpcError),
    #[error("крипто: {0}")]
    Crypto(#[from] torgashka_prro::crypto::PrroCryptoError),
    #[error("ключ: {0}")]
    Key(#[from] torgashka_prro::keystore::KeyStoreError),
    #[error("XML: {0}")]
    Xml(#[from] torgashka_prro::xml::XmlBuilderError),
    #[error("черга: {0}")]
    Queue(#[from] torgashka_prro::prro::QueueError),
    #[error("{0}")]
    Settings(#[from] PrroSettingsError),
    #[error("{0}")]
    Fiscalize(#[from] PrroFiscalizeError),
}

impl From<PrroShiftError> for PrroApiError {
    fn from(source: PrroShiftError) -> Self {
        PrroApiError::Shift {
            message: source.message.clone(),
            source,
        }
    }
}

/// Контекст ПРРО: builder + signer + gRPC-клієнт (з налаштувань БД).
struct PrroContext {
    builder: XmlBuilder,
    signer: Box<dyn PrroSigner>,
    grpc: PrroGrpcClient,
}

/// Фасад фіскального ПРРО (Rust-гілка 7.3).
pub struct PrroFacade {
    repo: SqlxPrroRepository,
    /// shadow-режим: готуємо чек, але НЕ надсилаємо (Python виконує).
    shadow: bool,
    /// Сховище ключа КЕП (шлях/пароль) — 1:1 Python PrroKeyStore.
    key_store: PrroKeyStore,
}

impl PrroFacade {
    pub fn new(repo: SqlxPrroRepository, shadow: bool) -> Self {
        Self {
            repo,
            shadow,
            key_store: PrroKeyStore::default(),
        }
    }

    pub fn repo(&self) -> &SqlxPrroRepository {
        &self.repo
    }

    pub fn is_shadow(&self) -> bool {
        self.shadow
    }

    /// Будує контекст з налаштувань prro_settings + env PRRO_KEY_*.
    async fn context(&self) -> Result<PrroContext, PrroApiError> {
        let rro_fn = self
            .repo
            .get_setting(KEY_PRRO_FN)
            .await?
            .unwrap_or_default();
        let tax_number = self
            .repo
            .get_setting(KEY_PRRO_TN)
            .await?
            .unwrap_or_default();
        let factory_number = self
            .repo
            .get_setting(KEY_PRRO_ZN)
            .await?
            .unwrap_or_default();
        // URL: config env (PRRO_TEST_URL/PRRO_PROD_URL за mode) → БД → default.
        let mode = self
            .repo
            .get_setting(KEY_PRRO_MODE)
            .await?
            .filter(|m| !m.is_empty())
            .unwrap_or_else(torgashka_prro::prro::config_mode);
        let url = if let Ok(u) = std::env::var(if mode == "prod" {
            "PRRO_PROD_URL"
        } else {
            "PRRO_TEST_URL"
        }) {
            if !u.is_empty() {
                u
            } else {
                torgashka_prro::prro::config_url(&mode)
            }
        } else {
            self.repo
                .get_setting(KEY_PRRO_URL)
                .await?
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| torgashka_prro::prro::config_url(&mode))
        };
        let packet_id: i64 = self
            .repo
            .get_setting(KEY_LAST_PACKET_ID)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mac_number: i64 = self
            .repo
            .get_setting(KEY_LAST_MAC_NUMBER)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        if rro_fn.is_empty() || url.is_empty() {
            return Err(PrroApiError::Config(
                "налаштуйте ПРРО: prro_fn, prro_url (через /api/v2/prro/settings)".into(),
            ));
        }

        // Ключ КЕП: keystore (1:1 Python PrroKeyStore) → env PRRO_KEY_* fallback.
        let (key_file, key_password) = match (
            self.key_store.get_key_path(),
            self.key_store.decrypt_password(),
        ) {
            (Ok(kp), Ok(pw)) => (kp, pw),
            _ => {
                let kf = std::env::var("PRRO_KEY_FILE").ok();
                let kp = std::env::var("PRRO_KEY_PASSWORD").ok();
                let (Some(kf), Some(kp)) = (kf, kp) else {
                    return Err(PrroApiError::Config(
                        "налаштуйте ключ КЕП: завантажте його через PUT /api/v2/prro/settings                          (або задайте PRRO_KEY_FILE/PRRO_KEY_PASSWORD env для 7.3-сумісності)"
                            .into(),
                    ));
                };
                (kf, kp)
            }
        };

        let material =
            keystore::load_key_material(std::path::Path::new(&key_file), &key_password, None)?;
        let signer = signer_from_key_material(&material, &key_password)?;
        let grpc = PrroGrpcClient::connect(&url, TlsConfig::default(), &rro_fn).await?;
        let builder = XmlBuilder::new(
            rro_fn,
            tax_number,
            factory_number,
            "1",
            "2.1.7",
            packet_id,
            mac_number,
        );
        Ok(PrroContext {
            builder,
            signer,
            grpc,
        })
    }

    /// Відкриває зміну (T=108). У shadow-режимі — лише підготовка + лог.
    pub async fn open_shift(&self) -> Result<PrroShiftDto, PrroApiError> {
        let mut ctx = self.context().await?;
        if self.shadow {
            // shadow: Rust готує чек+підпис, Python виконує (parity-лог)
            let dat_xml = ctx.builder.build_service_check_xml("108", &ts_now())?;
            let message = ctx.builder.build_message(&dat_xml, None, true)?;
            let signed = ctx.signer.sign(message.as_bytes())?;
            eprintln!(
                "[torgashka-prro:shadow] open_shift готовий: dat_len={} signed_len={} di={}",
                dat_xml.len(),
                signed.len(),
                torgashka_prro::xml::extract_di(&dat_xml).unwrap_or_default()
            );
            return Err(PrroApiError::Config(
                "shadow-режим: запит виконує Python (проксі); Rust-підготовка залогована".into(),
            ));
        }
        PrroShiftUseCase::open_shift(
            &self.repo,
            &ctx.grpc,
            &mut ctx.builder,
            ctx.signer.as_ref(),
            None,
        )
        .await
        .map_err(Into::into)
    }

    /// Закриває зміну (Z-звіт).
    pub async fn close_shift(&self, comment: Option<String>) -> Result<PrroShiftDto, PrroApiError> {
        if self.shadow {
            return Err(PrroApiError::Config(
                "shadow-режим: close_shift виконує Python (проксі)".into(),
            ));
        }
        let mut ctx = self.context().await?;
        PrroShiftUseCase::close_shift(
            &self.repo,
            &ctx.grpc,
            &mut ctx.builder,
            ctx.signer.as_ref(),
            comment,
            None,
        )
        .await
        .map_err(Into::into)
    }

    /// Синхронізує офлайн-чергу (replay pending/failed).
    pub async fn sync(&self, limit: u32) -> Result<serde_json::Value, PrroApiError> {
        if self.shadow {
            let n = PrroOfflineQueue::count_pending(&self.repo).await?;
            eprintln!("[torgashka-prro:shadow] sync: pending={n} (Python виконує передачу)");
            return Ok(json!({"shadow": true, "pending": n, "note": "виконує Python (проксі)"}));
        }
        let mut ctx = self.context().await?;
        let result = SyncOfflineQueueUseCase::sync(
            &self.repo,
            &ctx.grpc,
            &mut ctx.builder,
            ctx.signer.as_ref(),
            limit,
        )
        .await
        .map_err(PrroApiError::from)?;
        Ok(serde_json::to_value(result).unwrap_or_else(|_| json!({"error": "serialize"})))
    }

    /// Журнал змін (з БД, без gRPC — 1:1 Python list_shifts).
    pub async fn list_shifts(
        &self,
        page: u32,
        size: u32,
    ) -> Result<serde_json::Value, PrroApiError> {
        let (items, total) = PrroShiftUseCase::list_shifts(&self.repo, page, size).await?;
        Ok(json!({
            "items": items.iter().map(|s| serde_json::to_value(s).unwrap_or_default()).collect::<Vec<_>>(),
            "total": total,
            "page": page,
            "size": size,
        }))
    }

    /// Журнал офлайн-черги (з БД — 1:1 Python get_queue спрощено).
    pub async fn queue(&self, limit: u32) -> Result<serde_json::Value, PrroApiError> {
        let pending = PrroOfflineQueue::get_pending(&self.repo, limit).await?;
        let expired = PrroOfflineQueue::get_expired(&self.repo, limit).await?;
        let items = pending
            .iter()
            .map(|i| {
                json!({
                    "id": i.id,
                    "local_number": i.local_number,
                    "check_type": i.check_type,
                    "status": i.status.as_str(),
                    "error": i.error,
                    "created_at": i.created_at,
                    "sent_at": i.sent_at,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "items": items,
            "pending": items.len(),
            "expired": expired.len(),
            "limit": limit,
        }))
    }

    /// Статус ПРРО: налаштування + лічильники черги (без gRPC statusRro).
    pub async fn status(&self) -> Result<serde_json::Value, PrroApiError> {
        let pending = PrroOfflineQueue::count_pending(&self.repo).await?;
        let open = self.repo.get_open_shift().await?;
        let last_shift: Option<String> = self.repo.get_setting("last_shift_number").await?;
        Ok(json!({
            "configured": !self.repo.get_setting(KEY_PRRO_FN).await?.unwrap_or_default().is_empty(),
            "queue_pending": pending,
            "open_shift": open.map(|s| json!({"id": s.id, "shift_number": s.shift_number, "opened_at": s.opened_at})),
            "last_shift_number": last_shift,
            "rust_gate": true,
        }))
    }

    // ─── Група 8/9: settings + test-connection + fiscalize ─────────────────

    /// GET /api/v2/prro/settings — 1:1 Python get_settings.
    pub async fn get_settings(&self) -> Result<PrroSettingsDto, PrroApiError> {
        let uc = PrroSettingsUseCase::new(self.key_store.clone());
        // _check_online: окремий gRPC-клієнт (без ключа — лише statusRro,
        // 1:1 Python _check_online через context.grpc_client()).
        let grpc = self.grpc_only().await.ok();
        Ok(uc.get_settings(&self.repo, grpc.as_ref()).await?)
    }

    /// PUT /api/v2/prro/settings — 1:1 Python save_settings (multipart form).
    #[allow(clippy::too_many_arguments)]
    /// gRPC-клієнт лише з налаштувань (без ключа) — для _check_online.
    async fn grpc_only(&self) -> Result<PrroGrpcClient, PrroApiError> {
        let mode = self
            .repo
            .get_setting(KEY_PRRO_MODE)
            .await?
            .filter(|m| !m.is_empty())
            .unwrap_or_else(torgashka_prro::prro::config_mode);
        let url = if let Ok(u) = std::env::var(if mode == "prod" {
            "PRRO_PROD_URL"
        } else {
            "PRRO_TEST_URL"
        }) {
            if !u.is_empty() {
                u
            } else {
                torgashka_prro::prro::config_url(&mode)
            }
        } else {
            self.repo
                .get_setting(KEY_PRRO_URL)
                .await?
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| torgashka_prro::prro::config_url(&mode))
        };
        let rro_fn = self
            .repo
            .get_setting(KEY_PRRO_FN)
            .await?
            .unwrap_or_default();
        PrroGrpcClient::connect(&url, torgashka_prro::grpc::TlsConfig::default(), &rro_fn)
            .await
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_settings(
        &self,
        key_file_content: Option<Vec<u8>>,
        key_file_name: Option<String>,
        key_file_path: Option<String>,
        key_password: Option<String>,
        prro_fn: Option<String>,
        prro_tn: Option<String>,
        prro_zn: Option<String>,
        mode: Option<String>,
        auto_fiscalize: Option<bool>,
    ) -> Result<PrroSettingsDto, PrroApiError> {
        let uc = PrroSettingsUseCase::new(self.key_store.clone());
        let grpc = self.grpc_only().await.ok();
        Ok(uc
            .save_settings(
                &self.repo,
                grpc.as_ref(),
                key_file_content.as_deref(),
                key_file_name.as_deref(),
                key_file_path.as_deref(),
                key_password.as_deref(),
                prro_fn.as_deref(),
                prro_tn.as_deref(),
                prro_zn.as_deref(),
                mode.as_deref(),
                auto_fiscalize,
            )
            .await?)
    }

    /// POST /api/v2/prro/test-connection — 1:1 Python test_connection (ping).
    pub async fn test_connection(&self) -> Result<serde_json::Value, PrroApiError> {
        let mut ctx = self.context().await?;
        let uc = PrroSettingsUseCase::new(self.key_store.clone());
        Ok(uc
            .test_connection(&ctx.grpc, &mut ctx.builder, Some(ctx.signer.as_ref()))
            .await)
    }

    /// POST /api/v2/prro/receipts/{id}/fiscalize — 1:1 Python fiscalize_receipt.
    pub async fn fiscalize(
        &self,
        receipt_id: uuid::Uuid,
        manual: bool,
    ) -> Result<PrroFiscalizeDtoOut, PrroApiError> {
        let mut ctx = self.context().await?;
        let dto = FiscalizeReceiptUseCase::fiscalize_receipt(
            &self.repo,
            &self.key_store,
            &ctx.grpc,
            &mut ctx.builder,
            ctx.signer.as_ref(),
            receipt_id,
            manual,
        )
        .await?;
        Ok(PrroFiscalizeDtoOut::from(dto))
    }
}

/// HTTP-відповідь фіскалізації (Serialize) — 1:1 FiscalizeResponseDTO.
#[derive(serde::Serialize)]
pub struct PrroFiscalizeDtoOut {
    pub receipt_id: uuid::Uuid,
    pub fiscal_status: String,
    pub status: String,
    pub fiscal_date: Option<chrono::DateTime<chrono::Utc>>,
    pub message: Option<String>,
    pub fiscal_number: Option<String>,
    pub fiscal_serial: Option<String>,
    pub fiscal_sent_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error: Option<String>,
    pub split_receipt_id: Option<uuid::Uuid>,
    pub fiscal_check_url: Option<String>,
    pub warning: Option<String>,
}

impl From<torgashka_prro::prro::FiscalizeResponseDto> for PrroFiscalizeDtoOut {
    fn from(d: torgashka_prro::prro::FiscalizeResponseDto) -> Self {
        Self {
            receipt_id: d.receipt_id,
            fiscal_status: d.fiscal_status,
            status: d.status,
            fiscal_date: d.fiscal_date,
            message: d.message,
            fiscal_number: d.fiscal_number,
            fiscal_serial: d.fiscal_serial,
            fiscal_sent_at: d.fiscal_sent_at,
            error: d.error,
            split_receipt_id: d.split_receipt_id,
            fiscal_check_url: d.fiscal_check_url,
            warning: d.warning,
        }
    }
}

fn ts_now() -> String {
    chrono::Utc::now().format("%Y%m%d%H%M%S").to_string()
}

// ─── Axum-хендлери ───────────────────────────────────────────────────────────
// Універсальний стиль (State + Request): тіло/query парсяться вручну, щоб
// коректно підтримувати опційне тіло (1:1 Python Optional body) і не
// залежати від extractor-комбінацій axum.

use axum::extract::Request;

/// Опційне тіло запиту: {"comment": "..."} (1:1 Python DTO).
#[derive(Deserialize, Default)]
pub struct ShiftBody {
    pub comment: Option<String>,
}

fn facade(state: &AppState) -> Result<Arc<PrroFacade>, (StatusCode, String)> {
    state.prro.clone().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Rust-гілка ПРРО вимкнена (TORGASHKA_RUST_PRRO=0)".to_string(),
        )
    })
}

/// Парсить query-параметр (page/size/limit) з URI.
fn query_param(req: &Request, key: &str) -> Option<String> {
    req.uri().query().and_then(|q| {
        q.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k == key).then(|| v.to_string())
        })
    })
}

fn page_q(req: &Request) -> (u32, u32) {
    let page = query_param(req, "page")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let size = query_param(req, "size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    (page, size)
}

fn limit_q(req: &Request) -> u32 {
    query_param(req, "limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .clamp(1, 500)
}

/// POST /api/v2/prro/fiscal/shift/open
pub async fn open_shift(
    State(state): State<AppState>,
    _req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    match f.open_shift().await {
        Ok(dto) => Ok(Json(serde_json::to_value(dto).unwrap_or_default())),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

/// POST /api/v2/prro/fiscal/shift/close (require_admin)
pub async fn close_shift(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<crate::auth::Claims>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    if crate::auth_routes::require_admin(&state, &claims)
        .await
        .is_err()
    {
        return Err((
            StatusCode::FORBIDDEN,
            "потрібні права адміністратора".to_string(),
        ));
    }
    // опційне тіло: {"comment": "..."} (1:1 Python CloseShiftRequestDTO)
    let body = axum::body::to_bytes(req.into_body(), 64 * 1024)
        .await
        .unwrap_or_default();
    let comment = if body.is_empty() {
        None
    } else {
        serde_json::from_slice::<ShiftBody>(&body)
            .ok()
            .and_then(|b| b.comment)
    };
    match f.close_shift(comment).await {
        Ok(dto) => Ok(Json(serde_json::to_value(dto).unwrap_or_default())),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

/// GET /api/v2/prro/fiscal/shifts
pub async fn list_shifts(
    State(state): State<AppState>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    let (page, size) = page_q(&req);
    f.list_shifts(page, size)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// POST /api/v2/prro/fiscal/sync
pub async fn sync_queue(
    State(state): State<AppState>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    f.sync(limit_q(&req))
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// GET /api/v2/prro/fiscal/queue
pub async fn queue(
    State(state): State<AppState>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    f.queue(limit_q(&req))
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// GET /api/v2/prro/fiscal/status
pub async fn status(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    f.status()
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

// ─── Група 8/9: settings + test-connection + fiscalize (TORGASHKA_RUST_PRRO_V2) ──

/// GET /api/v2/prro/settings
pub async fn settings_get(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    match f.get_settings().await {
        Ok(dto) => Ok(Json(serde_json::to_value(dto).unwrap_or_default())),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

/// PUT /api/v2/prro/settings (multipart: key_file + form-поля, require_admin)
pub async fn settings_put(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<crate::auth::Claims>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    if crate::auth_routes::require_admin(&state, &claims)
        .await
        .is_err()
    {
        return Err((
            StatusCode::FORBIDDEN,
            "потрібні права адміністратора".to_string(),
        ));
    }
    let mut key_file_content: Option<Vec<u8>> = None;
    let mut key_file_name: Option<String> = None;
    let mut key_file_path: Option<String> = None;
    let mut key_password: Option<String> = None;
    let mut prro_fn: Option<String> = None;
    let mut prro_tn: Option<String> = None;
    let mut prro_zn: Option<String> = None;
    let mut mode: Option<String> = None;
    let mut auto_fiscalize: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if field.file_name().is_some() {
            // файл ключа
            key_file_name = field.file_name().map(str::to_string);
            key_file_content = field.bytes().await.ok().map(|b| b.to_vec());
        } else {
            let text = field.text().await.unwrap_or_default();
            match name.as_str() {
                "key_password" => key_password = Some(text),
                "prro_fn" => prro_fn = Some(text),
                "prro_tn" => prro_tn = Some(text),
                "prro_zn" => prro_zn = Some(text),
                "mode" => mode = Some(text),
                "key_file_path" => key_file_path = Some(text),
                "auto_fiscalize" => auto_fiscalize = Some(text),
                _ => {}
            }
        }
    }

    let auto_bool = match auto_fiscalize.as_deref() {
        None => None,
        Some(v) => {
            let v = v.trim().to_lowercase();
            Some(matches!(v.as_str(), "1" | "true" | "yes" | "on"))
        }
    };

    match f
        .save_settings(
            key_file_content,
            key_file_name,
            key_file_path,
            key_password,
            prro_fn,
            prro_tn,
            prro_zn,
            mode,
            auto_bool,
        )
        .await
    {
        Ok(dto) => Ok(Json(serde_json::to_value(dto).unwrap_or_default())),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

/// POST /api/v2/prro/test-connection (require_admin)
pub async fn test_connection(
    State(state): State<AppState>,
    axum::Extension(claims): axum::Extension<crate::auth::Claims>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    if crate::auth_routes::require_admin(&state, &claims)
        .await
        .is_err()
    {
        return Err((
            StatusCode::FORBIDDEN,
            "потрібні права адміністратора".to_string(),
        ));
    }
    match f.test_connection().await {
        Ok(v) => Ok(Json(v)),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

/// POST /api/v2/prro/receipts/{receipt_id}/fiscalize
pub async fn fiscalize_receipt(
    State(state): State<AppState>,
    axum::extract::Path(receipt_id): axum::extract::Path<uuid::Uuid>,
    req: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let f = facade(&state)?;
    // Опційне тіло: {"manual": bool} (1:1 Python FiscalizeRequestDTO; відсутнє
    // тіло → manual=true — юзер натиснув кнопку).
    let body = axum::body::to_bytes(req.into_body(), 64 * 1024)
        .await
        .unwrap_or_default();
    let manual = if body.is_empty() {
        true
    } else {
        serde_json::from_slice::<torgashka_prro::prro::FiscalizeRequestDto>(&body)
            .map(|d| d.manual)
            .unwrap_or(true)
    };
    match f.fiscalize(receipt_id, manual).await {
        Ok(dto) => Ok(Json(serde_json::to_value(dto).unwrap_or_default())),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}
