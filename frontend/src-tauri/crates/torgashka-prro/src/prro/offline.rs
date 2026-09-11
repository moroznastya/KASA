//! Офлайн-режим ПРРО: переходи 109/110, резервні номери 112, id_offline.
//! 1:1 Python `backend/app/infrastructure/services/prro/offline_state.py`
//! (спека prro_fix_spec_2026-09-08, F/I).
//!
//! Протокол [ДПС, 262576.docx]:
//!   - T=112 — запит діапазону резервних номерів:
//!     `<C T="112"><H SIZE="150"></H></C>`
//!     Відповідь сервера — перелік фіскальних номерів:
//!     `<RS V="1"><C T="112"><ID>aaaaaaaaaa</ID>…</C></RS>`
//!   - id_offline = фіскальний номер з отриманого діапазону (НЕ рядок
//!     "offline-{n}"); той самий номер іде в ID тега <MAC> офлайн-чека.
//!   - local_number офлайн-чеків = послідовний номер з початку зміни (як
//!     онлайн); резервний номер — лише в id_offline та ID тега <MAC>.
//!   - T=109 (перехід в офлайн) ОБОВ'ЯЗКОВО потрапляє в офлайн-чергу, щоб
//!     тимчасовий ланцюжок передавався до ФСКО після відновлення зв'язку.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::crypto::PrroSigner;
use crate::proto::{Check, CheckType};
use crate::xml::{self, XmlBuilder};

use super::chk_sender::ChkSender;
use super::models::{
    CHECK_TYPE_SERVICECHK, KEY_PRRO_OFFLINE, KEY_PRRO_OFFLINE_NEXT, KEY_PRRO_RESERVE_END,
    KEY_PRRO_RESERVE_START,
};
use super::queue::PrroOfflineQueue;
use super::repository::PrroRepository;
use super::shift::{ts_now, PrroShiftError};
use super::sync::SyncResult;

/// Формує службовий gRPC Check (T=108..112) — 1:1 Python `_make_service_check`.
fn make_service_check(xml_builder: &XmlBuilder, check_sign: Vec<u8>, now: DateTime<Utc>) -> Check {
    Check {
        rro_fn: xml_builder.rro_fn().to_string(),
        date_time: crate::grpc::check_date_time_from(now),
        check_sign,
        local_number: 0,
        check_type: CheckType::Servicechk as i32,
        id_offline: String::new(),
        id_cancel: String::new(),
    }
}

/// Парсить відповідь на T=112: перелік фіскальних номерів `<ID>`.
///
/// Формат (262576.docx):
/// `<RS V="1"><C T="112"><ID>aaaaaaaaaa</ID><ID>bbbbbbbbb</ID>…</C></RS>`
/// Прибирає модемний парсинг `<CNF FR TO>` (спека F).
pub fn parse_reserve_ids(data_sign: &[u8]) -> Vec<i64> {
    let xml = String::from_utf8_lossy(data_sign);
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE
        .get_or_init(|| regex::Regex::new(r"<ID>\s*(\d+)\s*</ID>").expect("валідний регекс <ID>"));
    re.captures_iter(&xml)
        .filter_map(|c| c.get(1))
        .filter_map(|m| m.as_str().parse::<i64>().ok())
        .filter(|x| *x >= 1)
        .collect()
}

/// Backward-сумісний аналог [`parse_reserve_ids`]: (start, end) або None.
/// За спекою F парсинг іде по `<ID>`-переліку (ПРРО), не по `<CNF FR TO>`.
pub fn parse_reserve_range(data_sign: &[u8]) -> Option<(i64, i64)> {
    let ids = parse_reserve_ids(data_sign);
    if ids.is_empty() {
        return None;
    }
    let min = ids.iter().copied().min()?;
    let max = ids.iter().copied().max()?;
    Some((min, max))
}

/// Державна машина офлайн-режиму ПРРО — безстатеві методи (1:1 Python).
pub struct OfflineStateMachine;

impl OfflineStateMachine {
    /// Поточний стан: чи ПРРО в офлайні (persist у налаштуваннях).
    pub async fn is_offline(repo: &dyn PrroRepository) -> Result<bool, PrroShiftError> {
        let v = repo.get_setting(KEY_PRRO_OFFLINE).await?;
        Ok(v.as_deref() == Some("1"))
    }

    /// Повертає наступний резервний фіскальний номер (або None, якщо діапазон
    /// не отримано/вичерпано). Номер споживається (лічильник зсувається).
    /// 1:1 Python `_next_reserve_number`.
    async fn next_reserve_number(repo: &dyn PrroRepository) -> Result<Option<i64>, PrroShiftError> {
        let start_raw = repo.get_setting(KEY_PRRO_RESERVE_START).await?;
        let end_raw = repo.get_setting(KEY_PRRO_RESERVE_END).await?;
        let next_raw = repo.get_setting(KEY_PRRO_OFFLINE_NEXT).await?;
        let (Some(start_raw), Some(end_raw)) = (start_raw, end_raw) else {
            return Ok(None);
        };
        let start: i64 = match start_raw.parse() {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let end: i64 = match end_raw.parse() {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let nxt: i64 = match next_raw {
            Some(r) => match r.parse() {
                Ok(v) => v,
                Err(_) => return Ok(None),
            },
            None => start,
        };
        if nxt < start || nxt > end {
            return Ok(None);
        }
        repo.set_setting(KEY_PRRO_OFFLINE_NEXT, &(nxt + 1).to_string())
            .await?;
        Ok(Some(nxt))
    }

    /// Наступний резервний фіскальний номер для офлайн-чека (id_offline).
    /// Помилка, якщо діапазон не отримано/вичерпано (треба T=112) — без
    /// фейкових дефолтів (спека F). 1:1 Python `next_offline_number`.
    pub async fn next_offline_number(repo: &dyn PrroRepository) -> Result<i64, PrroShiftError> {
        match Self::next_reserve_number(repo).await? {
            Some(n) => Ok(n),
            None => Err(PrroShiftError::new(
                "PRRO_OFFLINE | немає резервних фіскальних номерів: спочатку отримайте діапазон через T=112 (reserve_numbers)",
                "PRRO_OFFLINE_NO_RESERVE",
            )),
        }
    }

    /// ONLINE→OFFLINE: T=109 (спека D/I, 1:1 Python `enter_offline`).
    ///
    /// - Тіло 109 — `<C T="109"></C>`; ID тега <MAC> = резервний фіскальний
    ///   номер (якщо діапазон уже отримано), значення = поточний ланцюг
    ///   (shift.last_mac).
    /// - Документ НЕ втрачається: якщо доставка не вдалась — він кладеться
    ///   в офлайн-чергу (pending) і буде переданий після відновлення зв'язку.
    /// - Ланцюг: last_mac зміни = hash(повного RQ 109) — наступний офлайн-чек
    ///   посилається на цей хеш.
    pub async fn enter_offline(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        now: DateTime<Utc>,
        shift_id: Option<Uuid>,
    ) -> Result<(), PrroShiftError> {
        // Поточне значення ланцюга (shift.last_mac) → MAC документа 109
        let doc_mac = match shift_id {
            Some(sid) => repo
                .get_shift(sid)
                .await?
                .and_then(|s| s.last_mac.clone())
                .unwrap_or_default(),
            None => String::new(),
        };

        // Резервний фіскальний номер для ID тега <MAC> (якщо діапазон є)
        let offline_id = Self::next_reserve_number(repo).await?;
        let mac_id = offline_id.map(|n| n.to_string()).unwrap_or_default();

        let dat_xml = xml_builder
            .build_service_check_xml("109", &ts_now(now), 0)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let message = xml_builder
            .build_message(&dat_xml, Some(&doc_mac), &mac_id, true)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let signed = signer
            .sign(
                &xml::cp1251_bytes(&message)
                    .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?,
            )
            .map_err(|e| PrroShiftError::new(e.to_string(), "SIGN_ERROR"))?;
        let check = make_service_check(xml_builder, signed.clone(), now);

        let mut delivered = false;
        match sender.send_chk(check).await {
            Ok(r) => delivered = r.status == 1,
            Err(e) => {
                tracing::warn!("PRRO_OFFLINE | T=109 не доставлено: {e}");
            }
        }

        // 109 у черзі (для синку ланцюжка після відновлення зв'язку)
        let item = PrroOfflineQueue::add_document(
            repo,
            None,
            shift_id,
            0,
            CHECK_TYPE_SERVICECHK,
            &dat_xml,
            Some(doc_mac.clone()),
            Some(xml::signed_bytes_to_text(&signed)),
            if mac_id.is_empty() {
                None
            } else {
                Some(mac_id.clone())
            },
        )
        .await
        .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;
        if delivered {
            PrroOfflineQueue::mark_sent(repo, item.id, Some(now))
                .await
                .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;
        }

        // Ланцюг: наступний документ (офлайн-чек) посилається на хеш 109
        if let Some(sid) = shift_id {
            let next_mac = xml::compute_mac(&message)
                .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
            repo.update_shift_last_mac(sid, next_mac)
                .await
                .map_err(|e| PrroShiftError::new(e.to_string(), "QUEUE_ERROR"))?;
        }

        repo.set_setting(KEY_PRRO_OFFLINE, "1").await?;
        Ok(())
    }

    /// T=112: запит резервного діапазону номерів для offline-чеків (спека F).
    ///
    /// Запит: `<C T="112"><H SIZE="{reserve_size}"></H></C>`.
    /// Відповідь: перелік `<ID>…</ID>` (parse_reserve_ids).
    ///
    /// Помилка, якщо сервер не повернув жодного номера (без мовчазного
    /// фейкового дефолту — сервер його не реєстрував, і такі офлайн-чеки
    /// будуть відхилені з -16). 1:1 Python `reserve_numbers`.
    pub async fn reserve_numbers(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        now: DateTime<Utc>,
        mac_value: &str,
        reserve_size: i64,
    ) -> Result<(i64, i64), PrroShiftError> {
        let dat_xml = xml_builder
            .build_service_check_xml("112", &ts_now(now), reserve_size)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let message = xml_builder
            .build_message(&dat_xml, Some(mac_value), "", true)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let signed = signer
            .sign(
                &xml::cp1251_bytes(&message)
                    .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?,
            )
            .map_err(|e| PrroShiftError::new(e.to_string(), "SIGN_ERROR"))?;
        let check = make_service_check(xml_builder, signed, now);
        let response = sender.send_chk(check).await.map_err(|e| {
            PrroShiftError::new(format!("PRRO_OFFLINE | T=112 не вдався: {e}"), "GRPC_ERROR")
        })?;
        let ids = parse_reserve_ids(&response.data_sign);
        if ids.is_empty() {
            return Err(PrroShiftError::new(
                "PRRO_OFFLINE | T=112: сервер не повернув діапазон резервних номерів (<ID> відсутні у data_sign)",
                "PRRO_OFFLINE_NO_RESERVE_RANGE",
            ));
        }
        let start = *ids.iter().min().ok_or_else(|| {
            PrroShiftError::new(
                "T=112: порожній перелік ID",
                "PRRO_OFFLINE_NO_RESERVE_RANGE",
            )
        })?;
        let end = *ids.iter().max().ok_or_else(|| {
            PrroShiftError::new(
                "T=112: порожній перелік ID",
                "PRRO_OFFLINE_NO_RESERVE_RANGE",
            )
        })?;
        repo.set_setting(KEY_PRRO_RESERVE_START, &start.to_string())
            .await?;
        repo.set_setting(KEY_PRRO_RESERVE_END, &end.to_string())
            .await?;
        repo.set_setting(KEY_PRRO_OFFLINE_NEXT, &start.to_string())
            .await?;
        tracing::info!("PRRO_OFFLINE | резервний діапазон: {start}..{end}");
        Ok((start, end))
    }

    /// OFFLINE→ONLINE: T=110 → стан online → sync офлайн-черги.
    /// 1:1 Python `exit_offline` (MAC 110 = поточний ланцюг).
    pub async fn exit_offline(
        repo: &dyn PrroRepository,
        sender: &dyn ChkSender,
        xml_builder: &mut XmlBuilder,
        signer: &dyn PrroSigner,
        now: DateTime<Utc>,
        mac_value: &str,
        limit: u32,
    ) -> Result<SyncResult, PrroShiftError> {
        let dat_xml = xml_builder
            .build_service_check_xml("110", &ts_now(now), 0)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let message = xml_builder
            .build_message(&dat_xml, Some(mac_value), "", true)
            .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?;
        let signed = signer
            .sign(
                &xml::cp1251_bytes(&message)
                    .map_err(|e| PrroShiftError::new(e.to_string(), "XML_BUILD_ERROR"))?,
            )
            .map_err(|e| PrroShiftError::new(e.to_string(), "SIGN_ERROR"))?;
        let check = make_service_check(xml_builder, signed, now);
        // T=110 обов'язковий: без нього сервер не прийме offline-ланцюжок.
        sender.send_chk(check).await.map_err(|e| {
            PrroShiftError::new(format!("PRRO_OFFLINE | T=110 не вдався: {e}"), "GRPC_ERROR")
        })?;
        repo.set_setting(KEY_PRRO_OFFLINE, "0").await?;
        // Відправка накопичених offline-документів (ланцюг, D/G).
        super::sync::SyncOfflineQueueUseCase::sync(repo, sender, xml_builder, signer, limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reserve_ids_from_docx_sample() {
        // Зразок 262576.docx: <RS V="1"><C T="112"><ID>…</ID>…</C></RS>
        let xml = br#"<?xml version="1.0" encoding="windows-1251"?><RS V="1"><C T="112"><ID>100001</ID><ID>100002</ID><ID>100003</ID></C></RS>"#;
        assert_eq!(parse_reserve_ids(xml), vec![100001, 100002, 100003]);
        assert_eq!(parse_reserve_range(xml), Some((100001, 100003)));
    }

    #[test]
    fn parse_reserve_ids_ignores_cnf_modem_and_non_numeric() {
        // Спека F: модемний <CNF FR TO> більше не парситься.
        let xml = br#"<RS><C T="112"><CNF TY="C" FR="1001" TO="1100" ER="0"/></C></RS>"#;
        assert!(parse_reserve_ids(xml).is_empty());
        assert_eq!(parse_reserve_range(xml), None);
        assert_eq!(parse_reserve_ids(b"<ID>abc</ID>"), Vec::<i64>::new());
        assert_eq!(parse_reserve_ids(b"<ID>0</ID>"), Vec::<i64>::new());
    }

    #[test]
    fn parse_reserve_ids_invalid_returns_empty() {
        assert!(parse_reserve_ids(b"not xml").is_empty());
        assert_eq!(parse_reserve_range(b"not xml"), None);
    }
}
