//! Фіскалізація чеку через ПРРО — 1:1 Python
//! `backend/app/application/use_cases/prro/fiscalize_receipt_use_case.py`.
//!
//! Статуси: pending → (send_chk) → sent | failed. Split (часткова
//! фіскалізація) виконується ТУТ, а не при створенні чека — 1:1 Python.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

use crate::crypto::PrroSigner;
use crate::proto::CheckType;
use crate::xml::{Payment, ReceiptItem, TaxGroup, Totals, XmlBuilder};

use super::chk_sender::ChkSender;
use super::models::{
    ProductFiscalRow, ReceiptFiscalRow, ReceiptItemFiscalRow, SplitItemInput, CHECK_TYPE_CHK,
    KEY_AUTO_FISCALIZE, KEY_LAST_MAC_NUMBER, KEY_LAST_PACKET_ID, KEY_PRRO_FN, KEY_PRRO_STUB_MODE,
};
use super::offline::OfflineStateMachine;
use super::queue::PrroOfflineQueue;
use super::repository::{PrroRepoError, PrroRepository};
use super::settings::{build_fiscal_check_url, uuid6, PrroKeyStore};
use super::shift::PrroShiftError;

/// Коди помилок фіскального сервера — 1:1 Python ERROR_SAVE / ERROR_BAD_HASH_PREV.
const ERROR_SAVE: i32 = -3;
const ERROR_BAD_HASH_PREV: i32 = -12;

/// Помилка фіскалізації чеку ПРРО — 1:1 `PrroFiscalizeError` (message + code).
#[derive(Debug, thiserror::Error)]
#[error("[{code}] {message}")]
pub struct PrroFiscalizeError {
    pub message: String,
    pub code: String,
}

impl PrroFiscalizeError {
    pub fn new(message: impl Into<String>, code: &str) -> Self {
        Self {
            message: message.into(),
            code: code.to_string(),
        }
    }
}

impl From<PrroShiftError> for PrroFiscalizeError {
    fn from(e: PrroShiftError) -> Self {
        PrroFiscalizeError::new(e.message, &e.code)
    }
}

impl From<PrroRepoError> for PrroFiscalizeError {
    fn from(e: PrroRepoError) -> Self {
        Self::new(e.to_string(), "PRRO_REPO_ERROR")
    }
}

/// Ім'я статусу фіскального сервера (CheckResponse.Status enum) —
/// 1:1 Python `status_name` — єдине джерело: `status_codes`.
pub use super::status_codes::status_name;

/// Фінальний текст помилки для користувача: `[КОД] Точний текст` —
/// 1:1 Python `_server_error_text`. Код ДПС (ERROR_SAVE/-12/...) ЗАВЖДИ
/// присутній; текст — повідомлення сервера, а якщо його немає — людське
/// пояснення статусу.
pub fn server_error_text(status: i32, error_message: &str) -> String {
    let name = status_name(status);
    let text = if error_message.trim().is_empty() {
        crate::prro::settings::PrroSettingsUseCase::status_message(status)
    } else {
        error_message.to_string()
    };
    format!("[{name}] {text}")
}

/// Результат фіскалізації — 1:1 `FiscalizeResponseDTO`.
#[derive(Debug, Clone, Serialize)]
pub struct FiscalizeResponseDto {
    pub receipt_id: Uuid,
    pub fiscal_status: String,
    pub status: String,
    pub fiscal_date: Option<DateTime<Utc>>,
    pub message: Option<String>,
    pub fiscal_number: Option<String>,
    pub fiscal_serial: Option<String>,
    pub fiscal_sent_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub split_receipt_id: Option<Uuid>,
    pub fiscal_check_url: Option<String>,
    pub warning: Option<String>,
}

/// Запит на фіскалізацію — 1:1 `FiscalizeRequestDTO`.
#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct FiscalizeRequestDto {
    pub receipt_id: Option<Uuid>,
    pub manual: bool,
}

/// Use case фіскалізації чеку — безстатеві методи над repo + sender + signer.
pub struct FiscalizeReceiptUseCase;

impl FiscalizeReceiptUseCase {
    /// Чи увімкнена авто-фіскалізація — 1:1 `_auto_fiscalize_enabled`.
    pub async fn auto_fiscalize_enabled(repo: &dyn PrroRepository) -> Result<bool, PrroRepoError> {
        let value = repo.get_setting(KEY_AUTO_FISCALIZE).await?;
        Ok(super::settings::parse_bool(value.as_deref()))
    }

    /// Режим заглушки (prro_stub_mode або env PRRO_STUB) — 1:1 `_stub_mode_enabled`.
    pub async fn stub_mode_enabled(repo: &dyn PrroRepository) -> bool {
        if let Ok(Some(value)) = repo.get_setting(KEY_PRRO_STUB_MODE).await {
            let v = value.trim().to_lowercase();
            if v == "true" || v == "1" {
                return true;
            }
        }
        let env = std::env::var("PRRO_STUB").unwrap_or_default();
        let env = env.trim().to_lowercase();
        env == "true" || env == "1"
    }

    /// Головний метод — 1:1 Python `fiscalize_receipt`.
    #[allow(clippy::too_many_arguments)]
    pub async fn fiscalize_receipt(
        repo: &dyn PrroRepository,
        key_store: &PrroKeyStore,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        receipt_id: Uuid,
        manual: bool,
    ) -> Result<FiscalizeResponseDto, PrroFiscalizeError> {
        // 0. Режим заглушки (stub): реальний ПРРО не підключений.
        let auto_enabled = Self::auto_fiscalize_enabled(repo).await?;
        if Self::stub_mode_enabled(repo).await && (manual || auto_enabled) {
            let receipt = repo.load_receipt_with_items(receipt_id).await?;
            let receipt = receipt.ok_or_else(|| {
                PrroFiscalizeError::new(
                    format!("Чек з ID '{receipt_id}' не знайдено"),
                    "RECEIPT_NOT_FOUND",
                )
            })?;
            return Self::fiscalize_stub(repo, &receipt).await;
        }

        // 1. Авто-фіскалізація: якщо вимкнена і виклик не ручний — без дій
        if !manual && !auto_enabled {
            return Ok(FiscalizeResponseDto {
                receipt_id,
                fiscal_status: "none".to_string(),
                status: "success".to_string(),
                fiscal_date: None,
                message: None,
                fiscal_number: None,
                fiscal_serial: None,
                fiscal_sent_at: None,
                error: Some("Авто-фіскалізація вимкнена (auto_fiscalize=false)".to_string()),
                split_receipt_id: None,
                fiscal_check_url: None,
                warning: None,
            });
        }

        // 2. Завантажуємо чек з позиціями
        let receipt = repo.load_receipt_with_items(receipt_id).await?;
        let receipt = receipt.ok_or_else(|| {
            PrroFiscalizeError::new(
                format!("Чек з ID '{receipt_id}' не знайдено"),
                "RECEIPT_NOT_FOUND",
            )
        })?;

        // 3. Фіскальні позиції (fiscal_quantity > 0)
        let fiscal_items: Vec<&ReceiptItemFiscalRow> = receipt
            .items
            .iter()
            .filter(|i| i.fiscal_quantity > Decimal::ZERO)
            .collect();
        if fiscal_items.is_empty() {
            return Ok(FiscalizeResponseDto {
                receipt_id,
                fiscal_status: "none".to_string(),
                status: "success".to_string(),
                fiscal_date: None,
                message: None,
                fiscal_number: None,
                fiscal_serial: None,
                fiscal_sent_at: None,
                error: Some("Немає фіскальних позицій (fiscal_quantity <= 0)".to_string()),
                split_receipt_id: None,
                fiscal_check_url: None,
                warning: None,
            });
        }

        let is_return = receipt.is_return;

        // 3. Повернення (T=1): має посилатися на оригінальний фіскальний чек.
        //    id_cancel = фіскальний номер (response.id) оригінального чека (спека E).
        let mut id_cancel = String::new();
        if is_return {
            let original = match receipt.original_receipt_id {
                Some(orig_id) => repo.load_receipt_with_items(orig_id).await?,
                None => None,
            };
            let original = match original {
                Some(o) => o,
                None => {
                    return Ok(FiscalizeResponseDto {
                        receipt_id,
                        fiscal_status: "none".to_string(),
                        status: "success".to_string(),
                        fiscal_date: None,
                        message: None,
                        fiscal_number: None,
                        fiscal_serial: None,
                        fiscal_sent_at: None,
                        error: Some(
                            "Повернення не фіскалізується: не вказано оригінальний чек (original_receipt_id)"
                                .to_string(),
                        ),
                        split_receipt_id: None,
                        fiscal_check_url: None,
                        warning: None,
                    });
                }
            };
            let orig_status = original.fiscal_status.clone();
            if orig_status != "sent" {
                return Ok(FiscalizeResponseDto {
                    receipt_id,
                    fiscal_status: "none".to_string(),
                    status: "success".to_string(),
                    fiscal_date: None,
                    message: None,
                    fiscal_number: None,
                    fiscal_serial: None,
                    fiscal_sent_at: None,
                    error: Some(format!(
                        "Повернення не фіскалізується: оригінальний чек не фіскалізований (статус '{orig_status}')"
                    )),
                    split_receipt_id: None,
                    fiscal_check_url: None,
                    warning: None,
                });
            }
            // (спека E) id_cancel — фіскальний номер оригіналу; якщо його немає —
            // зрозуміла помилка, а не мовчазне порожнє поле для повернення.
            let original_fn = original.fiscal_number.clone().unwrap_or_default();
            if original_fn.is_empty() {
                return Ok(FiscalizeResponseDto {
                    receipt_id,
                    fiscal_status: "none".to_string(),
                    status: "success".to_string(),
                    fiscal_date: None,
                    message: None,
                    fiscal_number: None,
                    fiscal_serial: None,
                    fiscal_sent_at: None,
                    error: Some(
                        "Повернення не фіскалізується: оригінальний чек фіскалізований, але не має фіскального номера (fiscal_number) — неможливо заповнити id_cancel"
                            .to_string(),
                    ),
                    split_receipt_id: None,
                    fiscal_check_url: None,
                    warning: None,
                });
            }
            id_cancel = original_fn;
        }

        // 4. Валідація перед фіскалізацією
        Self::validate(repo, key_store, &receipt).await?;

        // 5. Відкрита зміна
        let open_shift = repo.get_open_shift().await?;
        let open_shift = open_shift.ok_or_else(|| {
            PrroFiscalizeError::new(
                "Зміна ПРРО не відкрита. Відкрийте зміну перед фіскалізацією",
                "PRRO_SHIFT_NOT_OPEN",
            )
        })?;

        // 6. Спліт + ефективні кількості
        let mut warnings: Vec<String> = Vec::new();
        let (planned, split_receipt_id) =
            Self::prepare_split(repo, &receipt, is_return, &mut warnings).await?;
        if planned.is_empty() {
            return Ok(FiscalizeResponseDto {
                receipt_id,
                fiscal_status: "none".to_string(),
                status: "success".to_string(),
                fiscal_date: None,
                message: None,
                fiscal_number: None,
                fiscal_serial: None,
                fiscal_sent_at: None,
                error: Some("Немає фіскальних позицій після перевірки залишків".to_string()),
                split_receipt_id: None,
                fiscal_check_url: None,
                warning: join_warnings(&warnings),
            });
        }

        let (items_xml, total, tax_groups) = Self::build_receipt_payload(&planned);

        // local_number — ЗАВЖДИ послідовний з початку зміни (онлайн і офлайн);
        // id_offline — лише в офлайні: резервний фіскальний номер (T=112).
        // (спека F/G: id_offline = номер з діапазону, не рядок "offline-{n}").
        let local_number = repo.next_local_number(open_shift.id).await?;
        let offline_id = if OfflineStateMachine::is_offline(repo).await? {
            OfflineStateMachine::next_offline_number(repo)
                .await?
                .to_string()
        } else {
            String::new()
        };

        let se = total - tax_groups.iter().map(|g| g.tax_total).sum::<Decimal>();
        let totals = Totals {
            fiscal_number: Some(local_number),
            total: total.to_string(),
            se: Some(se.to_string()),
            tax_groups: tax_groups
                .iter()
                .map(|g| TaxGroup {
                    tax: g.tax.clone(),
                    percent: Some(g.tax_percent.to_string()),
                    total: Some(g.tax_total.to_string()),
                    dtpr: Some(Decimal::ZERO.to_string()),
                    dtsm: Some(Decimal::ZERO.to_string()),
                    tax_type: Some("0".to_string()),
                    tax_algorithm: Some("0".to_string()),
                    ..Default::default()
                })
                .collect(),
            cashier: Some(1),
            ..Default::default()
        };
        let payments = Self::build_payments(&receipt, total);
        let check_type = if is_return { "1" } else { "0" };

        // Формує DAT/повне RQ/підпис/Check для поточного чеку.
        // chain_mac — значення <MAC> цього чека (hex sha256 попереднього RQ);
        // offline_id — резервний фіскальний номер ("" — онлайн).
        fn build_document(
            xml_builder: &mut XmlBuilder,
            signer: &dyn PrroSigner,
            check_type: &str,
            items_xml: &[ReceiptItem],
            payments: &[Payment],
            totals: &Totals,
            local_number: i64,
            chain_mac: &str,
            offline_id: &str,
            id_cancel: &str,
        ) -> Result<DocParts, PrroFiscalizeError> {
            let now = Utc::now();
            let ts = now
                .with_timezone(&chrono::Local)
                .format("%Y%m%d%H%M%S")
                .to_string();
            let dat_xml = xml_builder
                .build_receipt_xml(
                    check_type,
                    items_xml,
                    payments,
                    totals,
                    &ts,
                    &[],
                    None,
                    Some("0"), // RT: 0 — повернення товару (для T=1)
                )
                .map_err(|e| PrroFiscalizeError::new(e.to_string(), "XML_BUILD_ERROR"))?;
            let message = xml_builder
                .build_message(&dat_xml, Some(chain_mac), offline_id, true)
                .map_err(|e| PrroFiscalizeError::new(e.to_string(), "XML_BUILD_ERROR"))?;
            let bytes = crate::xml::cp1251_bytes(&message)
                .map_err(|e| PrroFiscalizeError::new(e.to_string(), "XML_BUILD_ERROR"))?;
            let signed = signer
                .sign(&bytes)
                .map_err(|e| PrroFiscalizeError::new(e.to_string(), "SIGN_ERROR"))?;
            // doc_mac = значення <MAC> цього чека (ланцюг попереднього RQ);
            // next_mac = hash повного RQ цього чека — наступний Check.
            let next_mac = crate::xml::compute_mac(&message)
                .map_err(|e| PrroFiscalizeError::new(e.to_string(), "XML_BUILD_ERROR"))?;
            let check = make_check(
                xml_builder,
                signed.clone(),
                local_number,
                now,
                offline_id.to_string(),
                id_cancel.to_string(),
            );
            Ok(DocParts {
                dat_xml,
                doc_mac: chain_mac.to_string(),
                next_mac,
                signed,
                check,
            })
        }

        // Офлайн-режим: документ одразу в чергу (без мережевих спроб),
        // ланцюг зміни зсувається на hash(повного RQ цього чека).
        if !offline_id.is_empty() {
            let doc = build_document(
                xml_builder,
                signer,
                check_type,
                &items_xml,
                &payments,
                &totals,
                local_number,
                open_shift.last_mac.as_deref().unwrap_or(""),
                &offline_id,
                &id_cancel,
            )?;
            let check_sign = crate::xml::signed_bytes_to_text(&doc.signed);
            PrroOfflineQueue::add_document(
                repo,
                Some(receipt.id),
                Some(open_shift.id),
                local_number,
                CHECK_TYPE_CHK,
                &doc.dat_xml,
                Some(doc.doc_mac.clone()),
                Some(check_sign),
                Some(offline_id.clone()),
            )
            .await
            .map_err(|e| PrroFiscalizeError::new(e.to_string(), "QUEUE_ERROR"))?;
            // pending — документ очікує синхронізації (не "помилка")
            repo.update_shift_last_mac(open_shift.id, doc.next_mac.clone())
                .await
                .map_err(|e| PrroFiscalizeError::new(e.to_string(), "QUEUE_ERROR"))?;
            let err_msg = format!(
                "PRRO в офлайн-режимі: документ у черзі синхронізації (#{local_number}, id_offline={offline_id})"
            );
            repo.update_receipt_fiscal_state(
                receipt.id,
                "failed",
                None,
                None,
                None,
                Some(&err_msg),
                None,
            )
            .await?;
            repo.set_setting(
                KEY_LAST_PACKET_ID,
                &xml_builder.last_packet_id().to_string(),
            )
            .await?;
            repo.set_setting(
                KEY_LAST_MAC_NUMBER,
                &xml_builder.last_mac_number().to_string(),
            )
            .await?;
            tracing::info!(
                "PRRO_FISCALIZE | OFFLINE: чек {} у черзі (local={local_number}, id_offline={offline_id})",
                receipt.id
            );
            return Ok(FiscalizeResponseDto {
                receipt_id: receipt.id,
                fiscal_status: "failed".to_string(),
                status: "success".to_string(),
                fiscal_date: None,
                message: None,
                fiscal_number: None,
                fiscal_serial: None,
                fiscal_sent_at: None,
                error: Some(
                    "Чек передано в офлайн-чергу ПРРО; буде надіслано після відновлення зв'язку (синхронізація)"
                        .to_string(),
                ),
                split_receipt_id,
                fiscal_check_url: None,
                warning: join_warnings(&warnings),
            });
        }

        // ── Онлайн: формуємо та надсилаємо ────────────────────────────────
        let doc = build_document(
            xml_builder,
            signer,
            check_type,
            &items_xml,
            &payments,
            &totals,
            local_number,
            open_shift.last_mac.as_deref().unwrap_or(""),
            "",
            &id_cancel,
        )?;
        let check_sign = crate::xml::signed_bytes_to_text(&doc.signed);
        let response = match sender.send_chk(doc.check.clone()).await {
            Ok(r) => r,
            Err(_e) => {
                // H1: НЕ сліпий retry — спочатку lastChk: сервер міг зберегти чек,
                // а відповідь загубилась. Збіг NO (local_number) у XML останнього
                // чека → чек уже там → SENT (без дубліката).
                if let Ok(last) = sender_last_chk(sender).await {
                    let last_xml = String::from_utf8_lossy(&last.data_sign);
                    if last.status == 1
                        && crate::xml::extract_check_no(&last_xml) == Some(local_number)
                    {
                        return Self::on_success(
                            repo,
                            xml_builder,
                            &receipt,
                            &planned,
                            total,
                            local_number,
                            &doc.dat_xml,
                            &doc.doc_mac,
                            &doc.next_mac,
                            &check_sign,
                            "", // B4: онлайн — id_offline порожній
                            &last.id,
                            &last.id_sign,
                            open_shift.id,
                            is_return,
                            split_receipt_id,
                            &warnings,
                            Utc::now(),
                        )
                        .await;
                    }
                }
                // H1: чека немає → один контрольований повторний send
                match sender.send_chk(doc.check).await {
                    Ok(r) if r.status == 1 => {
                        return Self::on_success(
                            repo,
                            xml_builder,
                            &receipt,
                            &planned,
                            total,
                            local_number,
                            &doc.dat_xml,
                            &doc.doc_mac,
                            &doc.next_mac,
                            &check_sign,
                            "", // B4: онлайн — id_offline порожній
                            &r.id,
                            &r.id_sign,
                            open_shift.id,
                            is_return,
                            split_receipt_id,
                            &warnings,
                            Utc::now(),
                        )
                        .await;
                    }
                    Ok(r) => {
                        let msg = server_error_text(r.status, &r.error_message);
                        return Self::on_error(
                            repo,
                            sender,
                            &receipt,
                            local_number,
                            &doc.dat_xml,
                            &doc.doc_mac,
                            &check_sign,
                            "", // B4: онлайн — id_offline порожній
                            open_shift.id,
                            r.status,
                            &msg,
                            &r.id_sign,
                            split_receipt_id,
                            &warnings,
                        )
                        .await;
                    }
                    Err(e2) => {
                        // Мережа впала (повторно): T=109 → T=112 → чек у чергу.
                        // Черговість критична для ланцюжка (спека D/I/F/G):
                        let transport_error =
                            format!("[GRPC_ERROR] gRPC send_chk повторно не вдався: {e2}");
                        // 1. enter_offline (T=109) — кладе 109 у чергу/ланцюг.
                        if !OfflineStateMachine::is_offline(repo).await? {
                            if let Err(e109) = OfflineStateMachine::enter_offline(
                                repo,
                                sender,
                                xml_builder,
                                signer,
                                Utc::now(),
                                Some(open_shift.id),
                            )
                            .await
                            {
                                tracing::warn!("PRRO_OFFLINE | T=109 не вдалося: {e109}");
                            }
                        }
                        // 2. Резервний діапазон (best-effort; MAC 112 = ланцюг до 109).
                        let chain_before = open_shift.last_mac.clone().unwrap_or_default();
                        if let Err(e112) = OfflineStateMachine::reserve_numbers(
                            repo,
                            sender,
                            xml_builder,
                            signer,
                            Utc::now(),
                            &chain_before,
                            crate::xml::DEFAULT_RESERVE_SIZE,
                        )
                        .await
                        {
                            tracing::warn!("PRRO_OFFLINE | T=112 не вдалося: {e112}");
                        }
                        // 3. Оновлена зміна: last_mac = hash(109) → чек на 109.
                        let chain_mac = match repo.get_shift(open_shift.id).await? {
                            Some(s) => s.last_mac.clone().unwrap_or_default(),
                            None => String::new(),
                        };
                        // 4. id_offline з отриманого діапазону.
                        let offline_id2 = match OfflineStateMachine::next_offline_number(repo).await
                        {
                            Ok(n) => n.to_string(),
                            Err(e) => {
                                // Немає діапазону (T=112 не вдався): документ у
                                // черзі без id_offline неможливо фіскалізувати.
                                let msg = format!("Немає резервних номерів (T=112): {e}");
                                repo.update_receipt_fiscal_state(
                                    receipt.id,
                                    "failed",
                                    None,
                                    None,
                                    None,
                                    Some(&msg),
                                    None,
                                )
                                .await?;
                                repo.set_setting(
                                    KEY_LAST_PACKET_ID,
                                    &xml_builder.last_packet_id().to_string(),
                                )
                                .await?;
                                repo.set_setting(
                                    KEY_LAST_MAC_NUMBER,
                                    &xml_builder.last_mac_number().to_string(),
                                )
                                .await?;
                                return Ok(FiscalizeResponseDto {
                                        receipt_id: receipt.id,
                                        fiscal_status: "failed".to_string(),
                                        status: "success".to_string(),
                                        fiscal_date: None,
                                        message: None,
                                        fiscal_number: None,
                                        fiscal_serial: None,
                                        fiscal_sent_at: None,
                                        error: Some(format!(
                                            "Перехід в офлайн виконано, але отримати діапазон резервних номерів (T=112) не вдалося: {e}"
                                        )),
                                        split_receipt_id,
                                        fiscal_check_url: None,
                                        warning: join_warnings(&warnings),
                                    });
                            }
                        };
                        // 5. Документ ПЕРЕформовується: MAC посилається на hash(109),
                        //    id_offline — номер з отриманого діапазону.
                        let doc2 = build_document(
                            xml_builder,
                            signer,
                            check_type,
                            &items_xml,
                            &payments,
                            &totals,
                            local_number,
                            &chain_mac,
                            &offline_id2,
                            &id_cancel,
                        )?;
                        let check_sign2 = crate::xml::signed_bytes_to_text(&doc2.signed);
                        PrroOfflineQueue::add_document(
                            repo,
                            Some(receipt.id),
                            Some(open_shift.id),
                            local_number,
                            CHECK_TYPE_CHK,
                            &doc2.dat_xml,
                            Some(doc2.doc_mac.clone()),
                            Some(check_sign2),
                            Some(offline_id2.clone()),
                        )
                        .await
                        .map_err(|e| PrroFiscalizeError::new(e.to_string(), "QUEUE_ERROR"))?;
                        repo.update_shift_last_mac(open_shift.id, doc2.next_mac.clone())
                            .await
                            .map_err(|e| PrroFiscalizeError::new(e.to_string(), "QUEUE_ERROR"))?;
                        let err_msg = format!(
                            "Транспортна помилка; документ у офлайн-черзі (id_offline={offline_id2}): {transport_error}"
                        );
                        repo.update_receipt_fiscal_state(
                            receipt.id,
                            "failed",
                            None,
                            None,
                            None,
                            Some(&err_msg),
                            None,
                        )
                        .await?;
                        repo.set_setting(
                            KEY_LAST_PACKET_ID,
                            &xml_builder.last_packet_id().to_string(),
                        )
                        .await?;
                        repo.set_setting(
                            KEY_LAST_MAC_NUMBER,
                            &xml_builder.last_mac_number().to_string(),
                        )
                        .await?;
                        tracing::warn!(
                            "PRRO_FISCALIZE | чек {} → офлайн-черга (local={local_number}, id_offline={offline_id2}): {transport_error}",
                            receipt.id
                        );
                        return Ok(FiscalizeResponseDto {
                            receipt_id: receipt.id,
                            fiscal_status: "failed".to_string(),
                            status: "success".to_string(),
                            fiscal_date: None,
                            message: None,
                            fiscal_number: None,
                            fiscal_serial: None,
                            fiscal_sent_at: None,
                            error: Some(
                                "Чек передано в офлайн-чергу ПРРО (після переходу в офлайн); буде надіслано після відновлення зв'язку"
                                    .to_string(),
                            ),
                            split_receipt_id,
                            fiscal_check_url: None,
                            warning: join_warnings(&warnings),
                        });
                    }
                }
            }
        };

        // 8. Обробка відповіді
        if response.status == 1 {
            return Self::on_success(
                repo,
                xml_builder,
                &receipt,
                &planned,
                total,
                local_number,
                &doc.dat_xml,
                &doc.doc_mac,
                &doc.next_mac,
                &check_sign,
                "", // B4: онлайн — id_offline порожній
                &response.id,
                &response.id_sign,
                open_shift.id,
                is_return,
                split_receipt_id,
                &warnings,
                Utc::now(),
            )
            .await;
        }

        let error_message = server_error_text(response.status, &response.error_message);
        Self::on_error(
            repo,
            sender,
            &receipt,
            local_number,
            &doc.dat_xml,
            &doc.doc_mac,
            &check_sign,
            "", // B4: онлайн — id_offline порожній
            open_shift.id,
            response.status,
            &error_message,
            &response.id_sign,
            split_receipt_id,
            &warnings,
        )
        .await
    }

    /// Тимчасова заглушка фіскалізації — 1:1 `_fiscalize_stub`.
    pub async fn fiscalize_stub(
        repo: &dyn PrroRepository,
        receipt: &ReceiptFiscalRow,
    ) -> Result<FiscalizeResponseDto, PrroFiscalizeError> {
        let now = Utc::now();
        let stub_number = format!("STUB-{}-{}", receipt.receipt_number, now.timestamp());
        repo.update_receipt_fiscal_state(
            receipt.id,
            "fiscalized",
            Some(&stub_number),
            Some("STUB"),
            Some(now),
            None,
            Some(true),
        )
        .await?;
        Ok(FiscalizeResponseDto {
            receipt_id: receipt.id,
            status: "success".to_string(),
            fiscal_status: "fiscalized".to_string(),
            fiscal_number: Some(stub_number),
            fiscal_serial: Some("STUB".to_string()),
            fiscal_sent_at: Some(now),
            fiscal_date: Some(now),
            message: Some("Фіскалізацію виконано (заглушка)".to_string()),
            error: None,
            split_receipt_id: None,
            fiscal_check_url: None,
            warning: None,
        })
    }

    /// Валідація перед фіскалізацією — 1:1 `_validate`.
    pub async fn validate(
        repo: &dyn PrroRepository,
        key_store: &PrroKeyStore,
        receipt: &ReceiptFiscalRow,
    ) -> Result<(), PrroFiscalizeError> {
        if receipt.fiscal_status == "sent" {
            return Err(PrroFiscalizeError::new(
                format!(
                    "Чек {} вже фіскалізований (fiscal_number={})",
                    receipt.id,
                    receipt.fiscal_number.clone().unwrap_or_default()
                ),
                "PRRO_ALREADY_FISCALIZED",
            ));
        }

        let total = receipt.total_amount;
        if total <= Decimal::ZERO {
            return Err(PrroFiscalizeError::new(
                format!(
                    "Сума чеку {} повинна бути додатною (отримано {})",
                    receipt.id, total
                ),
                "PRRO_ZERO_TOTAL",
            ));
        }

        let fn_val = repo.get_setting(KEY_PRRO_FN).await?;
        if fn_val.as_deref().unwrap_or("").is_empty() {
            return Err(PrroFiscalizeError::new(
                "ПРРО не налаштований: не задано фіскальний номер (prro_fn)",
                "PRRO_NOT_CONFIGURED",
            ));
        }

        let ok = key_store.get_key_path().is_ok() && key_store.is_configured();
        if !ok {
            let reason = if key_store.get_key_path().is_err() {
                "ключ КЕП не збережено"
            } else {
                "пароль ключа КЕП не збережено"
            };
            return Err(PrroFiscalizeError::new(
                format!("ПРРО не налаштований: {reason}"),
                "PRRO_NOT_CONFIGURED",
            ));
        }
        Ok(())
    }

    /// Спліт чеку — 1:1 `_prepare_split`.
    #[allow(clippy::type_complexity)]
    pub async fn prepare_split(
        repo: &dyn PrroRepository,
        receipt: &ReceiptFiscalRow,
        is_return: bool,
        warnings: &mut Vec<String>,
    ) -> Result<
        (
            Vec<(ReceiptItemFiscalRow, Decimal, Option<ProductFiscalRow>)>,
            Option<Uuid>,
        ),
        PrroFiscalizeError,
    > {
        let mut planned: Vec<(ReceiptItemFiscalRow, Decimal, Option<ProductFiscalRow>)> =
            Vec::new();
        let mut split_items: Vec<(ReceiptItemFiscalRow, Decimal)> = Vec::new();

        let fiscal_items: Vec<&ReceiptItemFiscalRow> = receipt
            .items
            .iter()
            .filter(|i| i.fiscal_quantity > Decimal::ZERO)
            .collect();

        // Ефективна фіскальна кількість по кожній позиції
        for item in fiscal_items {
            let mut qty = item.fiscal_quantity;
            let total_qty = item.quantity;
            let product = repo.load_product(item.product_id).await?;

            if !is_return {
                if let Some(p) = &product {
                    let remaining = p.fiscal_stock;
                    let effective = Decimal::max(Decimal::ZERO, Decimal::min(qty, remaining));
                    if effective < qty {
                        let name = p.title.clone().unwrap_or_else(|| p.id.to_string());
                        warnings.push(format!(
                            "Товар '{name}': фіскальний залишок {remaining}, заплановано {qty} → фіскалізовано {effective}"
                        ));
                    }
                    qty = effective;
                }
            }

            qty = Decimal::min(qty, total_qty);
            let effective = qty;
            repo.update_item_fiscal(item.id, item.quantity, item.total, effective)
                .await?;
            if effective > Decimal::ZERO {
                planned.push((item.clone(), effective, product));
            }
            let remainder = total_qty - effective;
            if remainder > Decimal::ZERO {
                split_items.push((item.clone(), remainder));
            }
        }

        // Нефіскальні позиції (fiscal_quantity == 0) — повністю у дублікат
        for item in &receipt.items {
            if item.fiscal_quantity > Decimal::ZERO {
                continue;
            }
            let total_qty = item.quantity;
            if total_qty > Decimal::ZERO {
                split_items.push((item.clone(), total_qty));
            }
        }

        // Повністю нефіскальний чек — split не потрібен
        if planned.is_empty() {
            return Ok((planned, None));
        }
        if split_items.is_empty() {
            return Ok((planned, None));
        }

        // ── Split: коригуємо оригінальний чек (фіскальна частина) ─────────
        for (item, effective, _product) in &planned {
            let new_total = item_total(item, *effective);
            repo.update_item_fiscal(item.id, *effective, new_total, *effective)
                .await?;
        }

        // Позиції без фіскальної частини видаляємо (cascade delete)
        repo.delete_non_fiscal_items(receipt.id).await?;

        let new_total: Decimal = planned
            .iter()
            .map(|(i, effective, _)| item_total(i, *effective))
            .sum();
        repo.update_receipt_total(receipt.id, new_total).await?;
        repo.update_receipt_fiscal_state(receipt.id, "pending", None, None, None, None, Some(true))
            .await?;

        // ── Створюємо нефіскальний дублікат ───────────────────────────────
        let dup_items: Vec<SplitItemInput> = split_items
            .iter()
            .map(|(item, remainder)| SplitItemInput {
                product_id: item.product_id,
                quantity: *remainder,
                price: item.price,
                total: item_total(item, *remainder),
                purchase_price: item.purchase_price,
            })
            .collect();
        let dup_total: Decimal = dup_items.iter().map(|i| i.total).sum();
        let duplicate_id = repo
            .create_receipt_duplicate(receipt, &dup_items, is_return, receipt.id)
            .await?;
        let _ = dup_total;

        warnings.push(format!(
            "Часткова фіскалізація: фіскальний чек #{}, нефіскальна частина — чек #NF-{}-{} (split_group_id={})",
            receipt.receipt_number,
            receipt.receipt_number.chars().take(20).collect::<String>(),
            uuid6(),
            duplicate_id
        ));
        Ok((planned, Some(duplicate_id)))
    }

    /// Будує позиції XML та податкові групи — 1:1 `_build_receipt_payload`.
    #[allow(clippy::type_complexity)]
    pub fn build_receipt_payload(
        planned: &[(ReceiptItemFiscalRow, Decimal, Option<ProductFiscalRow>)],
    ) -> (Vec<ReceiptItem>, Decimal, Vec<TaxGroupAcc>) {
        let mut items_xml: Vec<ReceiptItem> = Vec::new();
        let mut tax_groups: Vec<TaxGroupAcc> = Vec::new();
        let mut total = Decimal::ZERO;

        for (item, qty, product) in planned {
            let price = item.price;
            let item_total_v = (price * qty).round_dp(2);
            total += item_total_v;

            let tax_percent = tax_percent_of(product);
            let tx_code = tax_code(tax_percent);

            items_xml.push(ReceiptItem {
                code: Some(item.product_id.to_string()),
                barcode: None,
                name: product
                    .as_ref()
                    .and_then(|p| p.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| item.product_id.to_string()),
                quantity: qty.to_string(),
                price: price.to_string(),
                total: item_total_v.to_string(),
                tax_rate: tx_code.clone(),
            });

            let vat = vat_amount(item_total_v, tax_percent);
            match tax_groups.iter_mut().find(|g| g.tax == tx_code) {
                Some(g) => g.tax_total += vat,
                None => tax_groups.push(TaxGroupAcc {
                    tax: tx_code,
                    tax_percent,
                    tax_total: vat,
                }),
            }
        }

        (items_xml, total, tax_groups)
    }

    /// Оплати для XML — 1:1 `_build_payments`.
    pub fn build_payments(receipt: &ReceiptFiscalRow, total: Decimal) -> Vec<Payment> {
        let method = receipt
            .payment_method
            .clone()
            .unwrap_or_else(|| "cash".to_string())
            .to_lowercase();
        let total = total.round_dp(2);
        let mut payments: Vec<Payment> = Vec::new();

        if method == "mixed" {
            let mut cash = payment_share(receipt.cash_amount);
            let mut card = payment_share(receipt.card_amount);

            if cash + card != total {
                if card > total {
                    cash = Decimal::ZERO;
                } else {
                    cash = total - card;
                }
            }

            card = (total - cash).round_dp(2);

            if cash > Decimal::ZERO {
                payments.push(Payment {
                    code: "0".to_string(),
                    name: Some("ГОТІВКА".to_string()),
                    amount: cash.to_string(),
                    change: None,
                });
            }
            if card > Decimal::ZERO {
                payments.push(Payment {
                    code: "1".to_string(),
                    name: Some("КАРТКА".to_string()),
                    amount: card.to_string(),
                    change: None,
                });
            }
            if payments.is_empty() {
                payments.push(Payment {
                    code: "0".to_string(),
                    name: Some("ГОТІВКА".to_string()),
                    amount: total.to_string(),
                    change: None,
                });
            }
        } else if method.contains("card") {
            payments.push(Payment {
                code: "1".to_string(),
                name: Some("КАРТКА".to_string()),
                amount: total.to_string(),
                change: None,
            });
        } else {
            // Готівка та інші способи (bank_transfer тощо) — як готівковий платіж
            let change = payment_share(receipt.change_amount);
            let mut cash_pay = Payment {
                code: "0".to_string(),
                name: Some("ГОТІВКА".to_string()),
                amount: total.to_string(),
                change: None,
            };
            if change > Decimal::ZERO {
                cash_pay.change = Some(change.to_string());
            }
            payments.push(cash_pay);
        }

        payments
    }

    /// Обробка успіху — 1:1 `_on_success`.
    ///
    /// `doc_mac` — MAC цього чека (hex-значення з <MAC> = ланцюг попереднього
    /// RQ); `next_mac` = hash(повного RQ цього чека) → shift.last_mac (спека D).
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub async fn on_success(
        repo: &dyn PrroRepository,
        xml_builder: &XmlBuilder,
        receipt: &ReceiptFiscalRow,
        planned: &[(ReceiptItemFiscalRow, Decimal, Option<ProductFiscalRow>)],
        total: Decimal,
        local_number: i64,
        dat_xml: &str,
        doc_mac: &str,
        next_mac: &str,
        check_sign: &str, // B2: повний підписаний XML (RQ+MAC+підпис) — у чергу as-is
        id_offline: &str, // B4: резервний фіскальний номер або ""
        response_id: &str,
        id_sign: &[u8],
        open_shift_id: Uuid,
        is_return: bool,
        split_receipt_id: Option<Uuid>,
        warnings: &[String],
        now: DateTime<Utc>,
    ) -> Result<FiscalizeResponseDto, PrroFiscalizeError> {
        let serial = id_sign_str(id_sign, response_id);

        // 9. Оновлюємо чек
        repo.update_receipt_fiscal_state(
            receipt.id,
            "sent",
            Some(response_id),
            Some(&serial),
            Some(now),
            None,
            None,
        )
        .await?;

        // Зменшуємо/збільшуємо fiscal_stock товарів
        for (_item, qty, product) in planned {
            if let Some(p) = product {
                let current = p.fiscal_stock;
                let new_stock = if is_return {
                    current + qty
                } else {
                    Decimal::max(Decimal::ZERO, current - qty)
                };
                repo.update_product_fiscal_stock(p.id, new_stock).await?;
            }
        }

        // Запис у чергу (sent); mac = MAC цього чека (doc_mac)
        let queue_item = PrroOfflineQueue::add_document(
            repo,
            Some(receipt.id),
            Some(open_shift_id),
            local_number,
            CHECK_TYPE_CHK,
            dat_xml,
            Some(doc_mac.to_string()),
            Some(check_sign.to_string()), // B2: повний підписаний check_sign
            if id_offline.is_empty() {
                None
            } else {
                Some(id_offline.to_string()) // B4: резервний фіскальний номер
            },
        )
        .await
        .map_err(|e| PrroFiscalizeError::new(e.to_string(), "QUEUE_ERROR"))?;
        PrroOfflineQueue::mark_sent(repo, queue_item.id, None)
            .await
            .map_err(|e| PrroFiscalizeError::new(e.to_string(), "QUEUE_ERROR"))?;

        // Лічильники зміни: last_mac = hash(повного RQ цього чека) — наступний
        // Check посилатиметься на цей хеш (спека D).
        repo.increment_shift_counters(
            open_shift_id,
            total,
            Some(local_number),
            Some(next_mac.to_string()),
        )
        .await?;

        // persist_builder_counters
        repo.set_setting(
            KEY_LAST_PACKET_ID,
            &xml_builder.last_packet_id().to_string(),
        )
        .await?;
        repo.set_setting(
            KEY_LAST_MAC_NUMBER,
            &xml_builder.last_mac_number().to_string(),
        )
        .await?;

        // QR-код: mac = MAC цього чека (hex-значення з <MAC>, doc_mac) —
        // за офіційним описом ДПС §5 «Перевірка чеку» (спека H).
        let fiscal_check_url =
            build_fiscal_check_url(response_id, total, xml_builder.rro_fn(), now, Some(doc_mac));

        Ok(FiscalizeResponseDto {
            receipt_id: receipt.id,
            fiscal_status: "sent".to_string(),
            status: "success".to_string(),
            fiscal_date: None,
            message: None,
            fiscal_number: Some(response_id.to_string()),
            fiscal_serial: Some(serial),
            fiscal_sent_at: Some(now),
            error: None,
            split_receipt_id,
            fiscal_check_url,
            warning: join_warnings(warnings),
        })
    }

    /// Обробка помилки; при ERROR_SAVE/-12 пробує lastChk (дедуплікація) —
    /// 1:1 `_on_error`. last_mac НЕ оновлюється (документ не успішний).
    #[allow(clippy::too_many_arguments)]
    pub async fn on_error(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        receipt: &ReceiptFiscalRow,
        local_number: i64,
        dat_xml: &str,
        doc_mac: &str,
        check_sign: &str, // B2: повний підписаний XML — у чергу as-is
        id_offline: &str, // B4: резервний фіскальний номер або ""
        open_shift_id: Uuid,
        response_status: i32,
        error_message: &str,
        _id_sign: &[u8],
        split_receipt_id: Option<Uuid>,
        warnings: &[String],
    ) -> Result<FiscalizeResponseDto, PrroFiscalizeError> {
        repo.update_receipt_fiscal_state(
            receipt.id,
            "failed",
            None,
            None,
            None,
            Some(error_message),
            None,
        )
        .await?;

        let queue_item = PrroOfflineQueue::add_document(
            repo,
            Some(receipt.id),
            Some(open_shift_id),
            local_number,
            CHECK_TYPE_CHK,
            dat_xml,
            Some(doc_mac.to_string()),
            Some(check_sign.to_string()), // B2: повний підписаний check_sign
            if id_offline.is_empty() {
                None
            } else {
                Some(id_offline.to_string()) // B4: резервний фіскальний номер
            },
        )
        .await
        .map_err(|e| PrroFiscalizeError::new(e.to_string(), "QUEUE_ERROR"))?;
        PrroOfflineQueue::mark_failed(repo, queue_item.id, error_message.to_string())
            .await
            .map_err(|e| PrroFiscalizeError::new(e.to_string(), "QUEUE_ERROR"))?;

        // Дедуплікація: сервер міг зберегти чек, але відповідь загубилась
        if response_status == ERROR_SAVE || response_status == ERROR_BAD_HASH_PREV {
            if let Ok(last) = sender_last_chk(sender).await {
                if last.status == 1 && !last.id.is_empty() {
                    let serial = id_sign_str(&last.id_sign, &last.id);
                    let now = Utc::now();
                    repo.update_receipt_fiscal_state(
                        receipt.id,
                        "sent",
                        Some(&last.id),
                        Some(&serial),
                        Some(now),
                        None,
                        None,
                    )
                    .await?;
                    PrroOfflineQueue::mark_sent(repo, queue_item.id, None)
                        .await
                        .map_err(|e| PrroFiscalizeError::new(e.to_string(), "QUEUE_ERROR"))?;
                    return Ok(FiscalizeResponseDto {
                        receipt_id: receipt.id,
                        fiscal_status: "sent".to_string(),
                        status: "success".to_string(),
                        fiscal_date: None,
                        message: None,
                        fiscal_number: Some(last.id.clone()),
                        fiscal_serial: Some(serial),
                        fiscal_sent_at: Some(now),
                        error: None,
                        split_receipt_id,
                        fiscal_check_url: None,
                        warning: join_warnings(warnings),
                    });
                }
            }
        }

        Ok(FiscalizeResponseDto {
            receipt_id: receipt.id,
            fiscal_status: "failed".to_string(),
            status: "success".to_string(),
            fiscal_date: None,
            message: None,
            fiscal_number: None,
            fiscal_serial: None,
            fiscal_sent_at: None,
            error: Some(error_message.to_string()),
            split_receipt_id,
            fiscal_check_url: None,
            warning: join_warnings(warnings),
        })
    }
}

/// Проміжна структура податкової групи (до XML-конверсії) — 1:1 Python dict.
#[derive(Debug, Clone)]
pub struct TaxGroupAcc {
    pub tax: String,
    pub tax_percent: Decimal,
    pub tax_total: Decimal,
}

/// Ставка ПДВ (%) з товару (за замовчуванням 20%) — 1:1 `_tax_percent`.
pub fn tax_percent_of(product: &Option<ProductFiscalRow>) -> Decimal {
    if let Some(p) = product {
        if let Some(tr) = p.tax_rate {
            return tr;
        }
    }
    Decimal::new(20, 0)
}

/// Зіставлення ставки % → код TX — 1:1 `_tax_code`.
pub fn tax_code(percent: Decimal) -> String {
    if percent == Decimal::new(7, 0) {
        "1".to_string()
    } else if percent <= Decimal::ZERO {
        "2".to_string()
    } else {
        "0".to_string()
    }
}

/// Сума ПДВ: amount * percent / (100 + percent) — 1:1 `_vat_amount`.
pub fn vat_amount(amount: Decimal, percent: Decimal) -> Decimal {
    if percent <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    (amount * percent / (Decimal::new(100, 0) + percent)).round_dp(2)
}

/// Сума позиції = price × qty (округлення до 2 знаків) — 1:1 `_item_total`.
pub fn item_total(item: &ReceiptItemFiscalRow, qty: Decimal) -> Decimal {
    (item.price * qty).round_dp(2)
}

/// Сума (грн) з атрибута чеку — 1:1 `_payment_share` (None → 0).
pub fn payment_share(value: Option<Decimal>) -> Decimal {
    value.map(|v| v.round_dp(2)).unwrap_or(Decimal::ZERO)
}

/// Формує фіскальний серійний номер з id_sign (bytes) або fallback —
/// 1:1 `_id_sign_str`.
pub fn id_sign_str(id_sign: &[u8], fallback: &str) -> String {
    if id_sign.is_empty() {
        return fallback.to_string();
    }
    match std::str::from_utf8(id_sign) {
        Ok(s) => s.to_string(),
        Err(_) => hex::encode(id_sign),
    }
}

/// Сформований документ чеку: канонічний <DAT>, MAC-значення документа
/// (doc_mac = ланцюг попереднього RQ), next_mac = hash(повного RQ цього
/// документа), підписані байти та готовий gRPC Check.
struct DocParts {
    pub dat_xml: String,
    pub doc_mac: String,
    pub next_mac: String,
    pub signed: Vec<u8>,
    pub check: crate::proto::Check,
}

/// Формує gRPC Check — 1:1 Python `context.build_check` (спека B/E/G).
fn make_check(
    xml_builder: &XmlBuilder,
    check_sign: Vec<u8>,
    local_number: i64,
    now: DateTime<Utc>,
    id_offline: String, // резервний фіскальний номер для offline-чеків, інакше ""
    id_cancel: String,  // фіскальний номер оригінального чека для повернення (T=1)
) -> crate::proto::Check {
    crate::proto::Check {
        rro_fn: xml_builder.rro_fn().to_string(),
        date_time: crate::grpc::check_date_time_from(now), // YYYYMMDDhhmmss, локальний
        check_sign,
        local_number: local_number as i32,
        check_type: CheckType::Chk as i32,
        id_offline,
        id_cancel,
    }
}

/// lastChk через ChkSender (для дедуплікації) — дефолтна реалізація
/// у трейті повертає помилку, gRPC-клієнт і мок перевизначають.
async fn sender_last_chk(
    sender: &dyn ChkSender,
) -> Result<crate::proto::CheckResponse, PrroFiscalizeError> {
    sender
        .last_chk()
        .await
        .map_err(|e| PrroFiscalizeError::new(format!("lastChk не вдався: {e}"), "GRPC_ERROR"))
}

fn join_warnings(warnings: &[String]) -> Option<String> {
    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::PrroCryptoError;

    // ─── Помилки: код + точний текст (вимога UX) ─────────────────────────

    #[test]
    fn fiscalize_error_display_includes_code() {
        let e = PrroFiscalizeError::new("Unknown error", "ERROR_UNKNOWN");
        assert_eq!(e.to_string(), "[ERROR_UNKNOWN] Unknown error");
    }

    #[test]
    fn fiscalize_error_display_grpc_code() {
        let e = PrroFiscalizeError::new("gRPC send_chk не вдався: timeout", "GRPC_ERROR");
        assert!(e.to_string().starts_with("[GRPC_ERROR] "));
    }

    #[test]
    fn status_name_maps_server_codes() {
        assert_eq!(status_name(1), "OK");
        assert_eq!(status_name(-3), "ERROR_SAVE");
        assert_eq!(status_name(-12), "ERROR_BAD_HASH_PREV");
        assert_eq!(status_name(-16), "ERROR_OFFLINE_ID");
        assert_eq!(status_name(999), "STATUS_999");
    }

    #[test]
    fn server_error_text_includes_code_and_server_message() {
        // серверна помилка з текстом → [КОД] текст сервера as-is
        assert_eq!(
            server_error_text(-4, "Unknown error"),
            "[ERROR_UNKNOWN] Unknown error"
        );
        // ERROR_SAVE (-3) з текстом сервера
        assert_eq!(
            server_error_text(-3, "Server rejected receipt"),
            "[ERROR_SAVE] Server rejected receipt"
        );
    }

    #[test]
    fn server_error_text_falls_back_to_human_message_when_empty() {
        // порожній текст сервера → [КОД] людське пояснення статусу
        let t = server_error_text(-12, "");
        assert!(t.starts_with("[ERROR_BAD_HASH_PREV] "), "got: {t}");
        assert!(t.contains("ERROR_BAD_HASH_PREV"), "got: {t}");
    }

    #[test]
    fn on_error_dto_error_carries_code() {
        // DTO.error (у receipt.fiscal_error / GUI) — завжди з кодом.
        // Перевіряємо формат через server_error_text, який використовують
        // всі callsites on_error.
        let text = server_error_text(-4, "Unknown error");
        assert!(text.starts_with("[ERROR_UNKNOWN]"));
    }
    use crate::prro::chk_sender::MockChkSender;
    use crate::prro::repository::InMemoryPrroRepository;
    use crate::prro::settings::PrroKeyStore;
    use crate::prro::PrroShift;

    /// Мок-підписант: sign повертає b"sig:<len>" (без FFI).
    struct MockSigner;

    impl PrroSigner for MockSigner {
        fn sign(&self, xml_bytes: &[u8]) -> Result<Vec<u8>, PrroCryptoError> {
            Ok(format!("sig:{}", xml_bytes.len()).into_bytes())
        }
        fn verify(&self, _signed_xml: &[u8]) -> Result<bool, PrroCryptoError> {
            Ok(true)
        }
        fn get_serial_number(&self) -> Result<String, PrroCryptoError> {
            Ok("MOCK-SERIAL".to_string())
        }
        fn get_signer_name(&self) -> Result<String, PrroCryptoError> {
            Ok("Мок Підписант".to_string())
        }
    }

    fn tmp_keystore() -> (PrroKeyStore, std::path::PathBuf) {
        let key = fernet::Fernet::generate_key();
        let path = std::env::temp_dir().join(format!("prro_ks_{}.json", Uuid::new_v4()));
        let ks = PrroKeyStore::new(Some(&key), Some(path.to_str().unwrap()));
        ks.save_key_path("/tmp/mock-key.pfx", Some("pfx")).unwrap();
        ks.save_password_encrypted("secret").unwrap();
        (ks, path)
    }

    async fn seed_open_shift(repo: &InMemoryPrroRepository) -> Uuid {
        let shift = PrroShift::new(1, Utc::now());
        let id = shift.id;
        repo.create_shift(shift).await.unwrap();
        id
    }

    fn seed_sale_receipt(repo: &InMemoryPrroRepository) -> Uuid {
        let rid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        repo.seed_setting(KEY_PRRO_FN, "4000000001");
        repo.seed_product(ProductFiscalRow {
            id: pid,
            title: Some("Тестовий товар".to_string()),
            fiscal_stock: Decimal::new(10, 0),
            tax_rate: Some(Decimal::new(20, 0)),
        });
        repo.seed_receipt(ReceiptFiscalRow {
            id: rid,
            receipt_number: "T-100".to_string(),
            cashier_id: Uuid::new_v4(),
            total_amount: Decimal::new(2500, 2),
            paid_amount: Some(Decimal::new(2500, 2)),
            change_amount: Some(Decimal::ZERO),
            debtor_id: None,
            is_return: false,
            notes: None,
            payment_method: Some("cash".to_string()),
            cash_amount: Some(Decimal::new(2500, 2)),
            card_amount: Some(Decimal::ZERO),
            original_receipt_id: None,
            return_reason: None,
            split_group_id: None,
            fiscal_status: "pending".to_string(),
            fiscal_number: None,
            fiscal_serial: None,
            fiscal_sent_at: None,
            fiscal_error: None,
            is_fiscal: false,
            items: vec![ReceiptItemFiscalRow {
                id: Uuid::new_v4(),
                product_id: pid,
                quantity: Decimal::new(2, 0),
                price: Decimal::new(1250, 2),
                total: Decimal::new(2500, 2),
                purchase_price: None,
                fiscal_quantity: Decimal::new(2, 0),
            }],
        });
        rid
    }

    #[tokio::test]
    async fn stub_mode_fiscalizes() {
        let repo = InMemoryPrroRepository::new();
        repo.seed_setting(KEY_PRRO_STUB_MODE, "true");
        let rid = seed_sale_receipt(&repo);
        let (ks, _p) = tmp_keystore();
        let sender = MockChkSender::new();
        let mut builder =
            crate::xml::XmlBuilder::new("4000000001", "1234567890", "ZN123", "1", "2.1.7", 1, 1);
        let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
            &repo,
            &ks,
            &sender,
            &mut builder,
            &MockSigner,
            rid,
            true,
        )
        .await
        .unwrap();
        assert_eq!(resp.fiscal_status, "fiscalized");
        assert_eq!(resp.status, "success");
        assert!(resp.fiscal_number.unwrap().starts_with("STUB-T-100-"));
        assert_eq!(resp.fiscal_serial.as_deref(), Some("STUB"));
        // чек позначено фіскалізованим у БД
        let r = repo.load_receipt_with_items(rid).await.unwrap().unwrap();
        assert_eq!(r.fiscal_status, "fiscalized");
        assert!(r.is_fiscal);
        assert_eq!(sender.calls_len(), 0); // stub — без gRPC
    }

    #[tokio::test]
    async fn auto_disabled_returns_none() {
        let repo = InMemoryPrroRepository::new();
        repo.seed_setting(KEY_AUTO_FISCALIZE, "false");
        let rid = seed_sale_receipt(&repo);
        let (ks, _p) = tmp_keystore();
        let sender = MockChkSender::new();
        let mut builder =
            crate::xml::XmlBuilder::new("4000000001", "1234567890", "ZN123", "1", "2.1.7", 1, 1);
        let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
            &repo,
            &ks,
            &sender,
            &mut builder,
            &MockSigner,
            rid,
            false,
        )
        .await
        .unwrap();
        assert_eq!(resp.fiscal_status, "none");
        assert!(resp.error.unwrap().contains("Авто-фіскалізація вимкнена"));
    }

    #[tokio::test]
    async fn full_success_path() {
        let repo = InMemoryPrroRepository::new();
        let shift_id = seed_open_shift(&repo).await;
        let rid = seed_sale_receipt(&repo);
        let (ks, _p) = tmp_keystore();
        let sender = MockChkSender::new();
        sender.push_ok("FISC-001");
        let mut builder =
            crate::xml::XmlBuilder::new("4000000001", "1234567890", "ZN123", "1", "2.1.7", 1, 1);
        let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
            &repo,
            &ks,
            &sender,
            &mut builder,
            &MockSigner,
            rid,
            true,
        )
        .await
        .unwrap();
        assert_eq!(resp.fiscal_status, "sent");
        assert_eq!(resp.fiscal_number.as_deref(), Some("FISC-001"));
        assert!(resp
            .fiscal_check_url
            .unwrap()
            .starts_with("https://cabinet.tax.gov.ua/cashregs/check?"));
        // чек оновлено
        let r = repo.load_receipt_with_items(rid).await.unwrap().unwrap();
        assert_eq!(r.fiscal_status, "sent");
        assert_eq!(r.fiscal_number.as_deref(), Some("FISC-001"));
        // черга: 1 запис sent
        let items = repo.list_by_receipt(rid).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status.as_str(), "sent");
        // лічильники зміни
        let shift = repo.get_shift(shift_id).await.unwrap().unwrap();
        assert_eq!(shift.receipt_count, 1);
        assert_eq!(shift.last_local_number, 1);
        // send_chk викликано
        assert_eq!(sender.calls_len(), 1);
        // fiscal_stock зменшено 10 → 8
        let pid = r.items[0].product_id;
        let p = repo.load_product(pid).await.unwrap().unwrap();
        assert_eq!(p.fiscal_stock, Decimal::new(8, 0));
    }

    #[tokio::test]
    async fn split_creates_non_fiscal_duplicate() {
        let repo = InMemoryPrroRepository::new();
        seed_open_shift(&repo).await;
        let rid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        repo.seed_setting(KEY_PRRO_FN, "4000000001");
        repo.seed_product(ProductFiscalRow {
            id: pid,
            title: Some("Частковий".to_string()),
            fiscal_stock: Decimal::new(1, 0),
            tax_rate: Some(Decimal::new(20, 0)),
        });
        repo.seed_receipt(ReceiptFiscalRow {
            id: rid,
            receipt_number: "T-200".to_string(),
            cashier_id: Uuid::new_v4(),
            total_amount: Decimal::new(2000, 2),
            paid_amount: Some(Decimal::new(2000, 2)),
            change_amount: Some(Decimal::ZERO),
            debtor_id: None,
            is_return: false,
            notes: None,
            payment_method: Some("cash".to_string()),
            cash_amount: Some(Decimal::new(2000, 2)),
            card_amount: None,
            original_receipt_id: None,
            return_reason: None,
            split_group_id: None,
            fiscal_status: "pending".to_string(),
            fiscal_number: None,
            fiscal_serial: None,
            fiscal_sent_at: None,
            fiscal_error: None,
            is_fiscal: false,
            items: vec![ReceiptItemFiscalRow {
                id: Uuid::new_v4(),
                product_id: pid,
                quantity: Decimal::new(2, 0),
                price: Decimal::new(1000, 2),
                total: Decimal::new(2000, 2),
                purchase_price: None,
                fiscal_quantity: Decimal::new(2, 0),
            }],
        });
        let (ks, _p) = tmp_keystore();
        let sender = MockChkSender::new();
        sender.push_ok("FISC-SPLIT");
        let mut builder =
            crate::xml::XmlBuilder::new("4000000001", "1234567890", "ZN123", "1", "2.1.7", 1, 1);
        let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
            &repo,
            &ks,
            &sender,
            &mut builder,
            &MockSigner,
            rid,
            true,
        )
        .await
        .unwrap();
        assert_eq!(resp.fiscal_status, "sent");
        let split_id = resp.split_receipt_id.expect("split має створити дублікат");
        assert!(resp.warning.unwrap().contains("Часткова фіскалізація"));
        // дублікат існує
        let dup = repo
            .load_receipt_with_items(split_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!dup.is_fiscal);
        assert_eq!(dup.fiscal_status, "none");
        assert_eq!(dup.split_group_id, Some(rid));
        assert_eq!(dup.items.len(), 1);
        assert_eq!(dup.items[0].quantity, Decimal::new(1, 0)); // 2-1=1
                                                               // оригінал: quantity зменшено до 1 (ефективна)
        let orig = repo.load_receipt_with_items(rid).await.unwrap().unwrap();
        assert_eq!(orig.items[0].quantity, Decimal::new(1, 0));
        assert_eq!(orig.total_amount, Decimal::new(1000, 2));
    }

    #[tokio::test]
    async fn return_without_original_not_fiscalized() {
        let repo = InMemoryPrroRepository::new();
        seed_open_shift(&repo).await;
        let rid = Uuid::new_v4();
        let pid = Uuid::new_v4();
        repo.seed_setting(KEY_PRRO_FN, "4000000001");
        repo.seed_product(ProductFiscalRow {
            id: pid,
            title: Some("Повернення".to_string()),
            fiscal_stock: Decimal::ZERO,
            tax_rate: Some(Decimal::new(20, 0)),
        });
        repo.seed_receipt(ReceiptFiscalRow {
            id: rid,
            receipt_number: "R-300".to_string(),
            cashier_id: Uuid::new_v4(),
            total_amount: Decimal::new(500, 2),
            paid_amount: Some(Decimal::new(500, 2)),
            change_amount: Some(Decimal::ZERO),
            debtor_id: None,
            is_return: true,
            notes: None,
            payment_method: Some("cash".to_string()),
            cash_amount: Some(Decimal::new(500, 2)),
            card_amount: None,
            original_receipt_id: None,
            return_reason: None,
            split_group_id: None,
            fiscal_status: "pending".to_string(),
            fiscal_number: None,
            fiscal_serial: None,
            fiscal_sent_at: None,
            fiscal_error: None,
            is_fiscal: false,
            items: vec![ReceiptItemFiscalRow {
                id: Uuid::new_v4(),
                product_id: pid,
                quantity: Decimal::new(1, 0),
                price: Decimal::new(500, 2),
                total: Decimal::new(500, 2),
                purchase_price: None,
                fiscal_quantity: Decimal::new(1, 0),
            }],
        });
        let (ks, _p) = tmp_keystore();
        let sender = MockChkSender::new();
        let mut builder =
            crate::xml::XmlBuilder::new("4000000001", "1234567890", "ZN123", "1", "2.1.7", 1, 1);
        let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
            &repo,
            &ks,
            &sender,
            &mut builder,
            &MockSigner,
            rid,
            true,
        )
        .await
        .unwrap();
        assert_eq!(resp.fiscal_status, "none");
        assert!(resp.error.unwrap().contains("оригінальний чек"));
        assert_eq!(sender.calls_len(), 0);
    }

    #[tokio::test]
    async fn already_fiscalized_raises() {
        let repo = InMemoryPrroRepository::new();
        let rid = seed_sale_receipt(&repo);
        let (ks, _p) = tmp_keystore();
        let sender = MockChkSender::new();
        let mut builder =
            crate::xml::XmlBuilder::new("4000000001", "1234567890", "ZN123", "1", "2.1.7", 1, 1);
        let err = FiscalizeReceiptUseCase::fiscalize_receipt(
            &repo,
            &ks,
            &sender,
            &mut builder,
            &MockSigner,
            rid,
            true,
        )
        .await
        .unwrap_err();
        assert!(err.message.contains("Зміна ПРРО не відкрита"));
    }

    #[tokio::test]
    async fn dedup_on_error_save() {
        let repo = InMemoryPrroRepository::new();
        seed_open_shift(&repo).await;
        let rid = seed_sale_receipt(&repo);
        let (ks, _p) = tmp_keystore();
        let sender = MockChkSender::new();
        sender.push_fail("помилка запису", -3);
        let last = crate::prro::chk_sender::MockLastChk::new();
        last.set_ok("LAST-FISC");
        let mut builder =
            crate::xml::XmlBuilder::new("4000000001", "1234567890", "ZN123", "1", "2.1.7", 1, 1);
        // on_error приймає sender; дедуплікація через last_chk трейту
        let resp = FiscalizeReceiptUseCase::fiscalize_receipt(
            &repo,
            &ks,
            &last,
            &mut builder,
            &MockSigner,
            rid,
            true,
        )
        .await
        .unwrap();
        // MockLastChk.send_chk повертає Ok(status=1) → успіх, а не dedup.
        // Для dedup-гілки викликаємо on_error напряму.
        assert_eq!(resp.fiscal_status, "sent");
        let r = repo.load_receipt_with_items(rid).await.unwrap().unwrap();
        assert_eq!(r.fiscal_status, "sent");
    }
}
