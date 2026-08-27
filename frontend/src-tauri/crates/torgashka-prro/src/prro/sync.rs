//! Синхронізація офлайн-черги ПРРО — 1:1 Python
//! `backend/app/application/use_cases/prro/sync_offline_queue_use_case.py`.

use serde::Serialize;

use crate::crypto::PrroSigner;
use crate::proto::{Check, CheckType};
use crate::xml::XmlBuilder;

use super::chk_sender::ChkSender;
use super::queue::PrroOfflineQueue;
use super::repository::PrroRepository;
use super::shift::PrroShiftError;

/// Результат синхронізації — 1:1 dict Python `sync()`.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncResult {
    pub synced: u64,
    pub failed: u64,
    pub skipped: u64,
    pub total: u64,
    pub results: Vec<SyncItemResult>,
}

/// Результат передачі одного документа.
#[derive(Debug, Clone, Serialize)]
pub struct SyncItemResult {
    pub id: String,
    pub local_number: i64,
    pub check_type: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Use case синхронізації — replay pending/failed у порядку черги.
pub struct SyncOfflineQueueUseCase;

impl SyncOfflineQueueUseCase {
    /// Надсилає всі документи черги (pending/failed) по порядку — 1:1 Python.
    ///
    /// Кожен документ: повторно обгортається у <RQ>+<MAC>, підписується,
    /// надсилається send_chk; при status=1 → mark_sent, інакше → mark_failed
    /// з текстом помилки; при gRPC-виключенні → mark_failed (відкат: документ
    /// НЕ втрачається, статус failed + error зберігається).
    #[allow(clippy::too_many_arguments)]
    pub async fn sync(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        limit: u32,
    ) -> Result<SyncResult, PrroShiftError> {
        let pending = PrroOfflineQueue::get_pending(repo, limit)
            .await
            .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;

        let mut result = SyncResult {
            total: pending.len() as u64,
            ..Default::default()
        };

        if pending.is_empty() {
            return Ok(result);
        }

        for item in &pending {
            let mut entry = SyncItemResult {
                id: item.id.to_string(),
                local_number: item.local_number,
                check_type: item.check_type.clone(),
                status: "failed".to_string(),
                error: None,
            };

            // B2: відправляємо ПОВНИЙ підписаний check_sign as-is (ідемпотентність).
            // Документи, додані до B2 (check_sign=None), формуються рівно 1 раз
            // і фіксуються у черзі — повторні sync не переформовують (build_message
            // викликається не більше 1 разу на документ, NT/MAC не змінюються).
            let outcome = async {
                let signed_str: String = match &item.check_sign {
                    Some(cs) if !cs.is_empty() => cs.clone(),
                    _ => {
                        let message = xml_builder
                            .build_message(&item.xml_body, None, true)
                            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
                        let signed = signer
                            .sign(message.as_bytes())
                            .map_err(|e| PrroShiftError::new(e.to_string(), "SIGN_ERROR"))?;
                        let signed = String::from_utf8_lossy(&signed).into_owned();
                        PrroOfflineQueue::update_check_sign(repo, item.id, signed.clone())
                            .await
                            .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;
                        signed
                    }
                };
                let check = Check {
                    rro_fn: xml_builder.rro_fn().to_string(),
                    date_time: crate::grpc::check_date_time(),
                    check_sign: signed_str.into_bytes(),
                    local_number: item.local_number as i32,
                    check_type: check_type_code(&item.check_type),
                    id_offline: item.id_offline.clone().unwrap_or_default(), // B4
                    id_cancel: String::new(),
                };
                let response = sender
                    .send_chk(check)
                    .await
                    .map_err(|e| PrroShiftError::new(e.to_string(), "GRPC_ERROR"))?;
                Ok::<_, PrroShiftError>(response)
            }
            .await;

            match outcome {
                Ok(response) if response.status == 1 => {
                    PrroOfflineQueue::mark_sent(repo, item.id, None)
                        .await
                        .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;
                    // B1: оновлюємо last_mac зміни — наступний Check посилатиметься
                    // на хеш цього успішно відправленого документа (hash-ланцюжок).
                    if let Some(shift_id) = item.shift_id {
                        if let Some(mac) = &item.mac {
                            repo.update_shift_last_mac(shift_id, mac.clone())
                                .await
                                .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;
                        }
                    }
                    result.synced += 1;
                    entry.status = "sent".to_string();
                }
                Ok(response) => {
                    let error = if !response.error_message.is_empty() {
                        response.error_message.clone()
                    } else {
                        format!("status={}", response.status)
                    };
                    PrroOfflineQueue::mark_failed(repo, item.id, error.clone())
                        .await
                        .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;
                    result.failed += 1;
                    entry.error = Some(error);
                }
                Err(e) => {
                    PrroOfflineQueue::mark_failed(repo, item.id, e.to_string())
                        .await
                        .map_err(|err| PrroShiftError::new(err.to_string(), "QUEUE_ERROR"))?;
                    result.failed += 1;
                    entry.error = Some(e.to_string());
                }
            }
            result.results.push(entry);
        }

        // persist_builder_counters — 1:1 Python
        repo.set_setting(
            super::models::KEY_LAST_PACKET_ID,
            &xml_builder.last_packet_id().to_string(),
        )
        .await?;
        repo.set_setting(
            super::models::KEY_LAST_MAC_NUMBER,
            &xml_builder.last_mac_number().to_string(),
        )
        .await?;

        Ok(result)
    }
}

/// Тип чеку → gRPC enum — 1:1 Python `_PRRO_CHECK_TYPE_MAP`.
pub fn check_type_code(check_type: &str) -> i32 {
    match check_type {
        super::models::CHECK_TYPE_CHK => CheckType::Chk as i32,
        super::models::CHECK_TYPE_ZREPORT => CheckType::Zreport as i32,
        super::models::CHECK_TYPE_SERVICECHK => CheckType::Servicechk as i32,
        _ => CheckType::Chk as i32,
    }
}
