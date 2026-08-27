"""
gRPC-клієнт для взаємодії з фіскальним сервером ДПС України (ПРРО).

Обгортка над згенерованим сервісом `ChkIncomeService` (prro_pb2_grpc).
Реалізує асинхронні методи (grpcio.aio) з таймаутами та ретраями
(2-3 спроби з експоненційним бек-офом).

Протокол: docs/scr/_site_text.txt (розділ «Опис інтерфейсів»)
XML СЗЗД:  docs/scr/SZZD_RRO_Protokol_peredach_nformats_2_1_7.doc

Адреси:
  - тестове API: cabinet.tax.gov.ua:9443  (чеки НЕ фіскальні)
  - бойове API:  prro.tax.gov.ua:443

ВАЖЛИВО:
  - метод `sendChk` діє лише до 01.10.2021;
  - з 01.10.2021 використовується `sendChkV2`;
  - для відкриття зміни local_number == 0;
  - для перевірки зв'язку local_number == 0x7FFFFFFF, check_type=SERVICECHK;
  - підписаний XML (check_sign) формується в xml_builder.py (Фаза 1).

ФОРМАТ DATE_TIME:
  Офіційний семпл ДПС (github.com/programika/prro_sample, Sender.java)
  передає `date_time` у форматі `yyyyMMddHHmmss` (14 цифр, локальний час),
  а НЕ Unix epoch:
      long dateTime = Long.parseLong(
          new SimpleDateFormat("yyyyMMddHHmmss").format(check.getDate()));
  Тому тут використовується той самий формат (див. _check_date_time).

PING (перевірка зв'язку):
  Документація API (docs/scr/_site_text.txt):
    «Для перевірки зв'язку використовується метод ping (Check) який повертає
     CheckResponse та XML з типом <CT="111">. MAC не заповнюється.»
  Тобто check_sign МАЄ містити XML службового чеку T=111 (не порожній!).
  Якщо КЕП доступний — XML підписується (XAdES); інакше передається
  непідписаний (сервер поверне ERROR_VEREFY -1, доки підписант
  не буде зареєстрований / ключ не буде прочитано).
"""

from __future__ import annotations

import asyncio
import logging
from datetime import datetime
from typing import TypeVar

import grpc
from grpc import aio

from app.infrastructure.services.prro import prro_pb2
from app.infrastructure.services.prro import prro_pb2_grpc

logger = logging.getLogger(__name__)

# Максимальне значення int32 — використовується для перевірки зв'язку (ping)
PING_LOCAL_NUMBER = 0x7FFFFFFF


def _check_date_time(date_time: datetime | None = None) -> int:
    """
    Повертає date_time у форматі, який очікує фіскальний сервер ДПС.

    Офіційний семпл (programika/prro_sample, Sender.java) передає час у
    форматі `yyyyMMddHHmmss` (14 цифр), а не Unix epoch. Цей формат
    підтверджено оригінальним клієнтом ДПС — використовуємо його.
    """
    return int((date_time or datetime.now()).strftime("%Y%m%d%H%M%S"))

# Кількість спроб та початкова затримка для ретраїв
DEFAULT_MAX_RETRIES = 3
DEFAULT_INITIAL_BACKOFF_SECONDS = 1.0
DEFAULT_TIMEOUT_SECONDS = 30.0

# Тип відповіді gRPC (для загального коду ретраїв)
_RESP = TypeVar("_RESP")


class PrroGrpcClient:
    """
    Асинхронний gRPC-клієнт сервісу ChkIncomeService фіскального сервера ДПС.

    Args:
        channel: асинхронний gRPC-канал (grpc.aio.Channel).
        rro_fn: фіскальний номер ПРРО (ФН). Якщо вказано — підставляється
            у повідомлення Check автоматично.
    """

    def __init__(
        self,
        channel: aio.Channel,
        rro_fn: str | None = None,
        rro_fn_sign: bytes | None = None,
    ) -> None:
        self._channel = channel
        self._rro_fn = rro_fn
        # B3: підписаний ФН ПРРО (тим самим КЕП-ключем, що й check_sign).
        # Передається готовим з use-case (там є crypto signer); у statusRro/
        # infoRro/lastChk/delLastChk/delLastChkId — завжди непустий.
        self._rro_fn_sign = rro_fn_sign if rro_fn_sign else b""
        self._stub = prro_pb2_grpc.ChkIncomeServiceStub(channel)
        logger.info(
            "PRRO_GRPC_CLIENT_INIT | rro_fn=%s channel_ready=%s",
            rro_fn or "не задано", channel is not None,
        )

    # ─── Допоміжні методи ──────────────────────────────────────────────────

    def _make_check(
        self,
        *,
        check_sign: bytes = b"",
        local_number: int = 0,
        check_type: prro_pb2.Check.Type = prro_pb2.Check.CHK,
        date_time: int | None = None,
        id_offline: str = "",
        id_cancel: str = "",
    ) -> prro_pb2.Check:
        """
        Формує повідомлення Check з урахуванням rro_fn з конструктора.

        Args:
            check_sign: підписаний XML-документ СЗЗД (bytes).
            local_number: локальний номер чеку.
            check_type: тип чеку (CHK / ZREPORT / SERVICECHK).
            date_time: Unix epoch у секундах. Якщо None — поточний час UTC.
            id_offline: офлайн-ідентифікатор.
            id_cancel: ідентифікатор чеку, який скасовується.

        Returns:
            prro_pb2.Check — готове повідомлення для відправки.
        """
        if date_time is None:
            date_time = _check_date_time()

        return prro_pb2.Check(
            rro_fn=self._rro_fn or "",
            date_time=date_time,
            check_sign=check_sign,
            local_number=local_number,
            check_type=check_type,
            id_offline=id_offline,
            id_cancel=id_cancel,
        )

    async def _call_with_retry(
        self,
        rpc_method,
        request,
        *,
        timeout: float = DEFAULT_TIMEOUT_SECONDS,
        max_retries: int = DEFAULT_MAX_RETRIES,
        initial_backoff: float = DEFAULT_INITIAL_BACKOFF_SECONDS,
    ) -> _RESP:
        """
        Викликає RPC-метод з таймаутом та ретраями.

        Args:
            rpc_method: асинхронний метод стаба (наприклад, self._stub.sendChkV2).
            request: повідомлення-запит (Check / CheckRequest / CheckRequestId).
            timeout: таймаут одного виклику в секундах.
            max_retries: максимальна кількість спроб.
            initial_backoff: початкова затримка (сек) перед повторною спробою.

        Returns:
            Відповідь gRPC (CheckResponse / StatusResponse / RroInfoResponse).

        Raises:
            grpc.RpcError: якщо всі спроби вичерпано.
        """
        last_error: grpc.RpcError | None = None
        for attempt in range(1, max_retries + 1):
            try:
                response = await rpc_method(request, timeout=timeout)
                logger.info(
                    "PRRO_GRPC_CALL_OK | method=%s attempt=%d/%d",
                    getattr(rpc_method, "__name__", rpc_method),
                    attempt, max_retries,
                )
                return response
            except grpc.RpcError as e:
                last_error = e
                code = e.code() if hasattr(e, "code") else None
                logger.warning(
                    "PRRO_GRPC_CALL_ERR | attempt=%d/%d code=%s details=%s",
                    attempt, max_retries, code, e.details() if hasattr(e, "details") else e,
                )
                if attempt < max_retries:
                    delay = initial_backoff * (2 ** (attempt - 1))
                    logger.info("PRRO_GRPC_RETRY | delay=%.1fs", delay)
                    await asyncio.sleep(delay)

        raise last_error  # type: ignore[misc]

    # ─── Основні методи ────────────────────────────────────────────────────

    async def send_chk(
        self,
        check: prro_pb2.Check,
        *,
        timeout: float = DEFAULT_TIMEOUT_SECONDS,
    ) -> prro_pb2.CheckResponse:
        """
        Передає чек / Z-звіт на фіскальний сервер (метод sendChkV2).

        Args:
            check: повідомлення Check з підписаним XML у check_sign.
            timeout: таймаут виклику в секундах.

        Returns:
            CheckResponse — відповідь фіскального сервера (id, status, ...).

        Raises:
            grpc.RpcError: якщо сервер недоступний (після всіх ретраїв).
        """
        logger.info(
            "PRRO_SEND_CHK | rro_fn=%s local_number=%d check_type=%s",
            check.rro_fn, check.local_number, check.check_type,
        )
        # H1: фіскальний документ — БЕЗ сліпих ретраїв. Якщо транспортна
        # помилка, fiscalize робить lastChk-перевірку і ТІЛЬКИ тоді повторює
        # send (контрольований retry без ризику дубліката).
        try:
            response = await self._stub.sendChkV2(check, timeout=timeout)
            logger.info("PRRO_GRPC_CALL_OK | method=sendChkV2 attempt=1/1")
            return response
        except grpc.RpcError as e:
            code = e.code() if hasattr(e, "code") else None
            logger.warning(
                "PRRO_GRPC_CALL_ERR | method=sendChkV2 attempt=1/1 code=%s details=%s",
                code, e.details() if hasattr(e, "details") else e,
            )
            raise

    async def ping(
        self,
        *,
        check_sign: bytes = b"",
        timeout: float = DEFAULT_TIMEOUT_SECONDS,
    ) -> prro_pb2.CheckResponse:
        """
        Перевірка зв'язку з фіскальним сервером (метод ping).

        Формує службовий Check:
          - local_number = 0x7FFFFFFF (максимальне значення int32);
          - check_type = SERVICECHK;
          - check_sign = XML службового чеку T=111 (підписаний, якщо КЕП
            доступний; MAC не заповнюється — згідно з документацією API:
            «XML з типом <CT=\"111\">. MAC не заповнюється»).

        Args:
            check_sign: XML-документ СЗЗД службового чеку (T=111).
                Якщо порожній — сервер поверне ERROR_VEREFY (-1),
                бо не зможе розібрати повідомлення.
            timeout: таймаут виклику в секундах.

        Returns:
            CheckResponse — відповідь сервера. Навіть якщо ПРРО не
            зареєстровано, канал gRPC має бути живим, а статус —
            ERROR_NOT_REGISTERED_RRO або подібний.

        Raises:
            grpc.RpcError: якщо з'єднання неможливе (після всіх ретраїв).
        """
        check = self._make_check(
            check_sign=check_sign,
            local_number=PING_LOCAL_NUMBER,
            check_type=prro_pb2.Check.SERVICECHK,
        )
        logger.info(
            "PRRO_PING | rro_fn=%s check_sign_len=%d",
            check.rro_fn, len(check_sign),
        )
        return await self._call_with_retry(self._stub.ping, check, timeout=timeout)

    async def status(
        self,
        *,
        timeout: float = DEFAULT_TIMEOUT_SECONDS,
    ) -> prro_pb2.StatusResponse:
        """
        Отримує статус ПРРО (метод statusRro).

        Returns:
            StatusResponse — open_shift, online, last_signer, status.

        Raises:
            grpc.RpcError: якщо сервер недоступний (після всіх ретраїв).
        """
        request = prro_pb2.CheckRequest(rro_fn_sign=self._rro_fn_sign)
        logger.info("PRRO_STATUS | rro_fn=%s", self._rro_fn or "не задано")
        return await self._call_with_retry(self._stub.statusRro, request, timeout=timeout)

    async def info(
        self,
        *,
        timeout: float = DEFAULT_TIMEOUT_SECONDS,
    ) -> prro_pb2.RroInfoResponse:
        """
        Отримує детальну інформацію про ПРРО (метод infoRro).

        Returns:
            RroInfoResponse — статус ПРРО, касири, податкові номери тощо.

        Raises:
            grpc.RpcError: якщо сервер недоступний (після всіх ретраїв).
        """
        request = prro_pb2.CheckRequest(rro_fn_sign=self._rro_fn_sign)
        logger.info("PRRO_INFO | rro_fn=%s", self._rro_fn or "не задано")
        return await self._call_with_retry(self._stub.infoRro, request, timeout=timeout)

    async def last_chk(
        self,
        *,
        timeout: float = DEFAULT_TIMEOUT_SECONDS,
    ) -> prro_pb2.CheckResponse:
        """
        Отримує останній переданий чек (метод lastChk).

        Returns:
            CheckResponse — у полі data_sign міститься останній чек.

        Raises:
            grpc.RpcError: якщо сервер недоступний (після всіх ретраїв).
        """
        request = prro_pb2.CheckRequest(rro_fn_sign=self._rro_fn_sign)
        logger.info("PRRO_LAST_CHK | rro_fn=%s", self._rro_fn or "не задано")
        return await self._call_with_retry(self._stub.lastChk, request, timeout=timeout)

    async def del_last_chk(
        self,
        *,
        timeout: float = DEFAULT_TIMEOUT_SECONDS,
    ) -> prro_pb2.CheckResponse:
        """
        Вилучає останній чек (метод delLastChk).

        УВАГА: можна використовувати тільки 1 раз і тільки для чеку продажу
        (наприклад, при обриві зв'язку та переході в офлайн режим).

        Returns:
            CheckResponse — результат вилучення.

        Raises:
            grpc.RpcError: якщо сервер недоступний (після всіх ретраїв).
        """
        request = prro_pb2.CheckRequest(rro_fn_sign=self._rro_fn_sign)
        logger.info("PRRO_DEL_LAST_CHK | rro_fn=%s", self._rro_fn or "не задано")
        return await self._call_with_retry(self._stub.delLastChk, request, timeout=timeout)

    async def del_last_chk_id(
        self,
        check_id: str,
        *,
        timeout: float = DEFAULT_TIMEOUT_SECONDS,
    ) -> prro_pb2.CheckResponse:
        """
        Вилучає чек за ідентифікатором (метод delLastChkId).

        УВАГА: можна використовувати тільки 1 раз і тільки для чеку продажу.
        Якщо ID відповідає останньому чеку — чек буде вилучено.

        Args:
            check_id: ідентифікатор чеку (з CheckResponse.id).

        Returns:
            CheckResponse — результат вилучення.

        Raises:
            grpc.RpcError: якщо сервер недоступний (після всіх ретраїв).
        """
        request = prro_pb2.CheckRequestId(id=check_id, rro_fn_sign=self._rro_fn_sign)
        logger.info("PRRO_DEL_LAST_CHK_ID | check_id=%s", check_id)
        return await self._call_with_retry(self._stub.delLastChkId, request, timeout=timeout)

    async def open_shift(
        self,
        check_sign: bytes,
        *,
        timeout: float = DEFAULT_TIMEOUT_SECONDS,
    ) -> prro_pb2.CheckResponse:
        """
        Відкриває зміну (службовий чек з local_number = 0).

        Args:
            check_sign: підписаний XML службового чеку відкриття зміни
                (тип T=108 з 01.10.2021, формується в xml_builder.py).
            timeout: таймаут виклику в секундах.

        Returns:
            CheckResponse — відповідь фіскального сервера.

        Raises:
            grpc.RpcError: якщо сервер недоступний (після всіх ретраїв).
        """
        check = self._make_check(
            check_sign=check_sign,
            local_number=0,
            check_type=prro_pb2.Check.CHK,
        )
        logger.info("PRRO_OPEN_SHIFT | rro_fn=%s", check.rro_fn)
        return await self._call_with_retry(self._stub.sendChkV2, check, timeout=timeout)

    async def close(self) -> None:
        """Закриває gRPC-канал (якщо він був створений цим клієнтом)."""
        logger.info("PRRO_GRPC_CLIENT_CLOSE")
        if self._channel is not None:
            await self._channel.close()
