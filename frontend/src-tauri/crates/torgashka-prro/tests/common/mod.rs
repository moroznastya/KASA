//! Спільні мок-компоненти для тестів 7.3.
#![allow(dead_code)] // частина моків використовується не всіма тест-таргетами

use torgashka_prro::crypto::{PrroCryptoError, PrroSigner};

/// Мок-підписант: sign повертає вхідні байти (детерміновано).
#[derive(Debug, Clone, Copy)]
pub struct MockSigner;

impl PrroSigner for MockSigner {
    fn sign(&self, xml_bytes: &[u8]) -> Result<Vec<u8>, PrroCryptoError> {
        Ok(xml_bytes.to_vec())
    }
    fn verify(&self, _signed_xml: &[u8]) -> Result<bool, PrroCryptoError> {
        Ok(true)
    }
    fn get_serial_number(&self) -> Result<String, PrroCryptoError> {
        Ok("5E984D526F82F38F".to_string())
    }
    fn get_signer_name(&self) -> Result<String, PrroCryptoError> {
        Ok("ТЕСТОВИЙ ПІДПИСАНТ".to_string())
    }
}

/// Створює XmlBuilder з нульовими лічильниками (для тестів).
pub fn test_builder() -> torgashka_prro::xml::XmlBuilder {
    torgashka_prro::xml::XmlBuilder::new(
        "400000000000",
        "400000000000",
        "400000000000",
        "1",
        "2.1.7",
        0,
        0,
    )
}
