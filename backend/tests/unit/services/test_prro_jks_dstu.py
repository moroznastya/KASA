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
        """JKS з ДСТУ 4145 ключем читається, визначається і перемикає бекенд на ІІТ.

        Раніше (до інтеграції крипто-ядра ІІТ SDK) цей ключ викликав
        PrroCryptoError з повідомленням про необхідність SDK EUSign.
        Тепер ключ приймається: бекенд = "iit", матеріали ключа
        завантажуються ліниво через IitSdk.
        """
        import shutil

        dst = tmp_path / "pb.jks"
        shutil.copy(TEST_JKS, dst)
        signer = PrroCryptoSigner(
            key_path=str(dst),
            key_password=JKS_PASSWORD,
        )
        assert signer._backend == "iit"
        assert signer._iit_jks_path is not None
        assert signer._iit_jks_password == JKS_PASSWORD

    @pytest.mark.integration
    def test_dstu_jks_signs_and_verifies_via_iit(self, tmp_path):
        """ДСТУ 4145 підпис через крипто-ядро ІІТ (інтеграційний, skip без SDK).

        Перевіряє повний ланцюжок: JKS → IitSdk → CAdES-BES підпис →
        самоперевірка підпису. Виконується лише якщо SDK встановлено
        (backend/vendor/iit-sdk/opt/iit/eu/sw/euscp.so).
        """
        from app.infrastructure.services.prro.iit_sdk import IitSdk

        IitSdk.reset()
        sdk = IitSdk.get()
        if not sdk.available:
            pytest.skip("Крипто-ядро ІІТ не встановлено (backend/scripts/setup_iit_sdk.sh)")

        import shutil

        dst = tmp_path / "pb.jks"
        shutil.copy(TEST_JKS, dst)
        signer = PrroCryptoSigner(
            key_path=str(dst),
            key_password=JKS_PASSWORD,
        )
        xml = (
            '<?xml version="1.0" encoding="windows-1251"?>\n'
            '<RQ V="1">\n<DAT DI="162292" DT="0" FN="4000000001" '
            'TN="\u041f\u041d 2080903659" V="1" ZN="402342434">\n'
            '<C T="111">\n</C>\n<TS>20260801160000</TS>\n</DAT>\n</RQ>\n'
        ).encode("cp1251")
        signature = signer.sign(xml)
        assert len(signature) > 100
        # CAdES-BES: ContentInfo з signedData (1.2.840.113549.1.7.2)
        assert signature[:4] == b"\x30\x82" or signature[0] == 0x30
        assert signer.verify(signature) is True
        assert signer.get_serial_number()
        assert signer.get_signer_name()
