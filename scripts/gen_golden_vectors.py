#!/usr/bin/env python3
"""Генератор golden-векторів XML СЗЗД з Python-еталона (для Rust-тестів).

Відтворює /tmp/golden_vectors.json, на основі якого зафіксовано
crates/kasa-prro/tests/golden_xml.rs. Запуск з кореня kasa/:

    backend/venv/bin/python scripts/gen_golden_vectors.py > /tmp/golden_vectors.json

Вектори: чек продажу, повернення, знижка+коментар, податкові групи,
Z-звіт, службові 108/111, MAC, повне повідомлення, canonicalize.
"""
import json
import sys
from datetime import datetime

sys.path.insert(0, "backend")

from app.infrastructure.services.prro import xml_builder as xb  # noqa: E402

b = xb.XmlBuilder("4538765845", "345612052809", "АА57506761", initial_packet_id=0, initial_mac_number=0)
dt = datetime(2026, 8, 7, 11, 26, 1)

v1 = b.build_receipt_xml(
    check_type="0",
    items=[{"code": "120", "name": "Хліб", "quantity": "0.370", "price": "3.70",
            "total": "1.37", "tax_rate": "1", "barcode": "4820000000001"}],
    payments=[{"code": "0", "name": "ГОТІВКА", "amount": "5.00", "change": "3.63"}],
    totals={"fiscal_number": 3, "total": "1.37", "se": "1.14", "tax_total": "0.23",
            "tax_rate": "1", "tax_percent": "20.00", "dtpr": "0.00", "dtsm": "0",
            "tax_type": "0", "tax_algorithm": "0"},
    date_time=dt,
)
v2 = b.build_receipt_xml(
    check_type="1",
    items=[{"code": "250", "name": "Молоко", "quantity": "2.000", "price": "32.50",
            "total": "65.00", "tax_rate": "0"}],
    payments=[{"code": "0", "name": "ГОТІВКА", "amount": "65.00"}],
    totals={"fiscal_number": 4, "total": "65.00", "se": "65.00", "tax_total": "0",
            "tax_rate": "0", "tax_percent": "0.00", "dtpr": "0.00", "dtsm": "0",
            "tax_type": "0", "tax_algorithm": "0", "cashier": 2},
    return_type="0",
    date_time=dt,
)
v3 = b.build_receipt_xml(
    check_type="0",
    items=[{"code": "A", "name": "Кава & Чай", "quantity": "1.000", "price": "100.00",
            "total": "100.00", "tax_rate": "2"}],
    payments=[{"code": "1", "name": "КАРТКА", "amount": "100.00"}],
    totals={"fiscal_number": 5, "total": "100.00", "se": "83.33", "tax_total": "16.67",
            "tax_rate": "2", "tax_percent": "20.00", "dtpr": "0.00", "dtsm": "0",
            "tax_type": "0", "tax_algorithm": "0"},
    discounts=[{"type": "D", "tr": "0", "ty": "0", "total": "10.00", "ni": 1}],
    comment="Знижка за акцією",
    date_time=dt,
)
v4 = b.build_receipt_xml(
    check_type="0",
    items=[{"code": "1", "name": 'Товар "А"', "quantity": "1.500", "price": "10.00",
            "total": "15.00", "tax_rate": "0"}],
    payments=[{"code": "0", "name": "ГОТІВКА", "amount": "15.00"}],
    totals={"fiscal_number": 6, "total": "15.00", "se": "15.00", "tax_total": "0",
            "tax_rate": "0", "tax_percent": "0.00", "dtpr": "0.00", "dtsm": "0",
            "tax_type": "0", "tax_algorithm": "0",
            "tax_groups": [
                {"tax": "0", "tax_percent": "0.00", "tax_total": "0", "dtpr": "0.00",
                 "dtsm": "0", "tax_type": "0", "tax_algorithm": "0"},
                {"tax": "1", "tax_percent": "20.00", "tax_total": "2.50", "dtpr": "0.00",
                 "dtsm": "0", "tax_type": "0", "tax_algorithm": "0"},
            ]},
    date_time=dt,
)
v5 = b.build_zreport_xml(
    shift_data={
        "shift_number": 12, "sales_count": 150, "returns_count": 3,
        "taxes": [
            {"tax": "0", "ts": "20260807", "tax_percent": "0.00", "tax_in": "0",
             "tax_out": "0", "dtpr": "0.00", "dti": "0", "dto": "0", "tax_type": "0",
             "tax_algorithm": "0", "smi": "125000", "smo": "3000"},
            {"tax": "1", "ts": "20260807", "tax_percent": "20.00", "tax_in": "20833",
             "tax_out": "500", "dtpr": "0.00", "dti": "0", "dto": "0", "tax_type": "0",
             "tax_algorithm": "0", "smi": "104167", "smo": "2500"},
        ],
        "payments": [
            {"code": "0", "name": "ГОТІВКА", "smi": "120000", "smo": "2800"},
            {"code": "1", "name": "КАРТКА", "smi": "5000", "smo": "200"},
        ],
        "cash_io": [{"code": "0", "name": "ГОТІВКА", "smi": "1000", "smo": "0"}],
        "operations": {"qp": 5, "qs": "100.00"},
    },
    date_time=dt,
)
v6 = b.build_service_check_xml(service_type="108", date_time=dt)
v7 = b.build_service_check_xml(service_type="111", date_time=dt)
mac = xb.compute_mac(v1)
msg = b.build_message(v1, mac_value=mac)

vec = {
    "v1_receipt_sale": v1,
    "v2_receipt_return": v2,
    "v3_receipt_discount_comment": v3,
    "v4_receipt_tax_groups": v4,
    "v5_zreport": v5,
    "v6_service_open_shift": v6,
    "v7_service_ping": v7,
    "mac_of_v1": mac,
    "message_v1": msg,
    "canonical_check": xb.canonicalize('<C T="0">  <P N="1" C="120" NM="Хліб"/> </C>'),
    "to_cents_1.37": str(xb._to_cents("1.37")),
    "to_thousandths_0.370": str(xb._to_thousandths("0.370")),
}
print(json.dumps(vec, ensure_ascii=False, indent=1))
