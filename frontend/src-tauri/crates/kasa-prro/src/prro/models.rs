//! Доменні моделі ПРРО: зміни, офлайн-черга, налаштування (етап 7.3).
//! 1:1 Python `backend/app/infrastructure/persistence/models/prro.py`.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

/// Статус зміни ПРРО — 1:1 `PrroShiftStatus` (open/closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrroShiftStatus {
    Open,
    Closed,
}

impl PrroShiftStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrroShiftStatus::Open => "open",
            PrroShiftStatus::Closed => "closed",
        }
    }
}

/// Статус передачі фіскального документа — 1:1 `PrroQueueStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrroQueueStatus {
    Pending,
    Sent,
    Failed,
}

impl PrroQueueStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrroQueueStatus::Pending => "pending",
            PrroQueueStatus::Sent => "sent",
            PrroQueueStatus::Failed => "failed",
        }
    }
}

/// Зміна ПРРО (аналог касової зміни) — 1:1 `PrroShift`.
#[derive(Debug, Clone)]
pub struct PrroShift {
    pub id: Uuid,
    pub shift_number: i64,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub signer_serial: Option<String>,
    pub signer_name: Option<String>,
    pub closed_by: Option<String>,
    pub zreport_number: Option<String>,
    pub status: PrroShiftStatus,
    pub receipt_count: i64,
    pub total_amount: Decimal,
    pub last_local_number: i64,
    pub last_mac: Option<String>,
}

impl PrroShift {
    pub fn new(shift_number: i64, opened_at: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            shift_number,
            opened_at,
            closed_at: None,
            signer_serial: None,
            signer_name: None,
            closed_by: None,
            zreport_number: None,
            status: PrroShiftStatus::Open,
            receipt_count: 0,
            total_amount: Decimal::ZERO,
            last_local_number: 0,
            last_mac: None,
        }
    }
}

/// Запис офлайн-черги — 1:1 `PrroQueueItem`.
#[derive(Debug, Clone)]
pub struct PrroQueueItem {
    pub id: Uuid,
    pub receipt_id: Option<Uuid>,
    pub shift_id: Option<Uuid>,
    pub local_number: i64,
    pub check_type: String,
    pub xml_body: String,
    pub mac: Option<String>,
    pub status: PrroQueueStatus,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
}

impl PrroQueueItem {
    pub fn new(
        receipt_id: Option<Uuid>,
        shift_id: Option<Uuid>,
        local_number: i64,
        check_type: impl Into<String>,
        xml_body: impl Into<String>,
        mac: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            receipt_id,
            shift_id,
            local_number,
            check_type: check_type.into(),
            xml_body: xml_body.into(),
            mac,
            status: PrroQueueStatus::Pending,
            error: None,
            created_at: Utc::now(),
            sent_at: None,
        }
    }
}

/// Налаштування ПРРО (ключ-значення) — 1:1 `PrroSetting`.
#[derive(Debug, Clone)]
pub struct PrroSetting {
    pub key_name: String,
    pub value: Option<String>,
}

// Ключі налаштувань — 1:1 Python `context.py`.
pub const KEY_PRRO_FN: &str = "prro_fn";
pub const KEY_PRRO_TN: &str = "prro_tn";
pub const KEY_PRRO_ZN: &str = "prro_zn";
pub const KEY_PRRO_MODE: &str = "prro_mode";
pub const KEY_PRRO_URL: &str = "prro_url";
pub const KEY_LAST_SHIFT_NUMBER: &str = "last_shift_number";
pub const KEY_LAST_PACKET_ID: &str = "last_packet_id";
pub const KEY_LAST_MAC_NUMBER: &str = "last_mac_number";

// Типи фіскальних документів — 1:1 Python `offline_queue.py`.
pub const CHECK_TYPE_CHK: &str = "CHK";
pub const CHECK_TYPE_ZREPORT: &str = "ZREPORT";
pub const CHECK_TYPE_SERVICECHK: &str = "SERVICECHK";

/// Ліміт офлайн-режиму: 168 годин (7 діб) — 1:1 Python.
pub const PRRO_OFFLINE_LIMIT_HOURS: i64 = 168;
