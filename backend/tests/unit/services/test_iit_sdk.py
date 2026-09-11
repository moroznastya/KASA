"""Unit-тести обгортки крипто-ядра ІІТ (IitSdk).

Не потребують встановленого SDK: перевіряються помилки, евристика
визначення сертифіката підписанта та поведінка без ключа.
Інтеграційний тест (sign+verify) у test_prro_jks_dstu.py — виконується
лише за наявності backend/vendor/iit-sdk (див. scripts/setup_iit_sdk.sh).
"""
import datetime
import os
import sys

import pytest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID

from app.infrastructure.services.prro.iit_sdk import IitSdk, IitSdkError


def _make_cert(cn: str, is_ca: bool, self_signed: bool = False) -> bytes:
    """Створює DER-сертифікат для перевірки евристики _find_signer_cert."""
    key = rsa.generate_private_key(public_exponent=65537, key_size=1024)
    subject = issuer = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, cn)])
    if not self_signed:
        issuer = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Проміжний ЦСК")])
    cert = (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(issuer)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(datetime.datetime.now(datetime.UTC) - datetime.timedelta(days=1))
        .not_valid_after(datetime.datetime.now(datetime.UTC) + datetime.timedelta(days=365))
        .add_extension(
            x509.BasicConstraints(ca=is_ca, path_length=None),
            critical=True,
        )
        .sign(key, hashes.SHA256())
    )
    return cert.public_bytes(serialization.Encoding.DER)


class TestAvailable:
    def test_available_false_for_missing_lib(self, tmp_path):
        sdk = IitSdk(lib_path=tmp_path / "missing.so")
        assert sdk.available is False

    def test_ensure_library_raises_clear_error(self, tmp_path):
        sdk = IitSdk(lib_path=tmp_path / "missing.so")
        with pytest.raises(IitSdkError) as ei:
            sdk._ensure_library()
        assert "EUSignCP" in str(ei.value)
        assert "setup_iit_sdk.sh" in str(ei.value)


class TestSignWithoutKey:
    def test_sign_before_load_raises(self, tmp_path):
        sdk = IitSdk(lib_path=tmp_path / "missing.so")
        with pytest.raises(IitSdkError) as ei:
            sdk.sign_data_internal(b"<data/>")
        assert "не завантажено" in str(ei.value)


class TestFindSignerCert:
    def test_skips_root_and_intermediate_ca(self):
        root = _make_cert("Засвідчувальний центр", is_ca=True, self_signed=True)
        intermediate = _make_cert("КНЕДП АЦСК АТ КБ ПРИВАТБАНК", is_ca=True)
        end_user = _make_cert("МОРОЗ АНАСТАСІЯ", is_ca=False)
        found = IitSdk._find_signer_cert([root, intermediate, end_user])
        assert found is not None
        cn = found.subject.get_attributes_for_oid(NameOID.COMMON_NAME)[0].value
        assert cn == "МОРОЗ АНАСТАСІЯ"

    def test_end_user_without_basic_constraints(self):
        # кінцевий сертифікат без BasicConstraints — теж підходить
        key = rsa.generate_private_key(public_exponent=65537, key_size=1024)
        cert = (
            x509.CertificateBuilder()
            .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "КІНЦЕВИЙ")]))
            .issuer_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "ЦСК")]))
            .public_key(key.public_key())
            .serial_number(x509.random_serial_number())
            .not_valid_before(datetime.datetime.now(datetime.UTC))
            .not_valid_after(datetime.datetime.now(datetime.UTC) + datetime.timedelta(days=30))
            .sign(key, hashes.SHA256())
        )
        found = IitSdk._find_signer_cert(
            [cert.public_bytes(serialization.Encoding.DER)]
        )
        assert found is not None

    def test_empty_chain_returns_none(self):
        assert IitSdk._find_signer_cert([]) is None

    def test_broken_certs_are_skipped(self):
        root = _make_cert("ЗЦ", is_ca=True, self_signed=True)
        found = IitSdk._find_signer_cert([root, b"\x00\x01broken"])
        # пошкоджений сертифікат ігнорується; ЦСК пропускається,
        # але fallback повертає перший розпарсений сертифікат
        assert found is None or found.subject.rfc4514_string().startswith("CN=ЗЦ")
