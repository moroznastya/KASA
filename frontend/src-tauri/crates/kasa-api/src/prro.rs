// ─────────────────────────────────────────────────────────────────────────────
// kasa-api — фіскальна гілка ПРРО (етап 7.3)
// ─────────────────────────────────────────────────────────────────────────────
// Реалізує РЕАЛЬНУ фіскалізацію через Rust (на відміну від локальних
// X/Z pos.rs-маршрутів): gRPC sendChkV2 + КЕП-підпис + офлайн-черга.
//
// Feature-flag KASA_RUST_PRRO:
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
use kasa_infrastructure::prro::SqlxPrroRepository;
use kasa_prro::crypto::{signer_from_key_material, PrroSigner};
use kasa_prro::grpc::{PrroGrpcClient, TlsConfig};
use kasa_prro::keystore;
use kasa_prro::prro::{
    PrroOfflineQueue, PrroRepoError, PrroRepository, PrroShiftDto, PrroShiftError,
    PrroShiftUseCase, SyncOfflineQueueUseCase, KEY_LAST_MAC_NUMBER, KEY_LAST_PACKET_ID,
    KEY_PRRO_FN, KEY_PRRO_TN, KEY_PRRO_URL, KEY_PRRO_ZN,
};
use kasa_prro::xml::XmlBuilder;
use serde::Deserialize;
use serde_json::json;

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
    Grpc(#[from] kasa_prro::grpc::PrroGrpcError),
    #[error("крипто: {0}")]
    Crypto(#[from] kasa_prro::crypto::PrroCryptoError),
    #[error("ключ: {0}")]
    Key(#[from] kasa_prro::keystore::KeyStoreError),
    #[error("XML: {0}")]
    Xml(#[from] kasa_prro::xml::XmlBuilderError),
    #[error("черга: {0}")]
    Queue(#[from] kasa_prro::prro::QueueError),
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
}

impl PrroFacade {
    pub fn new(repo: SqlxPrroRepository, shadow: bool) -> Self {
        Self { repo, shadow }
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
        let url = self
            .repo
            .get_setting(KEY_PRRO_URL)
            .await?
            .unwrap_or_default();
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

        let key_file = std::env::var("PRRO_KEY_FILE").ok();
        let key_password = std::env::var("PRRO_KEY_PASSWORD").ok();
        let (Some(key_file), Some(key_password)) = (key_file, key_password) else {
            return Err(PrroApiError::Config(
                "задайте PRRO_KEY_FILE та PRRO_KEY_PASSWORD (env) для Rust-гілки ПРРО".into(),
            ));
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
                "[kasa-prro:shadow] open_shift готовий: dat_len={} signed_len={} di={}",
                dat_xml.len(),
                signed.len(),
                kasa_prro::xml::extract_di(&dat_xml).unwrap_or_default()
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
            eprintln!("[kasa-prro:shadow] sync: pending={n} (Python виконує передачу)");
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
            "Rust-гілка ПРРО вимкнена (KASA_RUST_PRRO=0)".to_string(),
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
