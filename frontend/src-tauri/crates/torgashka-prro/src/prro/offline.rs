//! Офлайн-режим ПРРО: переходи 109/110, резервні номери 112, id_offline.
//! 1:1 Python `backend/app/infrastructure/services/prro/offline_state.py`.
//!
//! Протокол [ДПС]: службовий чек T=109 — перехід в офлайн, T=110 — в онлайн,
//! T=112 — запит діапазону резервних номерів (відповідь: `<CNF TY="C" FR=".."
//! TO=".."/>` у `data_sign` — СЗЗД 2.1.7, формат повідомлення від серверу).
//! Offline-чеки використовують local_number з резервного діапазону та
//! id_offline (не порожній).

use chrono::{DateTime, Utc};

use crate::crypto::PrroSigner;
use crate::proto::{Check, CheckType};
use crate::xml::XmlBuilder;

use super::chk_sender::ChkSender;
use super::models::{
    KEY_PRRO_OFFLINE, KEY_PRRO_OFFLINE_NEXT, KEY_PRRO_RESERVE_END, KEY_PRRO_RESERVE_START,
};
use super::repository::PrroRepository;
use super::shift::{ts_now, PrroShiftError};
use super::sync::{SyncOfflineQueueUseCase, SyncResult};

/// Дефолтний резервний діапазон, якщо сервер не відповів на T=112.
pub const DEFAULT_RESERVE_START: i64 = 1_000_000;
pub const DEFAULT_RESERVE_END: i64 = 1_000_999;

/// Формує службовий gRPC Check (T=108..112) — 1:1 Python `context.build_check`.
fn make_service_check(
    xml_builder: &XmlBuilder,
    check_sign: Vec<u8>,
    now: DateTime<Utc>,
    id_offline: String,
) -> Check {
    Check {
        rro_fn: xml_builder.rro_fn().to_string(),
        date_time: crate::grpc::check_date_time_from(now),
        check_sign,
        local_number: 0,
        check_type: CheckType::Servicechk as i32,
        id_offline,
        id_cancel: String::new(),
    }
}

/// Парсить резервний діапазон з XML-відповіді сервера (data_sign):
/// `<CNF TY="C" FR="1001" TO="1100"/>` — СЗЗД 2.1.7. None → дефолт.
pub fn parse_reserve_range(data_sign: &[u8]) -> Option<(i64, i64)> {
    let xml = std::str::from_utf8(data_sign).ok()?;
    let fr = xml.find("FR=\"")?;
    let rest = &xml[fr + 4..];
    let end_fr = rest.find('"')?;
    let start: i64 = rest[..end_fr].parse().ok()?;
    let to = rest.find("TO=\"")?;
    let rest2 = &rest[to + 4..];
    let end_to = rest2.find('"')?;
    let end: i64 = rest2[..end_to].parse().ok()?;
    if start < 1 || end < start {
        return None;
    }
    Some((start, end))
}

/// Державна машина офлайн-режиму ПРРО — безстатеві методи (1:1 Python).
pub struct OfflineStateMachine;

impl OfflineStateMachine {
    /// Поточний стан: чи ПРРО в офлайні (persist у налаштуваннях).
    pub async fn is_offline(repo: &dyn PrroRepository) -> Result<bool, PrroShiftError> {
        let v = repo.get_setting(KEY_PRRO_OFFLINE).await?;
        Ok(v.as_deref() == Some("1"))
    }

    /// ONLINE→OFFLINE: T=109. Надсилання — best-effort (мережа може бути
    /// вже мертва — помилка не блокує перехід, стан фіксується локально).
    pub async fn enter_offline(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        now: DateTime<Utc>,
    ) -> Result<(), PrroShiftError> {
        let dat_xml = xml_builder
            .build_service_check_xml("109", &ts_now(now))
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let message = xml_builder
            .build_message(&dat_xml, None, true)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let signed = signer
            .sign(message.as_bytes())
            .map_err(|e| PrroShiftError::new(e.to_string(), "SIGN_ERROR"))?;
        let check = make_service_check(xml_builder, signed, now, String::new());
        // Best-effort: транспортна помилка означає, що сервер недосяжний — це і є
        // причина переходу в офлайн. Документи з'являться в черзі при sync.
        if let Err(e) = sender.send_chk(check).await {
            tracing::warn!("PRRO_OFFLINE | T=109 не доставлено (мережа недоступна): {e}");
        }
        repo.set_setting(KEY_PRRO_OFFLINE, "1").await?;
        Ok(())
    }

    /// T=112: запит резервного діапазону номерів для offline-чеків.
    /// Повертає (start, end); діапазон persist у налаштуваннях.
    pub async fn reserve_numbers(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        now: DateTime<Utc>,
    ) -> Result<(i64, i64), PrroShiftError> {
        let dat_xml = xml_builder
            .build_service_check_xml("112", &ts_now(now))
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let message = xml_builder
            .build_message(&dat_xml, None, true)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let signed = signer
            .sign(message.as_bytes())
            .map_err(|e| PrroShiftError::new(e.to_string(), "SIGN_ERROR"))?;
        let check = make_service_check(xml_builder, signed, now, String::new());
        let response = sender
            .send_chk(check)
            .await
            .map_err(|e| PrroShiftError::new(format!("T=112 не вдався: {e}"), "GRPC_ERROR"))?;
        let range = parse_reserve_range(&response.data_sign)
            .unwrap_or((DEFAULT_RESERVE_START, DEFAULT_RESERVE_END));
        repo.set_setting(KEY_PRRO_RESERVE_START, &range.0.to_string())
            .await?;
        repo.set_setting(KEY_PRRO_RESERVE_END, &range.1.to_string())
            .await?;
        // Наступний вільний номер — з початку діапазону.
        repo.set_setting(KEY_PRRO_OFFLINE_NEXT, &range.0.to_string())
            .await?;
        Ok(range)
    }

    /// OFFLINE→ONLINE: T=110 → стан online → sync офлайн-черги.
    pub async fn exit_offline(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        limit: u32,
        now: DateTime<Utc>,
    ) -> Result<SyncResult, PrroShiftError> {
        let dat_xml = xml_builder
            .build_service_check_xml("110", &ts_now(now))
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let message = xml_builder
            .build_message(&dat_xml, None, true)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let signed = signer
            .sign(message.as_bytes())
            .map_err(|e| PrroShiftError::new(e.to_string(), "SIGN_ERROR"))?;
        let check = make_service_check(xml_builder, signed, now, String::new());
        // T=110 обов'язковий: без нього сервер не прийме offline-ланцюжок.
        sender
            .send_chk(check)
            .await
            .map_err(|e| PrroShiftError::new(format!("T=110 не вдався: {e}"), "GRPC_ERROR"))?;
        repo.set_setting(KEY_PRRO_OFFLINE, "0").await?;
        // Відправка накопичених offline-документів (ланцюжок, B1/B2).
        SyncOfflineQueueUseCase::sync(repo, sender, xml_builder, signer, limit).await
    }

    /// Наступний (local_number, id_offline) для offline-чека з резервного
    /// діапазону. id_offline — НЕ порожній: "offline-{local_number}".
    pub async fn next_offline_local(
        repo: &dyn PrroRepository,
    ) -> Result<(i64, String), PrroShiftError> {
        let start: i64 = repo
            .get_setting(KEY_PRRO_RESERVE_START)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_RESERVE_START);
        let end: i64 = repo
            .get_setting(KEY_PRRO_RESERVE_END)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_RESERVE_END);
        let next: i64 = repo
            .get_setting(KEY_PRRO_OFFLINE_NEXT)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(start);
        let n = next.min(end);
        repo.set_setting(KEY_PRRO_OFFLINE_NEXT, &(n + 1).to_string())
            .await?;
        Ok((n, format!("offline-{n}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reserve_range_from_cnf() {
        let xml = br#"<?xml version="1.0" encoding="windows-1251"?><RS V="1"><DAT><CNF TY="C" FR="1001" TO="1100" ER="0" TS="20260827120000"/></DAT></RS>"#;
        assert_eq!(parse_reserve_range(xml), Some((1001, 1100)));
    }

    #[test]
    fn parse_reserve_range_invalid_returns_none() {
        assert_eq!(parse_reserve_range(b"not xml"), None);
        assert_eq!(parse_reserve_range(br#"<CNF FR="abc" TO="100"/>"#), None);
        assert_eq!(
            parse_reserve_range(br#"<CNF FR="100" TO="50"/>"#),
            None,
            "end < start"
        );
    }
}
