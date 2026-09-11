//! Torgashka — ПРРО-модуль (етап 7 міграції → Rust).
//!
//! Фіскалізація чеків через фіскальний сервер ДПС України:
//! - [`grpc`] — gRPC-клієнт `ChkIncomeService` (tonic + prost, TLS),
//!   методи sendChkV2 / ping / statusRro / infoRro / lastChk / delLastChk / delLastChkId;
//! - [`xml`] — побудова XML СЗЗД 2.1.7 (чек `<C T="0|1">`, Z-звіт `<Z>`,
//!   службові T=108–112), канонічний вигляд (C14N), MAC-ланцюжок (SHA-256 → Base64);
//! - [`keystore`] — читання КЕП: JKS / PKCS#12 / PEM, авто-визначення формату,
//!   витяг приватного ключа (DER) + сертифікатів (DER) + OID алгоритму;
//! - [`crypto`] — крипто-шар: FFI до IIT SDK EUSignCP (ДСТУ 4145-2002) +
//!   чистий Rust для RSA/ECDSA (ADR-014).
//!
//! Стратегія: ADR-014. Еталон: `backend/app/infrastructure/services/prro/` (Python).

pub mod crypto;
pub mod grpc;
pub mod jks;
pub mod keystore;
pub mod proto;
pub mod prro;
pub mod xml;

pub use crypto::{PrroCryptoError, PrroSigner};
pub use keystore::{KeyFormat, KeyMaterial, KeyStoreError};
pub use xml::{XmlBuilder, XmlBuilderError};
