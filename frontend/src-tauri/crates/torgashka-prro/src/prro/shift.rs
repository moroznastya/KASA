//! Use case змін ПРРО: open_shift (T=108) / close_shift (Z-звіт) — 1:1 Python
//! `backend/app/application/use_cases/prro/shift_use_case.py`.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

use crate::crypto::PrroSigner;
use crate::proto::CheckType;
use crate::xml::{parse_receipt_xml_totals, ShiftData, ShiftPayment, TaxGroup, XmlBuilder};

use super::chk_sender::ChkSender;
use super::models::{
    PrroQueueStatus, PrroShift, CHECK_TYPE_CHK, CHECK_TYPE_SERVICECHK, CHECK_TYPE_ZREPORT,
    KEY_LAST_SHIFT_NUMBER,
};
use super::queue::PrroOfflineQueue;
use super::repository::{PrroRepoError, PrroRepository};

/// Помилка операції зі зміною ПРРО — 1:1 `PrroShiftError` (message + code).
#[derive(Debug, thiserror::Error)]
#[error("[{code}] {message}")]
pub struct PrroShiftError {
    pub message: String,
    pub code: String,
}

impl PrroShiftError {
    pub fn new(message: impl Into<String>, code: &str) -> Self {
        Self {
            message: message.into(),
            code: code.to_string(),
        }
    }
}

impl From<PrroRepoError> for PrroShiftError {
    fn from(e: PrroRepoError) -> Self {
        Self::new(e.to_string(), "PRRO_REPO_ERROR")
    }
}

/// DTO зміни — 1:1 `PrroShiftDTO` (для HTTP-відповідей).
#[derive(Debug, Clone, Serialize)]
pub struct PrroShiftDto {
    pub id: Uuid,
    pub shift_number: i64,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub signer_name: Option<String>,
    pub status: String,
    pub receipt_count: i64,
    pub total_amount: Decimal,
    pub zreport_number: Option<String>,
}

impl From<&PrroShift> for PrroShiftDto {
    fn from(s: &PrroShift) -> Self {
        Self {
            id: s.id,
            shift_number: s.shift_number,
            opened_at: s.opened_at,
            closed_at: s.closed_at,
            signer_name: s.signer_name.clone(),
            status: s.status.as_str().to_string(),
            receipt_count: s.receipt_count,
            total_amount: s.total_amount,
            zreport_number: s.zreport_number.clone(),
        }
    }
}

/// TS для XML СЗЗД — 1:1 Python `_fmt_datetime(datetime.utcnow())`.
pub fn ts_now(now: DateTime<Utc>) -> String {
    now.format("%Y%m%d%H%M%S").to_string()
}

/// Use case змін ПРРО — безстатеві методи над repo + sender + signer.
pub struct PrroShiftUseCase;

impl PrroShiftUseCase {
    /// Відкриває зміну: службовий чек T=108, local_number=0 — 1:1 Python.
    ///
    /// Кроки: (1) перевірка, що зміна не відкрита; (2) службовий чек T=108;
    /// (3) підписання; (4) send_chk (SERVICECHK); (5) при OK — PrroShift(open),
    /// запис у чергу (sent), збереження last_shift_number.
    #[allow(clippy::too_many_arguments)]
    pub async fn open_shift(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        now: Option<DateTime<Utc>>,
    ) -> Result<PrroShiftDto, PrroShiftError> {
        let now = now.unwrap_or_else(Utc::now);

        // 1. Зміна не має бути відкрита
        if let Some(open) = repo.get_open_shift().await? {
            return Err(PrroShiftError::new(
                format!(
                    "Зміна #{} вже відкрита ({}:{:02})",
                    open.shift_number,
                    open.opened_at.format("%Y-%m-%d %H"),
                    chrono::Timelike::minute(&open.opened_at)
                ),
                "SHIFT_ALREADY_OPEN",
            ));
        }

        // 2. Службовий чек T=108
        let dat_xml = xml_builder
            .build_service_check_xml("108", &ts_now(now))
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let message = xml_builder
            .build_message(&dat_xml, None, true)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let signed = signer
            .sign(message.as_bytes())
            .map_err(|e| PrroShiftError::new(e.to_string(), "SIGN_ERROR"))?;
        let mac = crate::xml::compute_mac(&dat_xml, None);

        // 3. Надсилаємо (SERVICECHK, local_number=0)
        // B2: signed (повний підписаний XML) зберігається у черзі as-is
        let check = make_check(
            xml_builder,
            signed.clone(),
            0,
            CheckType::Servicechk,
            now,
            String::new(),
        );
        let response = sender.send_chk(check).await.map_err(|e| {
            PrroShiftError::new(format!("gRPC send_chk не вдався: {e}"), "GRPC_ERROR")
        })?;

        if response.status != 1 {
            // Код + ім'я + людський опис ЗАВЖДИ; текст сервера — повністю
            // (1:1 Python shift_use_case.py; джерело мапи: status_codes).
            let error_msg = shift_status_error_text(response.status, &response.error_message);
            return Err(PrroShiftError::new(
                format!("Не вдалося відкрити зміну: {error_msg}"),
                "OPEN_SHIFT_FAILED",
            ));
        }

        // 4. Створюємо зміну
        let shift_number = next_shift_number(repo).await?;
        let mut shift = PrroShift::new(shift_number, now);
        shift.signer_serial = signer.get_serial_number().ok();
        shift.signer_name = signer.get_signer_name().ok();
        shift.last_mac = Some(mac.clone());
        repo.create_shift(shift.clone()).await?;
        repo.set_setting(KEY_LAST_SHIFT_NUMBER, &shift_number.to_string())
            .await?;

        // 5. Запис у чергу (успішно передано)
        let queue_item = PrroOfflineQueue::add_document(
            repo,
            None,
            Some(shift.id),
            0,
            CHECK_TYPE_SERVICECHK,
            &dat_xml,
            Some(mac),
            Some(String::from_utf8_lossy(&signed).into_owned()), // B2
            None, // B4: службові чеки (108/Z) — без id_offline
        )
        .await
        .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;
        PrroOfflineQueue::mark_sent(repo, queue_item.id, Some(now))
            .await
            .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;

        // persist_builder_counters — 1:1 Python (last_packet_id, last_mac_number)
        persist_builder_counters(repo, xml_builder).await?;

        Ok(PrroShiftDto::from(&shift))
    }

    /// Закриває зміну: Z-звіт, local_number=0 — 1:1 Python.
    #[allow(clippy::too_many_arguments)]
    pub async fn close_shift(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        comment: Option<String>,
        now: Option<DateTime<Utc>>,
    ) -> Result<PrroShiftDto, PrroShiftError> {
        let now = now.unwrap_or_else(Utc::now);

        let open_shift = repo.get_open_shift().await?;
        let Some(open_shift) = open_shift else {
            return Err(PrroShiftError::new(
                "Немає відкритої зміни ПРРО",
                "NO_OPEN_SHIFT",
            ));
        };

        // 2. Z-звіт з підсумками зміни (з фактично переданих чеків)
        let z_data = Self::build_zreport_data(repo, &open_shift).await?;
        let dat_xml = xml_builder
            .build_zreport_xml(&z_data, &ts_now(now))
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let message = xml_builder
            .build_message(&dat_xml, None, true)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let signed = signer
            .sign(message.as_bytes())
            .map_err(|e| PrroShiftError::new(e.to_string(), "SIGN_ERROR"))?;
        let mac = crate::xml::compute_mac(&dat_xml, None);

        // 3. Надсилаємо (ZREPORT, local_number=0)
        // B2: signed (повний підписаний XML) зберігається у черзі as-is
        let check = make_check(
            xml_builder,
            signed.clone(),
            0,
            CheckType::Zreport,
            now,
            String::new(),
        );
        let response = sender.send_chk(check).await.map_err(|e| {
            PrroShiftError::new(format!("gRPC send_chk не вдався: {e}"), "GRPC_ERROR")
        })?;

        if response.status != 1 {
            // Код + ім'я + людський опис ЗАВЖДИ; текст сервера — повністю
            // (1:1 Python shift_use_case.py; джерело мапи: status_codes).
            let error_msg = shift_status_error_text(response.status, &response.error_message);
            return Err(PrroShiftError::new(
                format!("Не вдалося закрити зміну: {error_msg}"),
                "CLOSE_SHIFT_FAILED",
            ));
        }

        // 4. Закриваємо зміну
        let closed = repo
            .close_shift(
                open_shift.id,
                now,
                comment.unwrap_or_else(|| "system".to_string()),
                response.id.clone(),
                signer.get_serial_number().ok(),
                signer.get_signer_name().ok(),
            )
            .await?
            .ok_or_else(|| PrroShiftError::new("Зміну не знайдено", "SHIFT_NOT_FOUND"))?;

        // Запис у чергу (Z-звіт успішно передано)
        let queue_item = PrroOfflineQueue::add_document(
            repo,
            None,
            Some(open_shift.id),
            0,
            CHECK_TYPE_ZREPORT,
            &dat_xml,
            Some(mac.clone()),
            Some(String::from_utf8_lossy(&signed).into_owned()), // B2
            None, // B4: службові чеки (108/Z) — без id_offline
        )
        .await
        .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;
        PrroOfflineQueue::mark_sent(repo, queue_item.id, Some(now))
            .await
            .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;

        // B1: last_mac = MAC(Z) — останній успішно відправлений документ зміни.
        repo.update_shift_last_mac(open_shift.id, mac)
            .await
            .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;

        persist_builder_counters(repo, xml_builder).await?;

        Ok(PrroShiftDto::from(&closed))
    }

    /// Підсумки зміни для Z-звіту — 1:1 Python `_build_zreport_data`.
    ///
    /// Розбирає XML лише тих чеків, що були ФАКТИЧНО передані на фіскальний
    /// сервер (queue status=sent, check_type=CHK, прив'язані до зміни).
    pub async fn build_zreport_data(
        repo: &dyn PrroRepository,
        shift: &PrroShift,
    ) -> Result<ShiftData, PrroShiftError> {
        let queue_items = repo.list_by_shift(shift.id).await?;
        let sent_checks: Vec<_> = queue_items
            .iter()
            .filter(|i| i.check_type == CHECK_TYPE_CHK && i.status == PrroQueueStatus::Sent)
            .collect();

        let mut sales_count = 0i64;
        let mut returns_count = 0i64;
        // code -> (name, smi, smo)
        let mut payments: Vec<(String, String, Decimal, Decimal)> = Vec::new();
        // tx_code -> (percent, in, out, smi)
        let mut taxes: Vec<(String, Decimal, Decimal, Decimal, Decimal)> = Vec::new();

        for item in &sent_checks {
            let parsed = match parse_receipt_xml_totals(&item.xml_body) {
                Ok(p) => p,
                Err(_) => continue, // Python: logger.warning + continue
            };

            let is_return = parsed.check_type == "1";
            if is_return {
                returns_count += 1;
            } else {
                sales_count += 1;
            }

            // Обороти за формами оплати (SMI — отримано, SMO — видано)
            for (code, amount) in &parsed.payments {
                if let Some(e) = payments.iter_mut().find(|(c, ..)| c == code) {
                    if is_return {
                        e.3 += *amount;
                    } else {
                        e.2 += *amount;
                    }
                } else {
                    payments.push((
                        code.clone(),
                        String::new(),
                        if is_return { Decimal::ZERO } else { *amount },
                        if is_return { *amount } else { Decimal::ZERO },
                    ));
                }
            }

            // ПДВ та обіг за податковими групами
            for (code, tax) in &parsed.taxes {
                if let Some(e) = taxes.iter_mut().find(|(c, ..)| c == code) {
                    e.4 += tax.smi;
                    if is_return {
                        e.3 += tax.tax_total; // out
                    } else {
                        e.2 += tax.tax_total; // in
                    }
                } else {
                    taxes.push((
                        code.clone(),
                        tax.percent,
                        if is_return {
                            Decimal::ZERO
                        } else {
                            tax.tax_total
                        }, // in
                        if is_return {
                            tax.tax_total
                        } else {
                            Decimal::ZERO
                        }, // out
                        tax.smi,
                    ));
                }
            }
        }

        let today = Utc::now().format("%Y%m%d").to_string();
        let mut tax_rows: Vec<TaxGroup> = Vec::new();
        taxes.sort_by(|a, b| a.0.cmp(&b.0));
        for (code, percent, tax_in, tax_out, smi) in taxes {
            tax_rows.push(TaxGroup {
                tax: code,
                percent: Some(
                    crate::xml::format_percent(&percent.to_string())
                        .unwrap_or_else(|_| percent.to_string()),
                ),
                total: None,
                dtpr: None,
                dtsm: None,
                tax_type: Some("0".to_string()),
                tax_algorithm: Some("0".to_string()),
                ts: Some(today.clone()),
                tax_in: Some(cents_str(tax_in)),
                tax_out: Some(cents_str(tax_out)),
                dti: None,
                dto: None,
                smi: Some(cents_str(smi)),
                smo: Some("0".to_string()),
            });
        }

        let mut payment_rows: Vec<ShiftPayment> = Vec::new();
        payments.sort_by(|a, b| a.0.cmp(&b.0));
        for (code, name, smi, smo) in payments {
            let name = if name.is_empty() {
                if code == "0" {
                    "ГОТІВКА".to_string()
                } else {
                    "КАРТКА".to_string()
                }
            } else {
                name
            };
            payment_rows.push(ShiftPayment {
                code,
                name: Some(name),
                smi: Some(cents_str(smi)),
                smo: Some(cents_str(smo)),
            });
        }

        // Якщо чеків не знайдено — лічильники зміни як fallback
        let sales_count = if sent_checks.is_empty() {
            shift.receipt_count
        } else {
            sales_count
        };

        Ok(ShiftData {
            shift_number: shift.shift_number,
            sales_count,
            returns_count,
            taxes: tax_rows,
            payments: payment_rows,
            cash_io: vec![],
            operations: None,
        })
    }

    /// Нагадування про відкриту зміну > 24 год — 1:1 Python `auto_reminder_check`.
    pub async fn auto_reminder_check(
        repo: &dyn PrroRepository,
        now: Option<DateTime<Utc>>,
    ) -> Result<Option<ReminderInfo>, PrroShiftError> {
        let now = now.unwrap_or_else(Utc::now);
        let Some(open) = repo.get_open_shift().await? else {
            return Ok(None);
        };
        let hours = (now - open.opened_at).num_minutes() as f64 / 60.0;
        if hours > 24.0 {
            return Ok(Some(ReminderInfo {
                warning: format!(
                    "Зміна #{} відкрита більше 24 годин ({:.1} год). Рекомендується закрити зміну (Z-звіт).",
                    open.shift_number, hours
                ),
                shift_open: true,
                hours_open: (hours * 10.0).round() / 10.0,
            }));
        }
        Ok(None)
    }

    /// Журнал змін з пагінацією — 1:1 Python `list_shifts`.
    pub async fn list_shifts(
        repo: &dyn PrroRepository,
        page: u32,
        size: u32,
    ) -> Result<(Vec<PrroShiftDto>, u64), PrroShiftError> {
        let (shifts, total) = repo.list_shifts(page.max(1), size.max(1)).await?;
        Ok((shifts.iter().map(PrroShiftDto::from).collect(), total))
    }
}

/// Результат `auto_reminder_check` — 1:1 dict Python.
#[derive(Debug, Clone, Serialize)]
pub struct ReminderInfo {
    pub warning: String,
    pub shift_open: bool,
    pub hours_open: f64,
}

/// Формує gRPC Check — 1:1 Python `context.build_check`.
fn make_check(
    xml_builder: &XmlBuilder,
    check_sign: Vec<u8>,
    local_number: i32,
    check_type: CheckType,
    now: DateTime<Utc>,
    id_offline: String,
) -> crate::proto::Check {
    crate::proto::Check {
        rro_fn: xml_builder.rro_fn().to_string(),
        date_time: crate::grpc::check_date_time_from(now),
        check_sign,
        local_number,
        check_type: check_type as i32,
        id_offline,
        id_cancel: String::new(),
    }
}

/// Наступний номер зміни: last_shift_number + 1 — 1:1 Python.
async fn next_shift_number(repo: &dyn PrroRepository) -> Result<i64, PrroShiftError> {
    let last_raw = repo.get_setting(KEY_LAST_SHIFT_NUMBER).await?;
    let last = last_raw.and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    Ok(last + 1)
}

/// Збереження лічильників XmlBuilder у налаштування — 1:1 Python
/// `context.persist_builder_counters`.
async fn persist_builder_counters(
    repo: &dyn PrroRepository,
    xml_builder: &XmlBuilder,
) -> Result<(), PrroShiftError> {
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
    Ok(())
}

/// Decimal грн → копійки (×100, ROUND_HALF_UP) — 1:1 Python `_to_cents`.
fn cents_str(value: Decimal) -> String {
    (value * Decimal::from(100)).round_dp(0).to_string()
}

/// Формує текст помилки зміни з кодом/ім'ям/описом ДПС.
///
/// Якщо текст сервера є — `"{error_message} | status=-13 (ERROR_...: опис)"`,
/// інакше — `"status=-13 (ERROR_...: опис)"`.
/// 1:1 Python `shift_use_case.py` (status_codes.status_error_text).
fn shift_status_error_text(status: i32, error_message: &str) -> String {
    let status_text = crate::prro::status_codes::status_error_text(status);
    if error_message.trim().is_empty() {
        status_text
    } else {
        format!("{} | {}", error_message.trim(), status_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_error_without_server_message_includes_name_and_description() {
        // Ключовий сценарій задачі: status=-13 НЕ доходить голим.
        let t = shift_status_error_text(-13, "");
        assert_eq!(
            t,
            "status=-13 (ERROR_NOT_REGISTERED_RRO: ПРРО не зареєстровано)"
        );
        assert!(t.contains("ERROR_NOT_REGISTERED_RRO"));
        assert!(t.contains("ПРРО не зареєстровано"));
    }

    #[test]
    fn shift_error_with_server_message_keeps_it_fully() {
        let t = shift_status_error_text(-3, "Server rejected receipt");
        assert!(t.starts_with("Server rejected receipt | "));
        assert!(t.contains("status=-3 (ERROR_SAVE:"));
        assert!(t.contains("ERROR_SAVE"));
    }

    #[test]
    fn shift_error_open_shift_wraps_status_text() {
        let msg = shift_status_error_text(-15, "");
        let full = format!("Не вдалося відкрити зміну: {msg}");
        assert!(full.contains("ERROR_NOT_OPEN_SHIFT"));
        assert!(full.contains("Зміну не відкрито"));
        assert!(!full.contains("status=-15)") && !full.ends_with("status=-15"));
    }
}
