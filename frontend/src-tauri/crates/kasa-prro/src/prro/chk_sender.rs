//! Контракт передачі чеків на фіскальний сервер (для use cases 7.3).
//! Реалізація: `PrroGrpcClient` (реальний gRPC sendChkV2); тести — мок.

use async_trait::async_trait;

use crate::grpc::{PrroGrpcClient, PrroGrpcError};
use crate::proto::{Check, CheckResponse};

/// Надсилач чеку/Z-звіту — абстракція над gRPC `sendChkV2`.
#[async_trait]
pub trait ChkSender: Send + Sync {
    /// Передає чек на фіскальний сервер. 1:1 Python `grpc_client.send_chk`.
    async fn send_chk(&self, check: Check) -> Result<CheckResponse, PrroGrpcError>;
}

#[async_trait]
impl ChkSender for PrroGrpcClient {
    async fn send_chk(&self, check: Check) -> Result<CheckResponse, PrroGrpcError> {
        self.send_chk_v2(check).await
    }
}

/// Мок-надсилач для тестів: керована відповідь (Ok/Err) + журнал викликів.
#[derive(Debug, Default)]
pub struct MockChkSender {
    /// Відповіді по черзі; якщо порожньо — Ok(status=1).
    pub responses: std::sync::Mutex<Vec<Result<CheckResponse, PrroGrpcError>>>,
    /// Журнал отриманих Check (для асертів).
    pub calls: std::sync::Mutex<Vec<Check>>,
}

impl MockChkSender {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_ok(&self, id: impl Into<String>) {
        self.responses.lock().unwrap().push(Ok(CheckResponse {
            id: id.into(),
            status: 1,
            id_sign: vec![],
            data_sign: vec![],
            error_message: String::new(),
        }));
    }

    pub fn push_fail(&self, error_message: &str, status: i32) {
        self.responses.lock().unwrap().push(Ok(CheckResponse {
            id: String::new(),
            status,
            id_sign: vec![],
            data_sign: vec![],
            error_message: error_message.to_string(),
        }));
    }

    pub fn calls_len(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait]
impl ChkSender for MockChkSender {
    async fn send_chk(&self, check: Check) -> Result<CheckResponse, PrroGrpcError> {
        self.calls.lock().unwrap().push(check);
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Ok(CheckResponse {
                id: "mock-id".into(),
                status: 1,
                id_sign: vec![],
                data_sign: vec![],
                error_message: String::new(),
            });
        }
        responses.remove(0)
    }
}
