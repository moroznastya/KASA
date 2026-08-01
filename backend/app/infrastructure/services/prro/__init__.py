"""
Пакет інфраструктури ПРРО (програмний РРО).

Містить:
  - prro.proto         — опис gRPC-інтерфейсів фіскального сервера ДПС;
  - prro_pb2.py        — згенеровані повідомлення (Check, CheckResponse, ...);
  - prro_pb2_grpc.py   — згенерований стаб сервісу ChkIncomeService;
  - grpc_client.py     — асинхронний gRPC-клієнт з таймаутами та ретраями;
  - xml_builder.py     — побудова XML СЗЗД 2.1.7 (чеки, Z-звіти, службові);
  - crypto_signer.py   — XAdES-підписання XML (pfx/p12/jks/pem/dat);
  - key_store.py       — безпечне зберігання шляху/пароля ключа (Fernet);
  - offline_queue.py   — черга офлайн-документів (ліміт 168 годин).

Джерела протоколу: docs/scr/_site_text.txt,
                   docs/scr/SZZD_RRO_Protokol_peredach_nformats_2_1_7.doc
"""

from app.infrastructure.services.prro.grpc_client import (
    PrroGrpcClient,
    PING_LOCAL_NUMBER,
    DEFAULT_MAX_RETRIES,
    DEFAULT_INITIAL_BACKOFF_SECONDS,
    DEFAULT_TIMEOUT_SECONDS,
)
from app.infrastructure.services.prro.xml_builder import (
    XmlBuilder,
    canonicalize,
    compute_mac,
    CHK_TYPE_SALE,
    CHK_TYPE_RETURN,
    CHK_TYPE_SERVICE,
    SERVICE_OPEN_SHIFT,
    SERVICE_OFFLINE,
    SERVICE_ONLINE,
    SERVICE_PING,
    SERVICE_RESERVE,
)
from app.infrastructure.services.prro.crypto_signer import (
    PrroCryptoSigner,
    PrroCryptoError,
)
from app.infrastructure.services.prro.key_store import (
    PrroKeyStore,
    PrroKeyStoreError,
    PASSWORD_MASK,
)
from app.infrastructure.services.prro.offline_queue import (
    PrroOfflineQueue,
    PRRO_OFFLINE_LIMIT_HOURS,
    CHECK_TYPE_CHK,
    CHECK_TYPE_ZREPORT,
    CHECK_TYPE_SERVICECHK,
)

__all__ = [
    # gRPC-клієнт
    "PrroGrpcClient",
    "PING_LOCAL_NUMBER",
    "DEFAULT_MAX_RETRIES",
    "DEFAULT_INITIAL_BACKOFF_SECONDS",
    "DEFAULT_TIMEOUT_SECONDS",
    # XML-білдер
    "XmlBuilder",
    "canonicalize",
    "compute_mac",
    "CHK_TYPE_SALE",
    "CHK_TYPE_RETURN",
    "CHK_TYPE_SERVICE",
    "SERVICE_OPEN_SHIFT",
    "SERVICE_OFFLINE",
    "SERVICE_ONLINE",
    "SERVICE_PING",
    "SERVICE_RESERVE",
    # Криптографія
    "PrroCryptoSigner",
    "PrroCryptoError",
    # Сховище ключів
    "PrroKeyStore",
    "PrroKeyStoreError",
    "PASSWORD_MASK",
    # Офлайн-черга
    "PrroOfflineQueue",
    "PRRO_OFFLINE_LIMIT_HOURS",
    "CHECK_TYPE_CHK",
    "CHECK_TYPE_ZREPORT",
    "CHECK_TYPE_SERVICECHK",
]
