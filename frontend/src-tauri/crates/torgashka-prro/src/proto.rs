//! Згенеровані tonic/prost типи з `prro.proto` (package `com.programika.rro.ws.chk`).
//!
//! Див. `build.rs` — proto читається з Python-еталона
//! `backend/app/infrastructure/services/prro/prro.proto`.

pub mod prro {
    tonic::include_proto!("com.programika.rro.ws.chk");
}

pub use prro::{
    check::Type as CheckType, check_response::Status as CheckResponseStatus,
    chk_income_service_client::ChkIncomeServiceClient, Check, CheckRequest, CheckRequestId,
    CheckResponse, RroInfoResponse, StatusResponse,
};

/// Максимальне значення int32 — для перевірки зв'язку (ping), 1:1 Python.
pub const PING_LOCAL_NUMBER: i32 = 0x7FFF_FFFF;
