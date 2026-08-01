"""Тести завантаження JKS-ключів та визначення ДСТУ 4145-2002.

Перевіряють обхід багу pyjks 20.0.0 (падіння на PKCS#8 з параметрами
ДСТУ 4145) та ручний DER-парсер OID алгоритму.
"""
import os
import sys

import pytest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))

from app.infrastructure.services.prro.crypto_signer import PrroCryptoSigner  # noqa: E402

TEST_JKS = os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..", "..", "..",
    "certs", "prro-test", "pb_3791505547 (2).jks",
)
JKS_PASSWORD = "test2003"


def _der_oid(oid_str: str) -> bytes:
    """Кодує OID-рядок у DER (тіло, без тега 0x06)."""
    parts = [int(x) for x in oid_str.split(".")]
    body = bytes([parts[0] * 40 + parts[1]])
    for p in parts[2:]:
        chunks = [p & 0x7F]
        p >>= 7
        while p:
            chunks.append((p & 0x7F) | 0x80)
            p >>= 7
        body += bytes(reversed(chunks))
    return body


def _pkcs8_der(oid_str: str) -> bytes:
    """Мінімальний PKCS#8: SEQUENCE { INTEGER 0, SEQUENCE { OBJECT oid } }."""
    oid_body = _der_oid(oid_str)
    alg = b"\x30" + bytes([len(oid_body) + 2]) + b"\x06" + bytes([len(oid_body)]) + oid_body
    ver = b"\x02\x01\x00"
    inner = ver + alg + b"\x04\x00"
    return b"\x30" + bytes([len(inner)]) + inner


class TestDetectPkcs8Algorithm:
    def test_rsa_oid(self):
        der = _pkcs8_der("1.2.840.113549.1.1.1")
        assert PrroCryptoSigner._detect_pkcs8_algorithm(der) == "1.2.840.113549.1.1.1"

    def test_dstu4145_little_endian_oid(self):
        der = _pkcs8_der("1.2.804.2.1.1.1.1.3.1.1")
        assert PrroCryptoSigner._detect_pkcs8_algorithm(der) == "1.2.804.2.1.1.1.1.3.1.1"

    def test_dstu4145_big_endian_oid(self):
        der = _pkcs8_der("1.2.804.2.1.1.1.1.3.1.2")
        assert PrroCryptoSigner._detect_pkcs8_algorithm(der) == "1.2.804.2.1.1.1.1.3.1.2"

    def test_ecdsa_oid(self):
        der = _pkcs8_der("1.2.840.10045.2.1")
        assert PrroCryptoSigner._detect_pkcs8_algorithm(der) == "1.2.840.10045.2.1"

    def test_invalid_der_returns_none(self):
        assert PrroCryptoSigner._detect_pkcs8_algorithm(b"\x00\x01\x02") is None


@pytest.mark.skipif(not os.path.isfile(TEST_JKS), reason="Тестовий JKS-ключ відсутній")
class TestLoadJks:
    def test_dstu_jks_loads_and_detects(self, tmp_path):
        """JKS з ДСТУ 4145 ключем читається і дає зрозумілу помилку."""
        import shutil

        dst = tmp_path / "pb.jks"
        shutil.copy(TEST_JKS, dst)
        from app.infrastructure.services.prro.crypto_signer import PrroCryptoError

        with pytest.raises(PrroCryptoError) as ei:
            PrroCryptoSigner(
                key_path=str(dst),
                key_password=JKS_PASSWORD,
            )
        assert "ДСТУ 4145-2002" in str(ei.value)
