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

from app.infrastructure.services.prro.crypto_signer import (
    PrroCryptoError,
    PrroCryptoSigner,
)
from app.infrastructure.services.prro.grpc_client import (
    DEFAULT_INITIAL_BACKOFF_SECONDS,
    DEFAULT_MAX_RETRIES,
    DEFAULT_TIMEOUT_SECONDS,
    PING_LOCAL_NUMBER,
    PrroGrpcClient,
)
from app.infrastructure.services.prro.key_store import (
    PASSWORD_MASK,
    PrroKeyStore,
    PrroKeyStoreError,
)
from app.infrastructure.services.prro.offline_queue import (
    CHECK_TYPE_CHK,
    CHECK_TYPE_SERVICECHK,
    CHECK_TYPE_ZREPORT,
    PRRO_OFFLINE_LIMIT_HOURS,
    PrroOfflineQueue,
)
from app.infrastructure.services.prro.xml_builder import (
    CHK_TYPE_RETURN,
    CHK_TYPE_SALE,
    CHK_TYPE_SERVICE,
    SERVICE_OFFLINE,
    SERVICE_ONLINE,
    SERVICE_OPEN_SHIFT,
    SERVICE_PING,
    SERVICE_RESERVE,
    XmlBuilder,
    canonicalize,
    compute_mac,
)

__all__ = [
    "CHECK_TYPE_CHK",
    "CHECK_TYPE_SERVICECHK",
    "CHECK_TYPE_ZREPORT",
    "CHK_TYPE_RETURN",
    "CHK_TYPE_SALE",
    "CHK_TYPE_SERVICE",
    "DEFAULT_INITIAL_BACKOFF_SECONDS",
    "DEFAULT_MAX_RETRIES",
    "DEFAULT_TIMEOUT_SECONDS",
    "PASSWORD_MASK",
    "PING_LOCAL_NUMBER",
    "PRRO_OFFLINE_LIMIT_HOURS",
    "SERVICE_OFFLINE",
    "SERVICE_ONLINE",
    "SERVICE_OPEN_SHIFT",
    "SERVICE_PING",
    "SERVICE_RESERVE",
    "PrroCryptoError",
    # Криптографія
    "PrroCryptoSigner",
    # gRPC-клієнт
    "PrroGrpcClient",
    # Сховище ключів
    "PrroKeyStore",
    "PrroKeyStoreError",
    # Офлайн-черга
    "PrroOfflineQueue",
    # XML-білдер
    "XmlBuilder",
    "canonicalize",
    "compute_mac",
]
