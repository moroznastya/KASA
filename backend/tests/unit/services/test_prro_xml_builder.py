"""Unit tests: XmlBuilder — побудова XML СЗЗД 2.1.7 (ПРРО)."""

from __future__ import annotations

import base64
from datetime import datetime
from decimal import Decimal

import pytest

from app.infrastructure.services.prro.xml_builder import (
    XmlBuilder,
    canonicalize,
    compute_mac,
    _to_cents,
    _to_thousandths,
    CHK_TYPE_RETURN,
    CHK_TYPE_SALE,
    SERVICE_OPEN_SHIFT,
    SERVICE_PING,
)

# ─── Фікстури ──────────────────────────────────────────────────────────────

TEST_FN = "3000628734"
TEST_TN = "38752983"
TEST_ZN = "3000989752"
TEST_DT = datetime(2026, 8, 1, 12, 0, 0)


@pytest.fixture
def builder() -> XmlBuilder:
    """Білдер з тестовими реквізитами ПРРО."""
    return XmlBuilder(
        rro_fn=TEST_FN,
        tax_number=TEST_TN,
        factory_number=TEST_ZN,
    )


@pytest.fixture
def receipt_items() -> list[dict]:
    """Тестові позиції чеку."""
    return [
        {
            "code": "1001",
            "barcode": "4820000000001",
            "name": "Кава",
            "quantity": Decimal("1"),
            "price": Decimal("25.00"),
            "total": Decimal("25.00"),
            "tax_rate": "1",
        },
        {
            "code": "1002",
            "name": "Булочка",
            "quantity": Decimal("2"),
            "price": Decimal("10.00"),
            "total": Decimal("20.00"),
            "tax_rate": "0",
        },
    ]


@pytest.fixture
def receipt_totals() -> dict:
    """Підсумкові суми чеку."""
    return {
        "total": Decimal("45.00"),
        "fiscal_number": 7,
        "tax_total": Decimal("4.17"),
        "tax_rate": "1",
        "tax_percent": Decimal("20.00"),
    }


# ─── _to_cents / _to_thousandths ───────────────────────────────────────────

class TestUnitConversion:
    """Перевірка перетворення сум та кількостей."""

    @pytest.mark.parametrize(
        "amount, expected",
        [
            (Decimal("12.34"), 1234),
            (Decimal("0.01"), 1),
            (Decimal("1"), 100),
            ("25.00", 2500),
            (25.5, 2550),
            (Decimal("0.005"), 1),   # ROUND_HALF_UP
            (Decimal("0.004"), 0),
        ],
    )
    def test_to_cents(self, amount, expected):
        """Сума грн × 100 → копійки."""
        assert _to_cents(amount) == expected

    @pytest.mark.parametrize(
        "quantity, expected",
        [
            (Decimal("1"), 1000),
            (Decimal("12.345"), 12345),
            (Decimal("0.5"), 500),
            (Decimal("1.2345"), 1235),  # ROUND_HALF_UP
            ("2", 2000),
        ],
    )
    def test_to_thousandths(self, quantity, expected):
        """Кількість × 1000 → тисячні частки."""
        assert _to_thousandths(quantity) == expected


# ─── canonicalize ──────────────────────────────────────────────────────────

class TestCanonicalize:
    """Канонічний вигляд XML (Додаток А СЗЗД 2.1.7)."""

    def test_attributes_in_alphabetical_order(self):
        """Атрибути всередині тегу розміщуються в алфавітному порядку."""
        xml = '<DAT ZN="AA" FN="123" DI="1" V="1"><C T="0"></C></DAT>'
        canonical = canonicalize(xml)
        # Порядок: DI, FN, V, ZN
        assert canonical == (
            '<DAT DI="1" FN="123" V="1" ZN="AA"><C T="0"></C></DAT>'
        )

    def test_no_whitespace_between_tags(self):
        """Пробіли/переноси між тегами видаляються."""
        xml = (
            '<DAT DI="1" FN="123" V="1" ZN="AA">\n'
            '  <C T="0">\n'
            '    <P N="1" NM="Хліб" SM="370" TX="1"/>\n'
            '  </C>\n'
            '  <TS>20110801112601</TS>\n'
            '</DAT>'
        )
        canonical = canonicalize(xml)
        assert "\n" not in canonical
        assert "  " not in canonical
        assert canonical == (
            '<DAT DI="1" FN="123" V="1" ZN="AA">'
            '<C T="0"><P N="1" NM="Хліб" SM="370" TX="1"></P></C>'
            '<TS>20110801112601</TS>'
            '</DAT>'
        )

    def test_self_closing_tags_expanded(self):
        """Самозакривні теги <tag/> → <tag></tag>."""
        xml = '<DAT DI="1" FN="1" V="1" ZN="1"><E N="1"/></DAT>'
        canonical = canonicalize(xml)
        assert "<E N=\"1\"></E>" in canonical
        assert "<E N=\"1\"/>" not in canonical

    def test_text_content_preserved(self):
        """Змістовний текстовий вміст зберігається."""
        xml = '<DAT DI="1" FN="1" V="1" ZN="1"><L N="1">Коментар   з  пробілами</L></DAT>'
        canonical = canonicalize(xml)
        assert "<L N=\"1\">Коментар   з  пробілами</L>" in canonical

    def test_invalid_xml_raises_value_error(self):
        """Некоректний XML → ValueError."""
        with pytest.raises(ValueError):
            canonicalize("<DAT><C></DAT>")

    def test_empty_xml_raises_value_error(self):
        """Порожній XML → ValueError."""
        with pytest.raises(ValueError):
            canonicalize("   ")


# ─── compute_mac ───────────────────────────────────────────────────────────

class TestComputeMac:
    """Обчислення MAC (SHA-256 + Base64)."""

    def test_deterministic(self):
        """Однаковий вхід → однаковий MAC."""
        dat = '<DAT DI="1" FN="123" V="1" ZN="AA"><C T="0"></C></DAT>'
        assert compute_mac(dat) == compute_mac(dat)

    def test_base64_encoded(self):
        """Результат — коректний Base64."""
        dat = '<DAT DI="1" FN="123" V="1" ZN="AA"></DAT>'
        mac = compute_mac(dat)
        # Можна декодувати як Base64
        decoded = base64.b64decode(mac)
        assert len(decoded) == 32  # SHA-256 = 32 байти

    def test_different_input_different_mac(self):
        """Різний вхід → різний MAC."""
        assert compute_mac('<DAT DI="1"></DAT>') != compute_mac('<DAT DI="2"></DAT>')


# ─── build_receipt_xml ─────────────────────────────────────────────────────

class TestBuildReceiptXml:
    """Побудова XML чеку продажу/повернення."""

    def test_structure(self, builder, receipt_items, receipt_totals):
        """Структура <DAT><C T="0"><P/><M/><E/></C><TS/>."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=receipt_items,
            payments=[{"code": "0", "name": "ГОТІВКА", "amount": Decimal("45.00")}],
            totals=receipt_totals,
            date_time=TEST_DT,
        )
        assert dat.startswith("<DAT ")
        assert dat.endswith("</DAT>")
        assert '<C T="0">' in dat
        assert "<P " in dat
        assert "<M " in dat
        assert "<E " in dat
        assert "<TS>20260801120000</TS>" in dat

    def test_amounts_multiplied_by_100(self, builder):
        """Суми вказуються в копійках (грн × 100)."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=[{
                "code": "1001", "name": "Кава",
                "quantity": Decimal("1"), "price": Decimal("25.00"),
                "total": Decimal("25.00"), "tax_rate": "1",
            }],
            payments=[{"code": "0", "amount": Decimal("50.00")}],
            totals={
                "total": Decimal("25.00"), "fiscal_number": 1,
                "tax_total": Decimal("4.17"), "tax_rate": "1",
                "tax_percent": Decimal("20.00"),
            },
            date_time=TEST_DT,
        )
        # 25.00 грн → 2500; 50.00 грн → 5000; ПДВ 4.17 грн → 417
        assert 'SM="2500"' in dat
        assert 'PRC="2500"' in dat
        assert 'SM="5000"' in dat
        assert 'TXSM="417"' in dat

    def test_quantity_multiplied_by_1000(self, builder):
        """Кількість вказується × 1000."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=[{
                "code": "1001", "name": "Вага",
                "quantity": Decimal("12.345"), "price": Decimal("10.00"),
                "total": Decimal("123.45"), "tax_rate": "0",
            }],
            payments=[{"code": "0", "amount": Decimal("123.45")}],
            totals={
                "total": Decimal("123.45"), "fiscal_number": 2,
                "tax_total": Decimal("0"), "tax_rate": "0",
            },
            date_time=TEST_DT,
        )
        assert 'Q="12345"' in dat

    def test_sequence_numbers(self, builder, receipt_items, receipt_totals):
        """Порядкові номери операцій N: 1, 2, ... — монотонні."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=receipt_items,  # 2 позиції
            payments=[{"code": "0", "amount": Decimal("45.00")}],
            totals=receipt_totals,
            date_time=TEST_DT,
        )
        # P: N=1, N=2; M: N=3; E: N=4 (канонічний вигляд: атрибути в алфавітному порядку)
        assert 'N="1"' in dat.split("<P ")[1].split(">")[0]
        assert 'N="2"' in dat.split("<P ")[2].split(">")[0]
        assert '<M N="3" ' in dat
        assert 'N="4"' in dat.split("<E ")[1].split(">")[0]

    def test_return_check_type(self, builder, receipt_items, receipt_totals):
        """Чек повернення має T="1"."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_RETURN,
            items=receipt_items,
            payments=[{"code": "0", "amount": Decimal("45.00")}],
            totals=receipt_totals,
            date_time=TEST_DT,
        )
        assert '<C RT="0" T="1">' in dat

    def test_fiscal_number_in_e(self, builder, receipt_items, receipt_totals):
        """Номер фіскального чеку NO присутній у <E>."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=receipt_items,
            payments=[{"code": "0", "amount": Decimal("45.00")}],
            totals={**receipt_totals, "fiscal_number": 7},
            date_time=TEST_DT,
        )
        assert 'NO="7"' in dat

    def test_multiple_tax_groups_nested_tx(self, builder):
        """Декілька податкових груп → вкладені <TX> у <E>."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=[{
                "code": "1", "name": "Товар",
                "quantity": Decimal("1"), "price": Decimal("100.00"),
                "total": Decimal("100.00"), "tax_rate": "1",
            }],
            payments=[{"code": "0", "amount": Decimal("100.00")}],
            totals={
                "total": Decimal("100.00"),
                "fiscal_number": 1,
                "tax_groups": [
                    {
                        "tax": "1", "tax_percent": Decimal("20.00"),
                        "tax_total": Decimal("16.67"),
                        "dtpr": Decimal("0.00"), "dtsm": Decimal("0"),
                        "tax_type": "0", "tax_algorithm": "0",
                    },
                    {
                        "tax": "0", "tax_percent": Decimal("0.00"),
                        "tax_total": Decimal("0"),
                        "dtpr": Decimal("0.00"), "dtsm": Decimal("0"),
                        "tax_type": "0", "tax_algorithm": "0",
                    },
                ],
            },
            date_time=TEST_DT,
        )
        assert "<TX " in dat
        assert 'TX="1"' in dat
        assert 'TX="0"' in dat
        # На самому <E> атрибута TXSM бути не повинно (групи вкладені)
        assert 'TXSM="' not in dat.split("<TX ")[0]

    def test_comment_tag(self, builder, receipt_items, receipt_totals):
        """Коментар <L> додається до чеку."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=receipt_items,
            payments=[{"code": "0", "amount": Decimal("45.00")}],
            totals=receipt_totals,
            comment="Повернення за чеком №100",
            date_time=TEST_DT,
        )
        assert "<L " in dat
        assert "Повернення за чеком №100" in dat


# ─── build_service_check_xml ───────────────────────────────────────────────

class TestBuildServiceCheckXml:
    """Побудова XML службових чеків."""

    def test_open_shift_service_type_108(self, builder):
        """Службовий чек відкриття зміни: T="108", E N="1"."""
        dat = builder.build_service_check_xml(
            service_type=SERVICE_OPEN_SHIFT,
            date_time=TEST_DT,
        )
        assert '<C T="108">' in dat
        assert '<E N="1"></E>' in dat

    def test_ping_service_type_111(self, builder):
        """Службовий чек перевірки зв'язку: T="111"."""
        dat = builder.build_service_check_xml(
            service_type=SERVICE_PING,
            date_time=TEST_DT,
        )
        assert '<C T="111">' in dat

    def test_invalid_service_type_raises(self, builder):
        """Невідомий тип службового чеку → ValueError."""
        with pytest.raises(ValueError):
            builder.build_service_check_xml(service_type="999")


# ─── build_zreport_xml ─────────────────────────────────────────────────────

class TestBuildZreportXml:
    """Побудова XML Z-звіту."""

    def test_zreport_structure(self, builder):
        """Структура <Z NO="..."><TXS/><M/><NC/></Z>."""
        dat = builder.build_zreport_xml(
            shift_data={
                "shift_number": 19,
                "sales_count": 18,
                "returns_count": 1,
                "taxes": [
                    {"tax": "0", "smi": Decimal("251.23")},
                    {
                        "tax": "1", "tax_percent": Decimal("20.00"),
                        "tax_in": Decimal("408.54"), "tax_out": Decimal("10.00"),
                        "smi": Decimal("2451.23"), "smo": Decimal("60.00"),
                    },
                ],
                "payments": [
                    {
                        "code": "0", "name": "ГОТІВКА",
                        "smi": Decimal("2451.23"), "smo": Decimal("60.00"),
                    },
                ],
            },
            date_time=TEST_DT,
        )
        assert '<Z NO="19">' in dat
        assert "<TXS " in dat
        assert "<M " in dat
        assert '<NC NI="18" NO="1">' in dat
        assert "<TS>20260801120000</TS>" in dat

    def test_zreport_amounts(self, builder):
        """Суми Z-звіту в копійках (×100)."""
        dat = builder.build_zreport_xml(
            shift_data={
                "shift_number": 1,
                "sales_count": 1,
                "returns_count": 0,
                "taxes": [{"tax": "1", "tax_percent": Decimal("20.00"),
                           "smi": Decimal("100.00"), "smo": Decimal("0")}],
                "payments": [{"code": "0", "name": "ГОТІВКА",
                              "smi": Decimal("100.00"), "smo": Decimal("0")}],
            },
            date_time=TEST_DT,
        )
        # 100.00 грн → 10000 копійок
        assert 'SMI="10000"' in dat

    def test_zreport_with_io_and_op(self, builder):
        """Опційні теги <IO> та <OP>."""
        dat = builder.build_zreport_xml(
            shift_data={
                "shift_number": 2,
                "sales_count": 3,
                "returns_count": 0,
                "taxes": [{"tax": "0", "smi": Decimal("50.00")}],
                "payments": [{"code": "0", "smi": Decimal("50.00")}],
                "cash_io": [
                    {"code": "0", "name": "ГОТІВКА",
                     "smi": Decimal("200.00"), "smo": Decimal("50.00")},
                ],
                "operations": {"qp": 2, "qs": Decimal("5.00")},
            },
            date_time=TEST_DT,
        )
        assert "<IO " in dat
        assert '<OP QP="2" QS="500">' in dat


# ─── build_message ─────────────────────────────────────────────────────────

class TestBuildMessage:
    """Обгортка повідомлення <RQ> з <MAC>."""

    def test_message_structure(self, builder, receipt_items, receipt_totals):
        """Повне повідомлення: <RQ><DAT/><MAC/></RQ>."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=receipt_items,
            payments=[{"code": "0", "amount": Decimal("45.00")}],
            totals=receipt_totals,
            date_time=TEST_DT,
        )
        msg = builder.build_message(dat)
        assert msg.startswith('<RQ V="1">')
        assert msg.endswith("</RQ>")
        assert "<DAT " in msg
        assert "<MAC " in msg
        assert "</MAC>" in msg

    def test_mac_di_matches_dat_di(self, builder, receipt_items, receipt_totals):
        """DI у <MAC> збігається з DI у <DAT>."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=receipt_items,
            payments=[{"code": "0", "amount": Decimal("45.00")}],
            totals=receipt_totals,
            date_time=TEST_DT,
        )
        msg = builder.build_message(dat)
        import re
        dat_di = re.search(r'<DAT\b[^>]*\bDI="(\d+)"', msg).group(1)
        mac_di = re.search(r'<MAC\b[^>]*\bDI="(\d+)"', msg).group(1)
        assert dat_di == mac_di

    def test_mac_is_computed_value(self, builder, receipt_items, receipt_totals):
        """MAC у повідомленні = compute_mac(dat)."""
        dat = builder.build_receipt_xml(
            check_type=CHK_TYPE_SALE,
            items=receipt_items,
            payments=[{"code": "0", "amount": Decimal("45.00")}],
            totals=receipt_totals,
            date_time=TEST_DT,
        )
        msg = builder.build_message(dat)
        expected = compute_mac(dat)
        assert f">{expected}</MAC>" in msg

    def test_include_mac_false(self, builder):
        """include_mac=False → повідомлення без <MAC>."""
        dat = builder.build_service_check_xml(
            service_type=SERVICE_PING,
            date_time=TEST_DT,
        )
        msg = builder.build_message(dat, include_mac=False)
        assert "<MAC" not in msg
        assert msg.startswith('<RQ V="1">')
        assert msg.endswith("</RQ>")

    def test_mac_number_monotonic(self, builder):
        """NT монотонно зростає (1, 2, 3...)."""
        for expected_nt in (1, 2, 3):
            dat = builder.build_service_check_xml(
                service_type=SERVICE_PING,
                date_time=TEST_DT,
            )
            msg = builder.build_message(dat)
            assert f'NT="{expected_nt}"' in msg


# ─── Лічильники ────────────────────────────────────────────────────────────

class TestCounters:
    """Монотонні лічильники DI/NT."""

    def test_packet_id_increments(self, builder):
        """Кожен новий <DAT> отримує новий DI."""
        d1 = builder.build_service_check_xml(service_type=SERVICE_PING, date_time=TEST_DT)
        d2 = builder.build_service_check_xml(service_type=SERVICE_PING, date_time=TEST_DT)
        import re
        di1 = int(re.search(r'DI="(\d+)"', d1).group(1))
        di2 = int(re.search(r'DI="(\d+)"', d2).group(1))
        assert di2 == di1 + 1

    def test_initial_packet_id(self):
        """Початковий лічильник DI задається через initial_packet_id."""
        b = XmlBuilder(
            rro_fn=TEST_FN, tax_number=TEST_TN, factory_number=TEST_ZN,
            initial_packet_id=100, initial_mac_number=50,
        )
        dat = b.build_service_check_xml(service_type=SERVICE_PING, date_time=TEST_DT)
        import re
        assert re.search(r'DI="(\d+)"', dat).group(1) == "101"
        msg = b.build_message(dat)
        assert 'NT="51"' in msg
