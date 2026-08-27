"""Еталонний Python ping до тестового API ДПС (порівняння з Rust-фасадом)."""
import asyncio
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "backend"))

from app.infrastructure.services.prro.crypto_signer import PrroCryptoSigner
from app.infrastructure.services.prro.factory import PrroServiceFactory
from app.infrastructure.services.prro.xml_builder import XmlBuilder


async def main() -> None:
    fn = "3000898168"
    tn = "3791505547"
    zn = "TEST00000001"
    url = "cabinet.tax.gov.ua:9443"
    key_path = "certs/prro-test/nastya_key.jks"
    key_password = "prrotestkey22"

    # 1. XML T=111 (дефолти rro_type="0", version="1")
    builder = XmlBuilder(rro_fn=fn, tax_number=tn, factory_number=zn)
    dat_xml = builder.build_service_check_xml(service_type="111")
    message = builder.build_message(dat_xml, include_mac=False)
    print("MESSAGE:", message)

    # 2. Підпис XAdES (як у Python-проді)
    signer = PrroCryptoSigner(key_path=key_path, key_password=key_password)
    signed = signer.sign(message.encode("utf-8"))
    print("SIGNED_LEN:", len(signed))
    print("SIGNED_HEAD:", signed[:200])

    # 3. gRPC ping
    factory = PrroServiceFactory()
    client = factory.grpc_client(url=url, rro_fn=fn)
    resp = await client.ping(check_sign=signed)
    print("PY_STATUS:", resp.status)
    print("PY_ERROR_MSG:", resp.error_message)
    await factory.close()


if __name__ == "__main__":
    asyncio.run(main())
