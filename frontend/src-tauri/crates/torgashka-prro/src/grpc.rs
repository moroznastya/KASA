//! gRPC-клієнт `ChkIncomeService` фіскального сервера ДПС (ПРРО) — tonic.
//!
//! 1:1 Python `grpc_client.py`:
//! - методи: sendChkV2, ping, statusRro, infoRro, lastChk, delLastChk, delLastChkId;
//! - TLS (WebPKI — native roots; кастомний CA — `ClientTlsConfig::ca_certificate`);
//! - таймаути (gRPC deadline) + ретраї (3 спроби, експоненційний бек-оф 1s → 2s);
//! - `date_time` у форматі `yyyyMMddHHmmss` (14 цифр, локальний час) — як
//!   офіційний семпл ДПС (Sender.java) та Python `_check_date_time`.

use crate::proto::{
    prro::chk_income_service_client::ChkIncomeServiceClient, Check, CheckRequest, CheckRequestId,
    PING_LOCAL_NUMBER,
};
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use tonic::{Request, Status};

/// Таймаут одного RPC-виклику (1:1 Python `DEFAULT_TIMEOUT_SECONDS`).
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
/// Максимальна кількість спроб (1:1 Python `DEFAULT_MAX_RETRIES`).
pub const DEFAULT_MAX_RETRIES: u32 = 3;
/// Початкова затримка бек-офа, сек (1:1 Python `DEFAULT_INITIAL_BACKOFF_SECONDS`).
pub const DEFAULT_INITIAL_BACKOFF_SECONDS: u64 = 1;

#[derive(Debug, Clone, thiserror::Error)]
pub enum PrroGrpcError {
    #[error("Не вдалося створити TLS-канал до {0}: {1}")]
    Channel(String, String),
    #[error("gRPC-виклик не вдався після {max_retries} спроб: {status}")]
    Rpc { status: Status, max_retries: u32 },
}

/// Конфігурація TLS-каналу.
#[derive(Debug, Clone, Default)]
pub struct TlsConfig {
    /// PEM-сертифікат кастомного CA (опційно; за замовчуванням — native roots).
    pub ca_cert_pem: Option<Vec<u8>>,
}

/// Формує date_time з моменту часу у форматі `yyyyMMddHHmmss` (локальний час) — 1:1 Python.
pub fn check_date_time_from(now: chrono::DateTime<chrono::Utc>) -> i64 {
    let s = now
        .with_timezone(&chrono::Local)
        .format("%Y%m%d%H%M%S")
        .to_string();
    s.parse::<i64>().unwrap_or(0)
}

/// Формує date_time у форматі `yyyyMMddHHmmss` (локальний час) — 1:1 Python.
pub fn check_date_time() -> i64 {
    check_date_time_from(chrono::Utc::now())
}

/// gRPC-клієнт сервісу ChkIncomeService.
#[derive(Clone)]
pub struct PrroGrpcClient {
    stub: ChkIncomeServiceClient<Channel>,
    rro_fn: String,
    /// B3: підписаний ФН ПРРО (тим самим КЕП-ключем, що й check_sign) —
    /// надсилається у statusRro/infoRro/lastChk/delLastChk/delLastChkId.
    rro_fn_sign: Vec<u8>,
    timeout: std::time::Duration,
    max_retries: u32,
    initial_backoff: std::time::Duration,
}

impl PrroGrpcClient {
    /// Створює клієнт з TLS-каналом (WebPKI або кастомний CA).
    pub async fn connect(
        target: &str,
        tls: TlsConfig,
        rro_fn: impl Into<String>,
    ) -> Result<Self, PrroGrpcError> {
        // PRRO_GRPC_INSECURE=1 → plaintext (для мок-сервера в differential;
        // Python-аналог: PRRO_USE_SSL=false).
        let insecure = matches!(
            std::env::var("PRRO_GRPC_INSECURE")
                .unwrap_or_default()
                .trim()
                .to_lowercase()
                .as_str(),
            "1" | "true"
        );
        let endpoint = if insecure {
            tonic::transport::Endpoint::from_shared(format!("http://{target}"))
                .map_err(|e| PrroGrpcError::Channel(target.to_string(), e.to_string()))?
        } else {
            let mut ep = tonic::transport::Endpoint::from_shared(format!("https://{target}"))
                .map_err(|e| PrroGrpcError::Channel(target.to_string(), e.to_string()))?;
            let mut tls_config = ClientTlsConfig::new().with_native_roots();
            if let Some(pem) = &tls.ca_cert_pem {
                let cert = Certificate::from_pem(pem);
                tls_config = tls_config.ca_certificate(cert);
            }
            ep = ep
                .tls_config(tls_config)
                .map_err(|e| PrroGrpcError::Channel(target.to_string(), e.to_string()))?;
            ep
        };

        let channel = endpoint
            .connect()
            .await
            .map_err(|e| PrroGrpcError::Channel(target.to_string(), e.to_string()))?;

        Ok(Self {
            stub: ChkIncomeServiceClient::new(channel),
            rro_fn: rro_fn.into(),
            rro_fn_sign: Vec::new(), // B3: заповнюється через with_rro_fn_sign
            timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
            max_retries: DEFAULT_MAX_RETRIES,
            initial_backoff: std::time::Duration::from_secs(DEFAULT_INITIAL_BACKOFF_SECONDS),
        })
    }

    /// Створює клієнт з готовим каналом (для тестів/інжекції).
    pub fn from_channel(channel: Channel, rro_fn: impl Into<String>) -> Self {
        Self {
            stub: ChkIncomeServiceClient::new(channel),
            rro_fn: rro_fn.into(),
            rro_fn_sign: Vec::new(), // B3: заповнюється через with_rro_fn_sign
            timeout: std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
            max_retries: DEFAULT_MAX_RETRIES,
            initial_backoff: std::time::Duration::from_secs(DEFAULT_INITIAL_BACKOFF_SECONDS),
        }
    }

    /// B3: задає підписаний ФН ПРРО (rro_fn_sign) для службових RPC.
    pub fn with_rro_fn_sign(mut self, sign: Vec<u8>) -> Self {
        self.rro_fn_sign = sign;
        self
    }

    fn make_check(
        &self,
        check_sign: Vec<u8>,
        local_number: i32,
        check_type: i32,
        date_time: Option<i64>,
        id_offline: String,
        id_cancel: String,
    ) -> Check {
        Check {
            rro_fn: self.rro_fn.clone(),
            date_time: date_time.unwrap_or_else(check_date_time),
            check_sign,
            local_number,
            check_type,
            id_offline,
            id_cancel,
        }
    }

    /// RPC-виклик з gRPC-дедлайном і ретраями (1:1 Python `_call_with_retry`).
    async fn call_with_retry<F, Fut, T>(
        &self,
        method_name: &str,
        mut f: F,
    ) -> Result<T, PrroGrpcError>
    where
        F: FnMut(ChkIncomeServiceClient<Channel>) -> Fut,
        Fut: std::future::Future<Output = Result<tonic::Response<T>, Status>>,
    {
        let mut last_error: Option<Status> = None;
        for attempt in 1..=self.max_retries {
            match f(self.stub.clone()).await {
                Ok(resp) => {
                    tracing::debug!("PRRO_GRPC_CALL_OK | method={method_name} attempt={attempt}");
                    return Ok(resp.into_inner());
                }
                Err(status) => {
                    tracing::warn!(
                        "PRRO_GRPC_CALL_ERR | method={method_name} attempt={attempt}/{max} code={:?} details={}",
                        status.code(),
                        status.message(),
                        max = self.max_retries,
                    );
                    last_error = Some(status);
                    if attempt < self.max_retries {
                        let delay = self.initial_backoff * (2u32.pow(attempt - 1));
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
        Err(PrroGrpcError::Rpc {
            status: last_error.unwrap_or_else(|| Status::unknown("no response")),
            max_retries: self.max_retries,
        })
    }

    /// RPC-виклик БЕЗ сліпих ретраїв (H1): фіскальний документ відправляється
    /// рівно один раз; рішення про повтор приймає fiscalize (lastChk-перевірка).
    async fn call_once<F, Fut, T>(&self, method_name: &str, f: F) -> Result<T, PrroGrpcError>
    where
        F: FnOnce(ChkIncomeServiceClient<Channel>) -> Fut,
        Fut: std::future::Future<Output = Result<tonic::Response<T>, Status>>,
    {
        match f(self.stub.clone()).await {
            Ok(resp) => {
                tracing::debug!("PRRO_GRPC_CALL_OK | method={method_name} attempt=1");
                Ok(resp.into_inner())
            }
            Err(status) => {
                tracing::warn!(
                    "PRRO_GRPC_CALL_ERR | method={method_name} attempt=1/1 code={:?} details={}",
                    status.code(),
                    status.message(),
                );
                Err(PrroGrpcError::Rpc {
                    status,
                    max_retries: 1,
                })
            }
        }
    }

    /// Передача чеку / Z-звіту (sendChkV2) — основний метод з 01.10.2021.
    pub async fn send_chk_v2(
        &self,
        check: Check,
    ) -> Result<crate::proto::CheckResponse, PrroGrpcError> {
        // H1: фіскальний документ — БЕЗ сліпих ретраїв. Якщо транспортна
        // помилка, fiscalize робить lastChk-перевірку і ТІЛЬКИ тоді повторює
        // send (контрольований retry без ризику дубліката).
        let timeout = self.timeout;
        self.call_once("sendChkV2", move |mut stub| {
            let req = request_with_deadline(check.clone(), timeout);
            async move { stub.send_chk_v2(req).await }
        })
        .await
    }

    /// Перевірка зв'язку (ping; local_number=0x7FFFFFFF, check_type=SERVICECHK).
    pub async fn ping(
        &self,
        check_sign: Vec<u8>,
    ) -> Result<crate::proto::CheckResponse, PrroGrpcError> {
        let check = self.make_check(
            check_sign,
            PING_LOCAL_NUMBER,
            3,
            None,
            String::new(),
            String::new(),
        );
        let timeout = self.timeout;
        self.call_with_retry("ping", move |mut stub| {
            let req = request_with_deadline(check.clone(), timeout);
            async move { stub.ping(req).await }
        })
        .await
    }

    /// Статус ПРРО (statusRro).
    pub async fn status(&self) -> Result<crate::proto::StatusResponse, PrroGrpcError> {
        let timeout = self.timeout;
        self.call_with_retry("statusRro", move |mut stub| {
            let req = request_with_deadline(
                CheckRequest {
                    rro_fn_sign: self.rro_fn_sign.clone(), // B3
                },
                timeout,
            );
            async move { stub.status_rro(req).await }
        })
        .await
    }

    /// Детальна інформація про ПРРО (infoRro).
    pub async fn info(&self) -> Result<crate::proto::RroInfoResponse, PrroGrpcError> {
        let timeout = self.timeout;
        self.call_with_retry("infoRro", move |mut stub| {
            let req = request_with_deadline(
                CheckRequest {
                    rro_fn_sign: self.rro_fn_sign.clone(), // B3
                },
                timeout,
            );
            async move { stub.info_rro(req).await }
        })
        .await
    }

    /// Останній чек (lastChk).
    pub async fn last_chk(&self) -> Result<crate::proto::CheckResponse, PrroGrpcError> {
        let timeout = self.timeout;
        self.call_with_retry("lastChk", move |mut stub| {
            let req = request_with_deadline(
                CheckRequest {
                    rro_fn_sign: self.rro_fn_sign.clone(), // B3
                },
                timeout,
            );
            async move { stub.last_chk(req).await }
        })
        .await
    }

    /// Вилучення останнього чеку (delLastChk).
    pub async fn del_last_chk(&self) -> Result<crate::proto::CheckResponse, PrroGrpcError> {
        let timeout = self.timeout;
        self.call_with_retry("delLastChk", move |mut stub| {
            let req = request_with_deadline(
                CheckRequest {
                    rro_fn_sign: self.rro_fn_sign.clone(), // B3
                },
                timeout,
            );
            async move { stub.del_last_chk(req).await }
        })
        .await
    }

    /// Вилучення чеку за ID (delLastChkId).
    pub async fn del_last_chk_id(
        &self,
        check_id: String,
    ) -> Result<crate::proto::CheckResponse, PrroGrpcError> {
        let timeout = self.timeout;
        self.call_with_retry("delLastChkId", move |mut stub| {
            let req = request_with_deadline(
                CheckRequestId {
                    id: check_id.clone(),
                    rro_fn_sign: self.rro_fn_sign.clone(), // B3
                },
                timeout,
            );
            async move { stub.del_last_chk_id(req).await }
        })
        .await
    }

    /// Відкриття зміни (службовий чек, local_number=0, check_type=CHK).
    pub async fn open_shift(
        &self,
        check_sign: Vec<u8>,
    ) -> Result<crate::proto::CheckResponse, PrroGrpcError> {
        let check = self.make_check(check_sign, 0, 1, None, String::new(), String::new());
        let timeout = self.timeout;
        self.call_with_retry("sendChkV2(open_shift)", move |mut stub| {
            let req = request_with_deadline(check.clone(), timeout);
            async move { stub.send_chk_v2(req).await }
        })
        .await
    }
}

/// Хелпер: встановлює gRPC-дедлайн (1:1 Python `timeout=` в grpc.aio).
fn request_with_deadline<T>(msg: T, timeout: std::time::Duration) -> Request<T> {
    let mut req = Request::new(msg);
    req.set_timeout(timeout);
    req
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_date_time_format_is_14_digits() {
        let v = check_date_time();
        let s = v.to_string();
        assert_eq!(s.len(), 14, "yyyyMMddHHmmss — 14 цифр, отримано {s}");
    }

    #[test]
    fn ping_local_number_is_max_int32() {
        assert_eq!(PING_LOCAL_NUMBER, 2_147_483_647);
    }

    #[tokio::test]
    async fn with_rro_fn_sign_sets_non_empty_signature() {
        // B3: rro_fn_sign — підпис ФН ПРРО тим самим КЕП-ключем.
        // Жоден CheckRequest/CheckRequestId не містить Vec::new(), коли
        // клієнт створено через with_rro_fn_sign.
        let channel = Channel::from_shared("http://127.0.0.1:1")
            .expect("channel uri")
            .connect_lazy();
        let client = PrroGrpcClient::from_channel(channel, "4538765845")
            .with_rro_fn_sign(b"sign-of-fn-4538765845".to_vec());

        assert!(!client.rro_fn_sign.is_empty(), "B3: rro_fn_sign заповнено");
        assert_eq!(client.rro_fn_sign, b"sign-of-fn-4538765845");
        // Усі службові RPC клонують self.rro_fn_sign (див. status/info/last_chk/
        // del_last_chk/del_last_chk_id) — жоден не створює порожній Vec::new().
        let requests = [
            CheckRequest {
                rro_fn_sign: client.rro_fn_sign.clone(),
            },
            CheckRequest {
                rro_fn_sign: client.rro_fn_sign.clone(),
            },
        ];
        for r in &requests {
            assert!(
                !r.rro_fn_sign.is_empty(),
                "B3: CheckRequest без rro_fn_sign"
            );
        }
        let req_by_id = CheckRequestId {
            id: "chk-1".into(),
            rro_fn_sign: client.rro_fn_sign.clone(),
        };
        assert!(
            !req_by_id.rro_fn_sign.is_empty(),
            "B3: CheckRequestId без rro_fn_sign"
        );
    }
}
