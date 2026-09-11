"""
Побудова XML-документів ПРРО для передачі на фіскальний сервер ДПС
(API ФСКО, тест. API `cabinet.tax.gov.ua:9443`).

Джерело: `resources/262576.docx` («Опис API фіскального сервера…») +
        `docs/scr/SZZD_RRO_Protokol_peredach_nformats_2_1_7.doc` (структура чеку).

СТРУКТУРА ПОВІДОМЛЕННЯ (ПРРО, розділ «XML-формати чеку та Z-звіту»):
    <?xml version="1.0" encoding="windows-1251"?>
    <RQ V="1">
      <DAT FN="..." TN="..." ZN="..." DI="..." DT="0" V="1">
        {зміст пакету даних}   ← <C ...> чек / <Z ...> Z-звіт
        <TS>YYYYMMDDHHMMSS</TS>
      </DAT>
      <MAC ID="">{hex}</MAC>
    </RQ>

- В повідомленні може бути ТІЛЬКИ одна пара <DAT>…<MAC>… (ПРРО).
- <TS> — ЛОКАЛЬНИЙ час у форматі YYYYMMDDhhmmss (не UTC, не epoch).
- <MAC> атрибути: лише ID (без DI/NT):
    * ID=""               — онлайн-документи;
    * ID={резервний №}    — офлайн-документи (той самий номер, що й id_offline);
    * T=112 — без атрибута ID; T=111 (ping) — <MAC></MAC> без значення.
- Значення MAC = hex (lowercase) sha256 XML ПОПЕРЕДНЬОГО повідомлення Check
  (повний RQ-документ, байти windows-1251). Перший документ ПРРО після
  реєстрації/скидання ланцюга — значення ПОРОЖНЄ. <H>-ланцюжка в тілі чеку
  НЕМАЄ — ланцюг контролюється самим <MAC>.
- Кодування: windows-1251, XML-декларація обов'язкова.

КАНОНІЧНИЙ ВИГЛЯД (Додаток А СЗЗД):
    1. Всі пробіли/табуляції/переноси між тегами видаляються.
    2. Теги завжди закриті (<tag/> → <tag></tag>).
    3. Атрибути — в АЛФАВІТНОМУ порядку в кожному тегу.

ФОРМАТ ЧЕКУ (СЗЗД §7.1): суми — копійки (×100), кількість ×1000.
"""

from __future__ import annotations

import hashlib
import re
from datetime import datetime
from decimal import ROUND_HALF_UP, Decimal
from typing import Any

from lxml import etree

# ─── XML-декларація (обов'язкова, windows-1251) ─────────────────────────────
XML_DECLARATION = '<?xml version="1.0" encoding="windows-1251"?>'

# ─── Коди типів чеку в <C T="..."> ───────────────────────────────────────────
CHK_TYPE_SALE = "0"        # Чек продажу
CHK_TYPE_RETURN = "1"      # Чек повернення
CHK_TYPE_SERVICE = "2"     # Службовий чек (внесення коштів)

# ─── Коди службових чеків з 01.10.2021 (sendChkV2) ───────────────────────────
SERVICE_OPEN_SHIFT = "108"   # Відкриття зміни
SERVICE_OFFLINE = "109"      # Перехід в офлайн
SERVICE_ONLINE = "110"       # Перехід в онлайн
SERVICE_PING = "111"         # Перевірка доступності ПРРО (ping)
SERVICE_RESERVE = "112"      # Запит діапазону резервних номерів

# Всі допустимі типи службових чеків
SERVICE_TYPES = frozenset(
    {SERVICE_OPEN_SHIFT, SERVICE_OFFLINE, SERVICE_ONLINE, SERVICE_PING, SERVICE_RESERVE}
)

# Розмір діапазону резервних номерів для T=112 (за зразком ДПС)
DEFAULT_RESERVE_SIZE = 150

# Шаблон для визначення типу службового чеку <C T="..."> всередині <DAT>
_C_TYPE_PATTERN = re.compile(r"<C\b[^>]*\bT=\"(\d+)\"")


# ─── Утиліти для роботи з числами та XML ────────────────────────────────────

def _as_decimal(value: Decimal | float | str | int) -> Decimal:
    """Перетворює значення на Decimal без втрати точності."""
    if isinstance(value, Decimal):
        return value
    return Decimal(str(value))


def _to_cents(amount: Decimal | float | str | int) -> int:
    """
    Перетворює суму в гривнях на копійки (грн × 100) для XML СЗЗД.

    Args:
        amount: сума в гривнях (Decimal, float, int або str).

    Returns:
        int — сума в копійках, округлена до цілого (ROUND_HALF_UP).
    """
    value = _as_decimal(amount)
    return int((value * 100).quantize(Decimal("1"), rounding=ROUND_HALF_UP))


def _to_thousandths(quantity: Decimal | float | str | int) -> int:
    """
    Перетворює кількість товару у тисячні частки (кількість × 1000).

    Args:
        quantity: кількість товару (Decimal, float, int або str).

    Returns:
        int — кількість × 1000, округлена до цілого (ROUND_HALF_UP).
    """
    value = _as_decimal(quantity)
    return int((value * 1000).quantize(Decimal("1"), rounding=ROUND_HALF_UP))


def _format_percent(value: Decimal | float | str | int) -> str:
    """
    Форматує відсоток у вигляді "00.00" (наприклад, 20 → "20.00").

    Args:
        value: значення відсотка.

    Returns:
        str — відсоток з двома десятковими знаками.
    """
    dec = _as_decimal(value)
    return f"{dec:.2f}"


def _esc_text(value: str) -> str:
    """Екранує спеціальні символи XML у текстовому вмісті."""
    return (
        value.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
    )


def _esc_attr(value: str) -> str:
    """Екранує спеціальні символи XML у значенні атрибута."""
    return _esc_text(value).replace('"', "&quot;")


def _fmt_datetime(date_time: datetime) -> str:
    """Форматує дату/час у формат YYYYMMDDhhmmss (тег <TS>)."""
    return date_time.strftime("%Y%m%d%H%M%S")


def _fmt_date(date_time: datetime) -> str:
    """Форматує дату у формат YYYYMMDD (тег <TS> у <TXS>)."""
    return date_time.strftime("%Y%m%d")


def signed_bytes_to_text(signed: bytes) -> str:
    """
    Перетворює підписані байти у текст для колонки Text (check_sign).

    Для XAdES-бекенду підпис — XML у windows-1251 → декодується у текст;
    для бінарного CAdES (ІІТ) — base64 з префіксом "b64:" (sync кодує назад).
    """
    import base64 as _b64

    for enc in ("cp1251", "utf-8"):
        try:
            return signed.decode(enc)
        except (UnicodeDecodeError, ValueError):
            continue
    return f"b64:{_b64.b64encode(signed).decode('ascii')}"


def cp1251_bytes(message: str) -> bytes:
    """
    Кодує повне RQ-повідомлення у байти windows-1251.

    Саме ці байти підписуються (CAdES/XAdES) і саме над ними обчислюється
    хеш-ланцюжок (MAC наступного чеку). Символи поза windows-1251 →
    UnicodeEncodeError (краще явна помилка, ніж мовчазне спотворення).

    Args:
        message: повне повідомлення <RQ>…</RQ> (з XML-декларацією).

    Returns:
        bytes — документ у кодуванні windows-1251.
    """
    return message.encode("cp1251")


# ─── Канонічний вигляд (Додаток А) ──────────────────────────────────────────

def _canonical_serialize(element: etree._Element) -> str:
    """
    Рекурсивно серіалізує елемент у канонічному вигляді СЗЗД.

    Правила:
      - атрибути — в алфавітному порядку;
      - теги завжди закриті (<tag></tag>, без <tag/>);
      - пробіли між тегами видаляються, текстовий вміст зберігається.
    """
    attrs = "".join(
        f' {name}="{_esc_attr(value)}"' for name, value in sorted(element.attrib.items())
    )
    tag_open = f"<{element.tag}{attrs}>"

    # Текстовий вміст: пробіли/переноси між тегами видаляються,
    # але змістовний текст (з непробільними символами) зберігається.
    text = element.text or ""
    if not text.strip():
        text = ""

    if len(element) == 0:
        return f"{tag_open}{text}</{element.tag}>"

    parts: list[str] = [tag_open, text]
    for child in element:
        parts.append(_canonical_serialize(child))
        tail = child.tail or ""
        if not tail.strip():
            tail = ""
        parts.append(tail)
    parts.append(f"</{element.tag}>")
    return "".join(parts)


def canonicalize(xml_str: str) -> str:
    """
    Приводить XML пакету даних до канонічного вигляду (Додаток А СЗЗД 2.1.7).

    Правила:
        1. Видаляються всі пробіли/табуляції/переноси рядків між тегами,
           перед першим та після останнього тегу.
        2. Всі теги приводяться до вигляду <tag_name …>…</tag_name>
           (самозакривні <tag/> → <tag></tag>).
        3. Пробіли в старт-тегах (окрім значень атрибутів) замінюються
           на один пробіл.
        4. Всі атрибути всередині кожного тегу розміщуються
           в АЛФАВІТНОМУ порядку.

    Args:
        xml_str: вихідний XML пакету даних (може містити XML-декларацію).

    Returns:
        str — канонічний XML (без XML-декларації, тільки кореневий тег).

    Raises:
        ValueError: якщо вхідний рядок не є коректним XML.
    """
    if not xml_str or not xml_str.strip():
        raise ValueError("Порожній XML-документ")

    parser = etree.XMLParser(remove_blank_text=False, resolve_entities=False)
    try:
        root = etree.fromstring(xml_str.encode("utf-8"), parser=parser)
    except etree.XMLSyntaxError as exc:
        raise ValueError(f"Некоректний XML: {exc}") from exc

    return _canonical_serialize(root)


# ─── Хеш-ланцюжок / MAC ──────────────────────────────────────────────────────

def _service_type_of(dat_xml: str) -> str | None:
    """Визначає тип службового чеку <C T="..."> у канонічному <DAT> (або None)."""
    match = _C_TYPE_PATTERN.search(dat_xml)
    if match is None:
        return None
    return match.group(1)


def _mac_tag(dat_xml: str, mac_value: str, mac_id: str) -> str:
    """
    Будує тег <MAC> за зразками ДПС (262576.docx).

    - T=111 (ping): `<MAC></MAC>` — без ID, без значення;
    - T=112 (резерв): `<MAC>{value}</MAC>` — без атрибута ID;
    - решта: `<MAC ID="{mac_id}">{value}</MAC>` — ID="" для онлайн,
      ID={резервний фіскальний номер} для офлайн-документів.
    """
    service = _service_type_of(dat_xml)
    if service == SERVICE_PING:
        return "<MAC></MAC>"
    if service == SERVICE_RESERVE:
        return f"<MAC>{_esc_text(mac_value)}</MAC>"
    return f'<MAC ID="{_esc_attr(mac_id)}">{_esc_text(mac_value)}</MAC>'


def compute_mac(data: str | bytes) -> str:
    """
    Обчислює MAC = hex (lowercase) sha256 XML попереднього Check.

    Джерело (API ФСКО): «Значення MAC розраховується за допомогою функції
    гешування sha256 попереднього чеку … Геш записується як хекс строка».
    Хешується ПОВНИЙ RQ-документ попереднього повідомлення у байтах
    windows-1251 (те, що «бачить» сервер через CAdES).

    Args:
        data: повне RQ-повідомлення попереднього Check (str — кодується
            у windows-1251; bytes — використовуються як є).

    Returns:
        str — 64-символьний hex sha256 (lowercase). Для першого документа
        ПРРО (ланцюг порожній) значення MAC не заповнюється ("").
    """
    raw = data.encode("cp1251") if isinstance(data, str) else data
    if not raw:
        return ""
    return hashlib.sha256(raw).hexdigest()


# Аліас: назва, що описує призначення в коді ланцюга.
chain_hash = compute_mac


def reconstruct_full_rq(
    dat_xml: str,
    mac_value: str,
    *,
    mac_id: str = "",
) -> str:
    """
    Відтворює ПОВНЕ RQ-повідомлення зі збережених частин.

    Використовується для обчислення хешу ланцюга документа з офлайн-черги:
    sha256(повний RQ попереднього Check). Відтворення детерміноване і
    байт-ідентичне оригінальному build_message(), якщо збережені ті самі
    частини (xml_body=канонічний <DAT>, mac=значення <MAC> документа,
    id_offline → ID тега MAC для офлайн-чеків).

    Args:
        dat_xml: канонічний XML <DAT>…</DAT> (як збережено у черзі).
        mac_value: значення <MAC> документа (hex-ланцюг попереднього RQ).
        mac_id: ID тега <MAC> (для офлайн-документів = резервний №; інакше "").

    Returns:
        str — повне повідомлення з XML-декларацією windows-1251.
    """
    dat_xml = canonicalize(dat_xml)
    mac_value = mac_value or ""
    return (
        f"{XML_DECLARATION}<RQ V=\"1\">"
        f"{dat_xml}{_mac_tag(dat_xml, mac_value, mac_id or '')}</RQ>"
    )


def extract_check_no(xml: str) -> int | None:
    """Дістає NO (номер операції) з XML чека — H1: lastChk повертає XML
    останнього чека в data_sign; NO == local_number (Totals.fiscal_number),
    тому за збігом NO ідентифікуємо "наш" чек. 1:1 Rust `extract_check_no`."""
    import re as _re

    match = _re.search(r'<E[^>]*NO="(\d+)"', xml)
    if match is None:
        return None
    return int(match.group(1))


# ─── Білдер XML ─────────────────────────────────────────────────────────────

class XmlBuilder:
    """
    Побудова XML-документів ПРРО для фіскалізації через gRPC.

    Конструктор отримує реквізити ПРРО, які підставляються в атрибути <DAT>.
    """

    def __init__(
        self,
        rro_fn: str,           # Фіскальний номер ПРРО
        tax_number: str,       # Податковий номер / індивідуальний номер платника ПДВ
        factory_number: str,   # Заводський номер РРО
        rro_type: str = "0",   # Тип РРО (0 — загального призначення)
        version: str = "1",    # Версія формату пакету даних
        *,
        initial_packet_id: int = 0,
        initial_mac_number: int = 0,
    ) -> None:
        """
        Ініціалізує білдер з реквізитами ПРРО.

        Args:
            rro_fn: фіскальний номер ПРРО (FN).
            tax_number: податковий номер платника (TN).
            factory_number: заводський номер РРО (ZN).
            rro_type: тип РРО (DT), за замовчуванням "0".
            version: версія формату пакету даних (V), за замовчуванням "1".
            initial_packet_id: початковий лічильник пакетів (DI).
            initial_mac_number: початковий лічильник MAC (NT; збережено для
                сумісності — ПРРО-формат не використовує NT).
        """
        self.rro_fn = rro_fn
        self.tax_number = tax_number
        self.factory_number = factory_number
        self.rro_type = rro_type
        self.version = version

        # Монотонний лічильник DI — унікальний в межах одного ПРРО
        self._packet_id = int(initial_packet_id)
        self._mac_number = int(initial_mac_number)

    # ─── Лічильники ─────────────────────────────────────────────────────────

    def _next_packet_id(self) -> int:
        """Повертає наступний унікальний ідентифікатор пакету даних (DI)."""
        self._packet_id += 1
        return self._packet_id

    def _next_mac_number(self) -> int:
        """Повертає наступний порядковий номер MAC (NT) — для сумісності."""
        self._mac_number += 1
        return self._mac_number

    @property
    def last_packet_id(self) -> int:
        """Останній виданий ідентифікатор пакету даних (DI)."""
        return self._packet_id

    @property
    def last_mac_number(self) -> int:
        """Останній виданий порядковий номер MAC (NT)."""
        return self._mac_number

    # ─── Формування <DAT> ───────────────────────────────────────────────────

    def _build_dat(
        self,
        body_xml: str,
        ts: str,
        *,
        di: int | None = None,
    ) -> str:
        """
        Обгортає вміст у тег <DAT> з реквізитами ПРРО та <TS>.

        Args:
            body_xml: вміст пакету даних (<C>…</C> або <Z>…</Z>).
            ts: дата/час у форматі YYYYMMDDhhmmss.
            di: ідентифікатор пакету даних; якщо None — береться з лічильника.

        Returns:
            str — НЕканонічний XML <DAT> (canonicalize застосовує caller).
        """
        packet_id = di if di is not None else self._next_packet_id()
        parts = [
            f'<DAT FN="{_esc_attr(self.rro_fn)}"',
            f'TN="{_esc_attr(self.tax_number)}"',
            f'ZN="{_esc_attr(self.factory_number)}"',
            f'DI="{packet_id}"',
            f'V="{_esc_attr(self.version)}"',
        ]
        if self.rro_type and self.rro_type != "0":
            parts.append(f'DT="{_esc_attr(self.rro_type)}"')

        return f'{" ".join(parts)}>{body_xml}<TS>{ts}</TS></DAT>'

    # ─── Чек продажу / повернення ───────────────────────────────────────────

    def build_receipt_xml(
        self,
        *,
        check_type: str = CHK_TYPE_SALE,
        items: list[dict[str, Any]],
        payments: list[dict[str, Any]],
        totals: dict[str, Any],
        date_time: datetime | None = None,
        discounts: list[dict[str, Any]] | None = None,
        comment: str | None = None,
        return_type: str | None = None,
    ) -> str:
        """
        Формує канонічний XML пакету даних чеку (<DAT><C>…</C><TS>…</TS></DAT>).

        Хеш-ланцюжок у тіло чеку НЕ вставляється (тег <H> відсутній) — контроль
        послідовності забезпечує <MAC> повідомлення (див. build_message).

        Args:
            check_type: тип чеку (T): "0" — продаж, "1" — повернення.
            items: список позицій чеку. Кожна позиція — словник:
                {"code", "barcode"(опц.), "name", "quantity", "price",
                 "total", "tax_rate"}
            payments: список оплат: {"code", "name"(опц.), "amount",
                "change"(опц.) — <M RM> додається лише якщо решта > 0}.
            totals: підсумкові суми чеку: {"total", "fiscal_number", "se",
                "tax_total", "tax_rate", "tax_percent", "dtpr", "dtsm",
                "tax_type", "tax_algorithm", "cashier"(опц.),
                "tax_groups"(опц.)}.
            date_time: дата/час операції (None — поточний ЛОКАЛЬНИЙ час).
            discounts: список знижок/націнок (<D>/<S>, опційно).
            comment: текстовий коментар чеку (<L>, опційно).
            return_type: тип виплати для чеку повернення (RT, опційно):
                "0" — повернення товару (за замовчуванням для T="1") тощо.

        Returns:
            str — канонічний XML тегу <DAT> (без <MAC>).
        """
        date_time = date_time or datetime.now()
        ts = _fmt_datetime(date_time)

        # ── Послідовність операцій у чеку ─────────────────────────────────
        seq = 0

        def next_n() -> int:
            nonlocal seq
            seq += 1
            return seq

        # ── Позиції продажу/повернення (<P>) ─────────────────────────────
        p_tags: list[str] = []
        for item in items:
            n = next_n()
            attrs: list[str] = [f'N="{n}"']
            if item.get("code") is not None:
                attrs.append(f'C="{_esc_attr(str(item["code"]))}"')
            if item.get("barcode"):
                attrs.append(f'CD="{_esc_attr(str(item["barcode"]))}"')
            attrs.append(f'NM="{_esc_attr(str(item["name"]))}"')
            attrs.append(f'SM="{_to_cents(item["total"])}"')
            attrs.append(f'Q="{_to_thousandths(item["quantity"])}"')
            attrs.append(f'PRC="{_to_cents(item["price"])}"')
            attrs.append(f'TX="{_esc_attr(str(item["tax_rate"]))}"')
            p_tags.append(f"<P {' '.join(attrs)}></P>")

        # ── Знижки/націнки (<D>/<S>) ─────────────────────────────────────
        d_tags: list[str] = []
        for disc in discounts or []:
            n = next_n()
            tag = "D" if disc.get("type", "D") == "D" else "S"
            attrs = [f'N="{n}"', f'TR="{disc.get("tr", "0")}"', f'TY="{disc.get("ty", "0")}"']
            if disc.get("percent") is not None:
                attrs.append(f'PR="{_format_percent(disc["percent"])}"')
            attrs.append(f'SM="{_to_cents(disc["total"])}"')
            if disc.get("ni") is not None:
                attrs.append(f'NI="{disc["ni"]}"')
            d_tags.append(f"<{tag} {' '.join(attrs)}></{tag}>")

        # ── Оплати (<M>) ──────────────────────────────────────────────────
        m_tags: list[str] = []
        for pay in payments:
            n = next_n()
            attrs = [f'N="{n}"', f'T="{_esc_attr(str(pay["code"]))}"']
            if pay.get("name"):
                attrs.append(f'NM="{_esc_attr(str(pay["name"]))}"')
            attrs.append(f'SM="{_to_cents(pay["amount"])}"')
            # RM: решта в гривнях — НЕ вказується, якщо решта відсутня
            change = pay.get("change")
            if change is not None and _as_decimal(change) > 0:
                attrs.append(f'RM="{_to_cents(change)}"')
            m_tags.append(f"<M {' '.join(attrs)}></M>")

        # ── Коментар (<L>) ────────────────────────────────────────────────
        l_tags: list[str] = []
        if comment:
            n = next_n()
            l_tags.append(f"<L N=\"{n}\">{_esc_text(comment)}</L>")

        # ── Закриття чеку (<E>) ───────────────────────────────────────────
        e_n = next_n()
        e_attrs: list[str] = [f'N="{e_n}"']
        if totals.get("fiscal_number") is not None:
            e_attrs.append(f'NO="{int(totals["fiscal_number"])}"')
        e_attrs.append(f'SM="{_to_cents(totals["total"])}"')
        if totals.get("se") is not None:
            e_attrs.append(f'SE="{_to_cents(totals["se"])}"')
        e_attrs.append(f'FN="{_esc_attr(self.rro_fn)}"')
        e_attrs.append(f'TS="{ts}"')

        tax_groups = totals.get("tax_groups")
        if tax_groups:
            # Декілька податкових груп → вкладені <TX> всередині <E>
            tx_tags = "".join(
                "<TX "
                + " ".join(
                    [
                        f'TX="{_esc_attr(str(g["tax"]))}"',
                        f'TXPR="{_format_percent(g.get("tax_percent", 0))}"',
                        f'TXSM="{_to_cents(g.get("tax_total", 0))}"',
                        f'DTPR="{_format_percent(g.get("dtpr", 0))}"',
                        f'DTSM="{_to_cents(g.get("dtsm", 0))}"',
                        f'TXTY="{_esc_attr(str(g.get("tax_type", "0")))}"',
                        f'TXAL="{_esc_attr(str(g.get("tax_algorithm", "0")))}"',
                    ]
                )
                + "></TX>"
                for g in tax_groups
            )
            e_tag = f"<E {' '.join(e_attrs)}>{tx_tags}</E>"
        else:
            # Одна податкова група → атрибути прямо на <E>
            e_attrs.append(f'TX="{_esc_attr(str(totals.get("tax_rate", "0")))}"')
            if totals.get("tax_percent") is not None:
                e_attrs.append(f'TXPR="{_format_percent(totals["tax_percent"])}"')
            if totals.get("tax_total") is not None:
                e_attrs.append(f'TXSM="{_to_cents(totals["tax_total"])}"')
            if totals.get("dtpr") is not None:
                e_attrs.append(f'DTPR="{_format_percent(totals["dtpr"])}"')
            if totals.get("dtsm") is not None:
                e_attrs.append(f'DTSM="{_to_cents(totals["dtsm"])}"')
            e_attrs.append(f'TXTY="{_esc_attr(str(totals.get("tax_type", "0")))}"')
            e_attrs.append(f'TXAL="{_esc_attr(str(totals.get("tax_algorithm", "0")))}"')
            if totals.get("cashier") is not None:
                e_attrs.append(f'CS="{int(totals["cashier"])}"')
            e_tag = f"<E {' '.join(e_attrs)}></E>"

        # RT — тип виплати; вказується тільки для чеку повернення (T="1")
        c_attrs = [f'T="{_esc_attr(check_type)}"']
        if check_type == CHK_TYPE_RETURN:
            c_attrs.append(f'RT="{_esc_attr(return_type or "0")}"')

        body = "".join(
            [
                f"<C {' '.join(c_attrs)}>",
                *p_tags,
                *d_tags,
                *m_tags,
                *l_tags,
                e_tag,
                "</C>",
            ]
        )

        dat_xml = self._build_dat(body, ts)
        return canonicalize(dat_xml)

    # ─── Z-звіт ─────────────────────────────────────────────────────────────

    def build_zreport_xml(
        self,
        *,
        shift_data: dict[str, Any],
        date_time: datetime | None = None,
    ) -> str:
        """
        Формує канонічний XML пакету даних Z-звіту (<DAT><Z>…</Z><TS>…</TS></DAT>).

        Args:
            shift_data: дані зміни (Z-звіту): shift_number, sales_count,
                returns_count, taxes[{tax,ts,tax_percent,tax_in,tax_out,dtpr,
                dti,dto,tax_type,tax_algorithm,smi,smo}], payments[{code,name,
                smi,smo}], cash_io[{code,name,smi,smo}], operations{qp,qs}.
            date_time: дата/час закриття зміни (None — поточний ЛОКАЛЬНИЙ час).

        Returns:
            str — канонічний XML тегу <DAT> (без <MAC>).
        """
        date_time = date_time or datetime.now()
        ts = _fmt_datetime(date_time)

        # ── Підсумки по податках (<TXS>) ──────────────────────────────────
        txs_tags: list[str] = []
        for tax in shift_data.get("taxes", []):
            attrs = [f'TX="{_esc_attr(str(tax["tax"]))}"']
            if tax.get("ts"):
                attrs.append(f'TS="{_esc_attr(str(tax["ts"]))}"')
            if tax.get("tax_percent") is not None:
                attrs.append(f'TXPR="{_format_percent(tax["tax_percent"])}"')
            if tax.get("tax_in") is not None:
                attrs.append(f'TXI="{_to_cents(tax["tax_in"])}"')
            if tax.get("tax_out") is not None:
                attrs.append(f'TXO="{_to_cents(tax["tax_out"])}"')
            if tax.get("dtpr") is not None:
                attrs.append(f'DTPR="{_format_percent(tax["dtpr"])}"')
            if tax.get("dti") is not None:
                attrs.append(f'DTI="{_to_cents(tax["dti"])}"')
            if tax.get("dto") is not None:
                attrs.append(f'DTO="{_to_cents(tax["dto"])}"')
            if tax.get("tax_type") is not None:
                attrs.append(f'TXTY="{_esc_attr(str(tax["tax_type"]))}"')
            if tax.get("tax_algorithm") is not None:
                attrs.append(f'TXAL="{_esc_attr(str(tax["tax_algorithm"]))}"')
            if tax.get("smi") is not None:
                attrs.append(f'SMI="{_to_cents(tax["smi"])}"')
            if tax.get("smo") is not None:
                attrs.append(f'SMO="{_to_cents(tax["smo"])}"')
            txs_tags.append(f"<TXS {' '.join(attrs)}></TXS>")

        # ── Обороти по формах оплати (<M>) ────────────────────────────────
        m_tags: list[str] = []
        for pay in shift_data.get("payments", []):
            attrs = [f'T="{_esc_attr(str(pay["code"]))}"']
            if pay.get("name"):
                attrs.append(f'NM="{_esc_attr(str(pay["name"]))}"')
            if pay.get("smi") is not None:
                attrs.append(f'SMI="{_to_cents(pay["smi"])}"')
            if pay.get("smo") is not None:
                attrs.append(f'SMO="{_to_cents(pay["smo"])}"')
            m_tags.append(f"<M {' '.join(attrs)}></M>")

        # ── Внесення/видачі (<IO>) ────────────────────────────────────────
        io_tags: list[str] = []
        for io in shift_data.get("cash_io", []):
            attrs = [f'T="{_esc_attr(str(io["code"]))}"']
            if io.get("name"):
                attrs.append(f'NM="{_esc_attr(str(io["name"]))}"')
            if io.get("smi") is not None:
                attrs.append(f'SMI="{_to_cents(io["smi"])}"')
            if io.get("smo") is not None:
                attrs.append(f'SMO="{_to_cents(io["smo"])}"')
            io_tags.append(f"<IO {' '.join(attrs)}></IO>")

        # ── Кількість чеків (<NC>) ────────────────────────────────────────
        nc_tag = (
            f'<NC NI="{int(shift_data.get("sales_count", 0))}" '
            f'NO="{int(shift_data.get("returns_count", 0))}"></NC>'
        )

        # ── Операції переказу (<OP>) ──────────────────────────────────────
        op_tags: list[str] = []
        operations = shift_data.get("operations")
        if operations:
            attrs = [f'QP="{int(operations.get("qp", 0))}"']
            if operations.get("qs") is not None:
                attrs.append(f'QS="{_to_cents(operations["qs"])}"')
            op_tags.append(f"<OP {' '.join(attrs)}></OP>")

        z_body = "".join([*txs_tags, *m_tags, *io_tags, nc_tag, *op_tags])
        z_xml = f'<Z NO="{int(shift_data["shift_number"])}">{z_body}</Z>'

        dat_xml = self._build_dat(z_xml, ts)
        return canonicalize(dat_xml)

    # ─── Службовий чек ──────────────────────────────────────────────────────

    def build_service_check_xml(
        self,
        *,
        service_type: str = SERVICE_PING,
        date_time: datetime | None = None,
        reserve_size: int = DEFAULT_RESERVE_SIZE,
    ) -> str:
        """
        Формує канонічний XML пакету даних службового чеку.

        Тіло — за зразками ДПС (262576.docx), БЕЗ <E N="1">:
          - 108/109/110/111: `<C T="..."></C>`;
          - 112: `<C T="112"><H SIZE="150"></H></C>` (SIZE = розмір діапазону).

        Args:
            service_type: тип службового чеку: 108 — відкриття зміни,
                109 — перехід в офлайн, 110 — перехід в онлайн,
                111 — перевірка зв'язку (ping), 112 — запит резервних номерів.
            date_time: дата/час операції (None — поточний ЛОКАЛЬНИЙ час).
            reserve_size: розмір діапазону резервних номерів для T=112.

        Returns:
            str — канонічний XML тегу <DAT> (без <MAC>).

        Raises:
            ValueError: якщо service_type не входить у допустимий набір.
        """
        if service_type not in SERVICE_TYPES:
            raise ValueError(
                f"Невідомий тип службового чеку: {service_type!r}. "
                f"Допустимі значення: {sorted(SERVICE_TYPES)}"
            )

        date_time = date_time or datetime.now()
        ts = _fmt_datetime(date_time)

        if service_type == SERVICE_RESERVE:
            body = (
                f'<C T="{service_type}"><H SIZE="{int(reserve_size)}"></H></C>'
            )
        else:
            body = f'<C T="{service_type}"></C>'
        dat_xml = self._build_dat(body, ts)
        return canonicalize(dat_xml)

    # ─── Повне повідомлення <RQ> ───────────────────────────────────────────

    def build_message(
        self,
        dat_xml: str,
        mac_value: str | None = None,
        *,
        mac_id: str = "",
        include_mac: bool = True,
    ) -> str:
        """
        Обгортає канонічний <DAT> у повне повідомлення <RQ>…</RQ>.

        Формат (ПРРО):
            <?xml version="1.0" encoding="windows-1251"?>
            <RQ V="1"><DAT …>…<TS>…</TS></DAT><MAC …>{hex}</MAC></RQ>

        MAC-значення = hex sha256 XML ПОПЕРЕДНЬОГО Check (передається в
        mac_value). None/"" — перший документ після скидання ланцюга.

        Args:
            dat_xml: канонічний XML тегу <DAT> (результат build_*_xml).
            mac_value: значення MAC (hex sha256 попереднього RQ); None → "".
            mac_id: ID тега <MAC>: "" для онлайн; резервний фіскальний номер
                для офлайн-документів (T=112/T=111 форму тега визначає білдер).
            include_mac: якщо False — тег <MAC> не додається (для сумісності).

        Returns:
            str — повне XML-повідомлення з XML-декларацією windows-1251.
        """
        dat_xml = canonicalize(dat_xml)  # гарантуємо канонічний вигляд
        if not include_mac:
            return f"{XML_DECLARATION}<RQ V=\"1\">{dat_xml}</RQ>"

        mac_value = "" if mac_value is None else mac_value
        return reconstruct_full_rq(dat_xml, mac_value, mac_id=mac_id)


# ─── Парсер підсумків чеку (для Z-звіту) ─────────────────────────────────────

def parse_receipt_xml_totals(dat_xml: str) -> dict:
    """
    Розбирає канонічний XML чеку (<DAT><C>…</C><TS>…</TS></DAT>) і повертає
    підсумкові дані для Z-звіту: тип чеку, суму, оплати та податки.

    Args:
        dat_xml: канонічний XML тегу <DAT> (результат build_receipt_xml).

    Returns:
        dict: {"check_type", "total", "payments", "taxes"} (див. код).

    Raises:
        ValueError: якщо XML некоректний або не містить тегу <C>.
    """
    if not dat_xml or not dat_xml.strip():
        raise ValueError("Порожній XML чеку")

    parser = etree.XMLParser(remove_blank_text=False, resolve_entities=False)
    try:
        root = etree.fromstring(dat_xml.encode("utf-8"), parser=parser)
    except etree.XMLSyntaxError as exc:
        raise ValueError(f"Некоректний XML чеку: {exc}") from exc

    c = root.find(".//C")
    if c is None:
        raise ValueError("У пакеті даних відсутній тег <C>")

    check_type = c.get("T", "0")
    total = Decimal("0")
    payments: dict[str, Decimal] = {}
    taxes: dict[str, dict] = {}

    # Обіг по податкових групах (сума позицій <P> по TX)
    turnover: dict[str, Decimal] = {}

    # Оплати (<M>)
    for m in c.findall("M"):
        code = m.get("T", "0")
        sm = Decimal(m.get("SM", "0")) / Decimal("100")
        payments[code] = payments.get(code, Decimal("0")) + sm

    # Позиції продажу/повернення (<P>) — обіг по податкових групах
    for p in c.findall("P"):
        tx = p.get("TX", "0")
        sm = Decimal(p.get("SM", "0")) / Decimal("100")
        turnover[tx] = turnover.get(tx, Decimal("0")) + sm

    # Закриття чеку (<E>) та податкові групи (<TX>)
    for e in c.findall("E"):
        total += Decimal(e.get("SM", "0")) / Decimal("100")
        tx_tags = e.findall("TX")
        if tx_tags:
            for tx in tx_tags:
                code = tx.get("TX", "0")
                taxes[code] = {
                    "percent": Decimal(tx.get("TXPR", "0")),
                    "tax_total": Decimal(tx.get("TXSM", "0")) / Decimal("100"),
                }
        else:
            # Одна група — атрибути безпосередньо на <E>
            code = e.get("TX", "0")
            taxes[code] = {
                "percent": Decimal(e.get("TXPR", "0")),
                "tax_total": Decimal(e.get("TXSM", "0")) / Decimal("100"),
            }

    # Додаємо обіг по кожній податковій групі (SMI для Z-звіту)
    for code, tax in taxes.items():
        tax["smi"] = turnover.get(code, Decimal("0"))

    return {
        "check_type": check_type,
        "total": total,
        "payments": payments,
        "taxes": taxes,
    }


__all__ = [
    "CHK_TYPE_RETURN",
    "CHK_TYPE_SALE",
    "CHK_TYPE_SERVICE",
    "DEFAULT_RESERVE_SIZE",
    "SERVICE_OFFLINE",
    "SERVICE_ONLINE",
    "SERVICE_OPEN_SHIFT",
    "SERVICE_PING",
    "SERVICE_RESERVE",
    "SERVICE_TYPES",
    "XML_DECLARATION",
    "XmlBuilder",
    "_to_cents",
    "_to_thousandths",
    "canonicalize",
    "chain_hash",
    "compute_mac",
    "cp1251_bytes",
    "parse_receipt_xml_totals",
    "reconstruct_full_rq",
    "signed_bytes_to_text",
]
