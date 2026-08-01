"""Unit tests: PrroCryptoSigner — XAdES-підписання та формати ключів."""

from __future__ import annotations

import datetime
from pathlib import Path

import pytest
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives.serialization import pkcs12
from cryptography.x509.oid import NameOID

from app.infrastructure.services.prro.crypto_signer import (
    PrroCryptoSigner,
    PrroCryptoError,
)

# ─── Допоміжні функції для генерації тестових ключів ──────────────────────

TEST_PASSWORD = "Test-Password-123"


def generate_test_cert(tmp_path: Path) -> tuple[Path, bytes]:
    """
    Генерує тестовий RSA-ключ + самопідписаний сертифікат.

    Returns:
        (pfx_path, cert_der) — шлях до PKCS#12 файлу та DER сертифіката.
    """
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)

    name = x509.Name([
        x509.NameAttribute(NameOID.COUNTRY_NAME, "UA"),
        x509.NameAttribute(NameOID.COMMON_NAME, "ТЕСТОВИЙ ПІДПИСАНТ"),
        x509.NameAttribute(NameOID.GIVEN_NAME, "Іван"),
        x509.NameAttribute(NameOID.SURNAME, "Петренко"),
    ])
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(days=1))
        .not_valid_after(now + datetime.timedelta(days=365))
        .sign(key, hashes.SHA256())
    )

    pfx_data = pkcs12.serialize_key_and_certificates(
        name=b"test",
        key=key,
        cert=cert,
        cas=None,
        encryption_algorithm=serialization.BestAvailableEncryption(
            TEST_PASSWORD.encode("utf-8")
        ),
    )

    pfx_path = tmp_path / "test-key.pfx"
    pfx_path.write_bytes(pfx_data)
    return pfx_path, cert.public_bytes(serialization.Encoding.DER)


@pytest.fixture
def pfx_key(tmp_path: Path) -> tuple[Path, bytes]:
    """Тестовий PKCS#12 ключ."""
    return generate_test_cert(tmp_path)


@pytest.fixture
def signer(pfx_key) -> PrroCryptoSigner:
    """Підписант на основі тестового PKCS#12."""
    path, _ = pfx_key
    return PrroCryptoSigner(key_path=path, key_password=TEST_PASSWORD)


# ─── detect_format ─────────────────────────────────────────────────────────

class TestDetectFormat:
    """Авто-визначення формату ключа за розширенням."""

    @pytest.mark.parametrize(
        "suffix, expected",
        [
            (".pfx", "pfx"),
            (".p12", "p12"),
            (".jks", "jks"),
            (".pem", "pem"),
            (".crt", "pem"),
            (".key", "pem"),
            (".dat", "dat"),
        ],
    )
    def test_extension_mapping(self, tmp_path, suffix, expected):
        """Визначення за розширенням файлу (detect_format не потребує ключа)."""
        key_file = tmp_path / f"key{suffix}"
        key_file.write_bytes(b"placeholder")
        s = PrroCryptoSigner.__new__(PrroCryptoSigner)
        s.key_path = key_file
        assert s.detect_format() == expected

    def test_unknown_extension_raises(self, tmp_path):
        """Невідоме розширення → PrroCryptoError."""
        key_file = tmp_path / "key.xyz"
        key_file.write_bytes(b"data")
        s = PrroCryptoSigner.__new__(PrroCryptoSigner)
        s.key_path = key_file
        with pytest.raises(PrroCryptoError):
            s.detect_format()

    def test_missing_file_raises(self, tmp_path):
        """Відсутній файл ключа → PrroCryptoError."""
        with pytest.raises(PrroCryptoError):
            PrroCryptoSigner(
                key_path=tmp_path / "nope.pfx",
                key_password=TEST_PASSWORD,
            )


# ─── sign / verify ─────────────────────────────────────────────────────────

class TestSignVerify:
    """XAdES-підписання та перевірка."""

    def test_sign_adds_signature_element(self, signer):
        """Після підписання з'являється елемент Signature."""
        dat_xml = (
            '<DAT DI="1" FN="1234567890" TN="123" V="1" ZN="ABC">'
            '<C T="0"><P N="1" NM="Товар" SM="1000" Q="1000" PRC="1000" TX="1"></P>'
            '<E N="2" NO="1" SM="1000" FN="1234567890" TS="20260801120000" '
            'TX="1" TXPR="20.00" TXSM="167" TXTY="0" TXAL="0"></E></C>'
            '<TS>20260801120000</TS></DAT>'
        ).encode("utf-8")
        signed = signer.sign(dat_xml)
        assert b"Signature" in signed
        assert b"<DAT" in signed

    def test_verify_returns_true(self, signer):
        """Підписаний документ проходить verify."""
        dat_xml = (
            '<DAT DI="1" FN="1234567890" TN="123" V="1" ZN="ABC">'
            '<C T="0"><E N="1"></E></C>'
            '<TS>20260801120000</TS></DAT>'
        ).encode("utf-8")
        signed = signer.sign(dat_xml)
        assert signer.verify(signed) is True

    def test_verify_tampered_returns_false(self, signer):
        """Змінений документ не проходить verify."""
        dat_xml = (
            '<DAT DI="1" FN="1234567890" TN="123" V="1" ZN="ABC">'
            '<C T="0"><E N="1"></E></C>'
            '<TS>20260801120000</TS></DAT>'
        ).encode("utf-8")
        signed = signer.sign(dat_xml)
        # Змінюємо TS — підпис має стати невалідним
        tampered = signed.replace(b"20260801120000", b"20260801120001")
        assert tampered != signed
        assert signer.verify(tampered) is False

    def test_sign_invalid_xml_raises(self, signer):
        """Підписання некоректного XML → PrroCryptoError."""
        with pytest.raises(PrroCryptoError):
            signer.sign(b"<DAT><unclosed>")


# ─── Дані підписанта ───────────────────────────────────────────────────────

class TestSignerData:
    """Серійний номер та ПІБ з сертифіката."""

    def test_get_serial_number(self, signer):
        """Серійний номер — шістнадцятковий рядок (upper)."""
        serial = signer.get_serial_number()
        assert isinstance(serial, str)
        assert len(serial) > 0
        # Валідний hex
        int(serial, 16)

    def test_get_signer_name(self, signer):
        """ПІБ визначається з CN сертифіката."""
        name = signer.get_signer_name()
        assert "ТЕСТОВИЙ ПІДПИСАНТ" in name


# ─── Формат .dat ───────────────────────────────────────────────────────────

class TestDatFormat:
    """Обмеження формату .dat (ІІТ «ЦСК-1»)."""

    def test_dat_non_pkcs12_raises_documented_error(self, tmp_path):
        """Файл .dat без PKCS#12-сигнатури → документована помилка."""
        dat_file = tmp_path / "Key-6.dat"
        dat_file.write_bytes(b"\x00\x01\x02\x03 not a pkcs12 container")

        with pytest.raises(PrroCryptoError) as exc_info:
            PrroCryptoSigner(key_path=dat_file, key_password="x")

        message = str(exc_info.value)
        assert "Key-6.dat" in message or "ЦСК" in message
        assert "PKCS#12" in message or "PEM" in message

    def test_dat_that_is_pkcs12_loads(self, pfx_key, tmp_path):
        """Якщо .dat насправді є PKCS#12 — завантажується."""
        pfx_path, _ = pfx_key
        dat_path = tmp_path / "Key-6.dat"
        dat_path.write_bytes(pfx_path.read_bytes())

        signer = PrroCryptoSigner(key_path=dat_path, key_password=TEST_PASSWORD)
        assert signer.key_format == "dat"
        # Підписання працює
        signed = signer.sign(
            b'<DAT DI="1" FN="1" TN="1" V="1" ZN="1"><TS>20260801120000</TS></DAT>'
        )
        assert signer.verify(signed) is True


# ─── Формат .pem ───────────────────────────────────────────────────────────

class TestPemFormat:
    """Завантаження PEM-ключів (окремі файли та об'єднані)."""

    def test_pem_combined_file(self, pfx_key, tmp_path):
        """Об'єднаний PEM-файл (ключ + сертифікат)."""
        pfx_path, cert_der = pfx_key
        password = TEST_PASSWORD.encode("utf-8")
        key, cert, _ = pkcs12.load_key_and_certificates(
            pfx_path.read_bytes(), password
        )

        key_pem = key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.PKCS8,
            encryption_algorithm=serialization.NoEncryption(),
        )
        cert_pem = cert.public_bytes(serialization.Encoding.PEM)

        pem_file = tmp_path / "combined.pem"
        pem_file.write_bytes(key_pem + cert_pem)

        signer = PrroCryptoSigner(key_path=pem_file, key_password="")
        assert signer.key_format == "pem"
        assert signer.verify(
            signer.sign(b'<DAT DI="1" FN="1" TN="1" V="1" ZN="1"><TS>x</TS></DAT>')
        ) is True

    def test_pem_separate_cert_file(self, pfx_key, tmp_path):
        """PEM-ключ + сертифікат в окремому .crt файлі."""
        pfx_path, _ = pfx_key
        password = TEST_PASSWORD.encode("utf-8")
        key, cert, _ = pkcs12.load_key_and_certificates(
            pfx_path.read_bytes(), password
        )

        key_pem = key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.PKCS8,
            encryption_algorithm=serialization.NoEncryption(),
        )
        cert_pem = cert.public_bytes(serialization.Encoding.PEM)

        key_file = tmp_path / "priv.key"
        key_file.write_bytes(key_pem)
        cert_file = tmp_path / "priv.crt"
        cert_file.write_bytes(cert_pem)

        signer = PrroCryptoSigner(key_path=key_file, key_password="")
        assert signer.get_serial_number() == format(cert.serial_number, "X")


# ─── Формат .jks ───────────────────────────────────────────────────────────

class TestJksFormat:
    """Завантаження JKS-ключів через pyjks."""

    def test_jks_load_and_sign(self, pfx_key, tmp_path):
        """Створюємо JKS через pyjks і перевіряємо sign/verify."""
        jks = pytest.importorskip("jks")

        pfx_path, _ = pfx_key
        password = TEST_PASSWORD.encode("utf-8")
        key, cert, _ = pkcs12.load_key_and_certificates(
            pfx_path.read_bytes(), password
        )

        # Серіалізуємо приватний ключ у PKCS#8 DER
        key_der = key.private_bytes(
            encoding=serialization.Encoding.DER,
            format=serialization.PrivateFormat.PKCS8,
            encryption_algorithm=serialization.NoEncryption(),
        )
        cert_der = cert.public_bytes(serialization.Encoding.DER)

        # Створюємо JKS-сховище (entry передається в new — так він потрапляє у store.entries)
        entry = jks.PrivateKeyEntry.new(
            alias="testkey",
            certs=[cert_der],
            key=key_der,
            key_format="pkcs8",
        )
        store = jks.KeyStore.new("jks", [entry])

        jks_file = tmp_path / "keystore.jks"
        store.save(str(jks_file), TEST_PASSWORD)

        signer = PrroCryptoSigner(
            key_path=jks_file,
            key_password=TEST_PASSWORD,
        )
        assert signer.key_format == "jks"

        dat_xml = (
            '<DAT DI="1" FN="1234567890" TN="123" V="1" ZN="ABC">'
            '<C T="0"><E N="1"></E></C>'
            '<TS>20260801120000</TS></DAT>'
        ).encode("utf-8")
        signed = signer.sign(dat_xml)
        assert signer.verify(signed) is True
        assert signer.get_serial_number() == format(cert.serial_number, "X")
