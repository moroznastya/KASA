//! Офлайн-черга фіскальних документів ПРРО — 1:1 Python `offline_queue.py`.
//!
//! Ліміт офлайн-режиму: 168 годин (7 діб) — `PRRO_OFFLINE_LIMIT_HOURS`.
//! Після перевищення ліміту документи вважаються простроченими.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::models::{PrroQueueItem, PRRO_OFFLINE_LIMIT_HOURS};
use super::repository::{PrroRepoError, PrroRepository};

/// Помилка черги — 1:1 ValueError Python.
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("Локальний номер не може бути від'ємним: {0}")]
    NegativeLocalNumber(i64),
    #[error("xml_body не може бути порожнім")]
    EmptyXml,
    #[error("репозиторій: {0}")]
    Repo(#[from] PrroRepoError),
}

/// Черга офлайн-документів ПРРО — 1:1 `PrroOfflineQueue` (методи безстатеві).
pub struct PrroOfflineQueue;

impl PrroOfflineQueue {
    /// Додає фіскальний документ в офлайн-чергу (status=pending).
    pub async fn add_document(
        repo: &dyn PrroRepository,
        receipt_id: Option<Uuid>,
        shift_id: Option<Uuid>,
        local_number: i64,
        check_type: &str,
        xml_body: &str,
        mac: Option<String>,
    ) -> Result<PrroQueueItem, QueueError> {
        if local_number < 0 {
            return Err(QueueError::NegativeLocalNumber(local_number));
        }
        if xml_body.trim().is_empty() {
            return Err(QueueError::EmptyXml);
        }
        let item = PrroQueueItem::new(
            receipt_id,
            shift_id,
            local_number,
            check_type,
            xml_body,
            mac,
        );
        Ok(repo.add_to_queue(item).await?)
    }

    /// Документи, що очікують передачі (pending/failed), у порядку черги.
    pub async fn get_pending(
        repo: &dyn PrroRepository,
        limit: u32,
    ) -> Result<Vec<PrroQueueItem>, QueueError> {
        Ok(repo.list_pending(limit).await?)
    }

    /// Кількість документів, що очікують передачі (лише pending) — 1:1.
    pub async fn count_pending(repo: &dyn PrroRepository) -> Result<u64, QueueError> {
        Ok(repo.count_pending().await?)
    }

    /// Документи черги за зміною (у порядку локальних номерів).
    pub async fn list_by_shift(
        repo: &dyn PrroRepository,
        shift_id: Uuid,
    ) -> Result<Vec<PrroQueueItem>, QueueError> {
        Ok(repo.list_by_shift(shift_id).await?)
    }

    /// Позначає документ як успішно переданий (status=sent, sent_at=now).
    pub async fn mark_sent(
        repo: &dyn PrroRepository,
        item_id: Uuid,
        sent_at: Option<DateTime<Utc>>,
    ) -> Result<Option<PrroQueueItem>, QueueError> {
        Ok(repo.mark_sent(item_id, sent_at).await?)
    }

    /// Позначає документ як помилку передачі (status=failed, error=текст).
    pub async fn mark_failed(
        repo: &dyn PrroRepository,
        item_id: Uuid,
        error: String,
    ) -> Result<Option<PrroQueueItem>, QueueError> {
        Ok(repo.mark_failed(item_id, error).await?)
    }

    /// Чи вичерпано ліміт офлайн-передачі (created_at старіше 168 год).
    pub fn is_expired(created_at: DateTime<Utc>, now: Option<DateTime<Utc>>) -> bool {
        let now = now.unwrap_or_else(Utc::now);
        (now - created_at) > chrono::Duration::hours(PRRO_OFFLINE_LIMIT_HOURS)
    }

    /// Прострочені документи (старші за ліміт офлайн-режиму) — 1:1.
    pub async fn get_expired(
        repo: &dyn PrroRepository,
        limit: u32,
    ) -> Result<Vec<PrroQueueItem>, QueueError> {
        let pending = repo.list_pending(limit).await?;
        let now = Utc::now();
        Ok(pending
            .into_iter()
            .filter(|item| Self::is_expired(item.created_at, Some(now)))
            .collect())
    }
}
