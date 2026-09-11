//! GOLDEN PARITY: Rust == Python `xml_builder.py` — байт-ідентично
//! (спека prro_fix_spec_2026-09-08: RQ+MAC windows-1251, hex sha256
//! попереднього RQ, службові чеки за зразками 262576.docx).
//!
//! Вектори згенеровані з Python-еталона (2026-09-08, фіксовані дати/
//! лічильники) і зафіксовані тут як очікувані.

use torgashka_prro::xml::{
    canonicalize, compute_mac, to_cents, to_thousandths, Discount, DiscountKind, Payment,
    ReceiptItem, ShiftData, ShiftOperations, ShiftPayment, TaxGroup, Totals, XmlBuilder,
};

const FN: &str = "4538765845";
const TN: &str = "345612052809";
const ZN: &str = "АА57506761";
const TS: &str = "20260807112601"; // 2026-08-07 11:26:01

fn builder() -> XmlBuilder {
    XmlBuilder::new(FN, TN, ZN, "0", "1", 0, 0)
}

/// Builder з початковим DI (вектори знімались з послідовного Python-builder'а:
/// v1→DI=1, v2→DI=2, ... v7→DI=7).
fn builder_from(initial_packet_id: i64) -> XmlBuilder {
    XmlBuilder::new(FN, TN, ZN, "0", "1", initial_packet_id, 0)
}

#[test]
fn golden_v1_receipt_sale() {
    let mut b = builder();
    let out = b
        .build_receipt_xml(
            "0",
            &[ReceiptItem {
                code: Some("120".into()),
                barcode: Some("4820000000001".into()),
                name: "Хліб".into(),
                quantity: "0.370".into(),
                price: "3.70".into(),
                total: "1.37".into(),
                tax_rate: "1".into(),
            }],
            &[Payment {
                code: "0".into(),
                name: Some("ГОТІВКА".into()),
                amount: "5.00".into(),
                change: Some("3.63".into()),
            }],
            &Totals {
                fiscal_number: Some(3),
                total: "1.37".into(),
                se: Some("1.14".into()),
                tax_rate: "1".into(),
                tax_percent: Some("20.00".into()),
                tax_total: Some("0.23".into()),
                dtpr: Some("0.00".into()),
                dtsm: Some("0".into()),
                tax_type: Some("0".into()),
                tax_algorithm: Some("0".into()),
                ..Default::default()
            },
            TS,
            &[],
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        out,
        r#"<DAT DI="1" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C T="0"><P C="120" CD="4820000000001" N="1" NM="Хліб" PRC="370" Q="370" SM="137" TX="1"></P><M N="2" NM="ГОТІВКА" RM="363" SM="500" T="0"></M><E DTPR="0.00" DTSM="0" FN="4538765845" N="3" NO="3" SE="114" SM="137" TS="20260807112601" TX="1" TXAL="0" TXPR="20.00" TXSM="23" TXTY="0"></E></C><TS>20260807112601</TS></DAT>"#
    );
}

#[test]
fn golden_v2_receipt_return() {
    let mut b = builder_from(1);
    let out = b
        .build_receipt_xml(
            "1",
            &[ReceiptItem {
                code: Some("250".into()),
                name: "Молоко".into(),
                quantity: "2.000".into(),
                price: "32.50".into(),
                total: "65.00".into(),
                tax_rate: "0".into(),
                ..Default::default()
            }],
            &[Payment {
                code: "0".into(),
                name: Some("ГОТІВКА".into()),
                amount: "65.00".into(),
                change: None,
            }],
            &Totals {
                fiscal_number: Some(4),
                total: "65.00".into(),
                se: Some("65.00".into()),
                tax_rate: "0".into(),
                tax_percent: Some("0.00".into()),
                tax_total: Some("0".into()),
                dtpr: Some("0.00".into()),
                dtsm: Some("0".into()),
                tax_type: Some("0".into()),
                tax_algorithm: Some("0".into()),
                cashier: Some(2),
                ..Default::default()
            },
            TS,
            &[],
            None,
            Some("0"),
        )
        .unwrap();
    assert_eq!(
        out,
        r#"<DAT DI="2" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C RT="0" T="1"><P C="250" N="1" NM="Молоко" PRC="3250" Q="2000" SM="6500" TX="0"></P><M N="2" NM="ГОТІВКА" SM="6500" T="0"></M><E CS="2" DTPR="0.00" DTSM="0" FN="4538765845" N="3" NO="4" SE="6500" SM="6500" TS="20260807112601" TX="0" TXAL="0" TXPR="0.00" TXSM="0" TXTY="0"></E></C><TS>20260807112601</TS></DAT>"#
    );
}

#[test]
fn golden_v3_receipt_discount_comment() {
    let mut b = builder_from(2);
    let out = b
        .build_receipt_xml(
            "0",
            &[ReceiptItem {
                code: Some("A".into()),
                name: "Кава & Чай".into(),
                quantity: "1.000".into(),
                price: "100.00".into(),
                total: "100.00".into(),
                tax_rate: "2".into(),
                ..Default::default()
            }],
            &[Payment {
                code: "1".into(),
                name: Some("КАРТКА".into()),
                amount: "100.00".into(),
                change: None,
            }],
            &Totals {
                fiscal_number: Some(5),
                total: "100.00".into(),
                se: Some("83.33".into()),
                tax_rate: "2".into(),
                tax_percent: Some("20.00".into()),
                tax_total: Some("16.67".into()),
                dtpr: Some("0.00".into()),
                dtsm: Some("0".into()),
                tax_type: Some("0".into()),
                tax_algorithm: Some("0".into()),
                ..Default::default()
            },
            TS,
            &[Discount {
                kind: DiscountKind::Discount,
                tr: "0".into(),
                ty: "0".into(),
                percent: None,
                total: "10.00".into(),
                ni: Some(1),
            }],
            Some("Знижка за акцією"),
            None,
        )
        .unwrap();
    assert_eq!(
        out,
        r#"<DAT DI="3" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C T="0"><P C="A" N="1" NM="Кава &amp; Чай" PRC="10000" Q="1000" SM="10000" TX="2"></P><D N="2" NI="1" SM="1000" TR="0" TY="0"></D><M N="3" NM="КАРТКА" SM="10000" T="1"></M><L N="4">Знижка за акцією</L><E DTPR="0.00" DTSM="0" FN="4538765845" N="5" NO="5" SE="8333" SM="10000" TS="20260807112601" TX="2" TXAL="0" TXPR="20.00" TXSM="1667" TXTY="0"></E></C><TS>20260807112601</TS></DAT>"#
    );
}

#[test]
fn golden_v4_receipt_tax_groups() {
    let mut b = builder_from(3);
    let out = b
        .build_receipt_xml(
            "0",
            &[ReceiptItem {
                code: Some("1".into()),
                name: "Товар \"А\"".into(),
                quantity: "1.500".into(),
                price: "10.00".into(),
                total: "15.00".into(),
                tax_rate: "0".into(),
                ..Default::default()
            }],
            &[Payment {
                code: "0".into(),
                name: Some("ГОТІВКА".into()),
                amount: "15.00".into(),
                change: None,
            }],
            &Totals {
                fiscal_number: Some(6),
                total: "15.00".into(),
                se: Some("15.00".into()),
                tax_rate: "0".into(),
                tax_groups: vec![
                    TaxGroup {
                        tax: "0".into(),
                        percent: Some("0.00".into()),
                        total: Some("0".into()),
                        dtpr: Some("0.00".into()),
                        dtsm: Some("0".into()),
                        tax_type: Some("0".into()),
                        tax_algorithm: Some("0".into()),
                        ..Default::default()
                    },
                    TaxGroup {
                        tax: "1".into(),
                        percent: Some("20.00".into()),
                        total: Some("2.50".into()),
                        dtpr: Some("0.00".into()),
                        dtsm: Some("0".into()),
                        tax_type: Some("0".into()),
                        tax_algorithm: Some("0".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            TS,
            &[],
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        out,
        r#"<DAT DI="4" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C T="0"><P C="1" N="1" NM="Товар &quot;А&quot;" PRC="1000" Q="1500" SM="1500" TX="0"></P><M N="2" NM="ГОТІВКА" SM="1500" T="0"></M><E FN="4538765845" N="3" NO="6" SE="1500" SM="1500" TS="20260807112601"><TX DTPR="0.00" DTSM="0" TX="0" TXAL="0" TXPR="0.00" TXSM="0" TXTY="0"></TX><TX DTPR="0.00" DTSM="0" TX="1" TXAL="0" TXPR="20.00" TXSM="250" TXTY="0"></TX></E></C><TS>20260807112601</TS></DAT>"#
    );
}

#[test]
fn golden_v5_zreport() {
    let mut b = builder_from(4);
    let out = b
        .build_zreport_xml(
            &ShiftData {
                shift_number: 12,
                sales_count: 150,
                returns_count: 3,
                taxes: vec![
                    TaxGroup {
                        tax: "0".into(),
                        ts: Some("20260807".into()),
                        percent: Some("0.00".into()),
                        tax_in: Some("0".into()),
                        tax_out: Some("0".into()),
                        dtpr: Some("0.00".into()),
                        dti: Some("0".into()),
                        dto: Some("0".into()),
                        tax_type: Some("0".into()),
                        tax_algorithm: Some("0".into()),
                        smi: Some("125000".into()),
                        smo: Some("3000".into()),
                        ..Default::default()
                    },
                    TaxGroup {
                        tax: "1".into(),
                        ts: Some("20260807".into()),
                        percent: Some("20.00".into()),
                        tax_in: Some("20833".into()),
                        tax_out: Some("500".into()),
                        dtpr: Some("0.00".into()),
                        dti: Some("0".into()),
                        dto: Some("0".into()),
                        tax_type: Some("0".into()),
                        tax_algorithm: Some("0".into()),
                        smi: Some("104167".into()),
                        smo: Some("2500".into()),
                        ..Default::default()
                    },
                ],
                payments: vec![
                    ShiftPayment {
                        code: "0".into(),
                        name: Some("ГОТІВКА".into()),
                        smi: Some("120000".into()),
                        smo: Some("2800".into()),
                    },
                    ShiftPayment {
                        code: "1".into(),
                        name: Some("КАРТКА".into()),
                        smi: Some("5000".into()),
                        smo: Some("200".into()),
                    },
                ],
                cash_io: vec![ShiftPayment {
                    code: "0".into(),
                    name: Some("ГОТІВКА".into()),
                    smi: Some("1000".into()),
                    smo: Some("0".into()),
                }],
                operations: Some(ShiftOperations {
                    qp: 5,
                    qs: Some("100.00".into()),
                }),
            },
            TS,
        )
        .unwrap();
    assert_eq!(
        out,
        r#"<DAT DI="5" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><Z NO="12"><TXS DTI="0" DTO="0" DTPR="0.00" SMI="12500000" SMO="300000" TS="20260807" TX="0" TXAL="0" TXI="0" TXO="0" TXPR="0.00" TXTY="0"></TXS><TXS DTI="0" DTO="0" DTPR="0.00" SMI="10416700" SMO="250000" TS="20260807" TX="1" TXAL="0" TXI="2083300" TXO="50000" TXPR="20.00" TXTY="0"></TXS><M NM="ГОТІВКА" SMI="12000000" SMO="280000" T="0"></M><M NM="КАРТКА" SMI="500000" SMO="20000" T="1"></M><IO NM="ГОТІВКА" SMI="100000" SMO="0" T="0"></IO><NC NI="150" NO="3"></NC><OP QP="5" QS="10000"></OP></Z><TS>20260807112601</TS></DAT>"#
    );
}

#[test]
fn golden_v6_service_open_shift() {
    // Зразок ДПС: 108 → `<C T="108"></C>` (БЕЗ <E N="1">); MAC ID="" + ланцюг.
    let mut b = builder_from(5);
    let out = b.build_service_check_xml("108", TS, 150).unwrap();
    assert_eq!(
        out,
        r#"<DAT DI="6" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C T="108"></C><TS>20260807112601</TS></DAT>"#
    );
    let msg = b.build_message(&out, Some(""), "", true).unwrap();
    assert!(
        msg.starts_with("<?xml version=\"1.0\" encoding=\"windows-1251\"?>"),
        "{msg}"
    );
    assert!(msg.contains("<C T=\"108\"></C>"));
    assert!(msg.contains("<MAC ID=\"\"></MAC>"), "{msg}");
}

#[test]
fn golden_v7_service_ping() {
    // Зразок ДПС: 111 → `<C T="111"></C>` + `<MAC></MAC>` (без ID/значення).
    let mut b = builder_from(6);
    let out = b.build_service_check_xml("111", TS, 150).unwrap();
    assert_eq!(
        out,
        r#"<DAT DI="7" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C T="111"></C><TS>20260807112601</TS></DAT>"#
    );
    let msg = b.build_message(&out, Some("abc"), "", true).unwrap();
    assert!(msg.contains("<C T=\"111\"></C>"));
    assert!(msg.contains("<MAC></MAC>"), "ping: <MAC></MAC>: {msg}");
    assert!(!msg.contains("<MAC ID"), "{msg}");
}

#[test]
fn golden_message_v1_full_rq() {
    // Спека A: повне RQ з XML-декларацією windows-1251, MAC ID="" = hex
    // sha256 попереднього RQ (для першого документа — порожнє).
    let b = builder();
    let dat = r#"<DAT DI="1" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C T="0"><P C="120" CD="4820000000001" N="1" NM="Хліб" PRC="370" Q="370" SM="137" TX="1"></P><M N="2" NM="ГОТІВКА" RM="363" SM="500" T="0"></M><E DTPR="0.00" DTSM="0" FN="4538765845" N="3" NO="3" SE="114" SM="137" TS="20260807112601" TX="1" TXAL="0" TXPR="20.00" TXSM="23" TXTY="0"></E></C><TS>20260807112601</TS></DAT>"#;
    let msg = b.build_message(dat, None, "", true).unwrap();
    assert_eq!(
        msg,
        r#"<?xml version="1.0" encoding="windows-1251"?><RQ V="1"><DAT DI="1" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C T="0"><P C="120" CD="4820000000001" N="1" NM="Хліб" PRC="370" Q="370" SM="137" TX="1"></P><M N="2" NM="ГОТІВКА" RM="363" SM="500" T="0"></M><E DTPR="0.00" DTSM="0" FN="4538765845" N="3" NO="3" SE="114" SM="137" TS="20260807112601" TX="1" TXAL="0" TXPR="20.00" TXSM="23" TXTY="0"></E></C><TS>20260807112601</TS></DAT><MAC ID=""></MAC></RQ>"#
    );
    // MAC цього повного RQ — hex sha256 cp1251-байтів (Python-вектор).
    assert_eq!(
        compute_mac(&msg).unwrap(),
        "d94c997cf5331a5f3b9654780af2bc3860cabd1b9aed71dc3939338ded9168f8"
    );
}

#[test]
fn golden_hash_chain_message_python_parity() {
    // D: ланцюг контролюється <MAC>. Вектор з Python-еталона 2026-09-08
    // (date_time=2026-08-07 11:26:01, DI=2/3): c1 (DI=2) MAC порожній;
    // c2 (DI=3) MAC = hex sha256 повного RQ(c1).
    let mut b = builder_from(1);
    let items = [ReceiptItem {
        code: Some("120".into()),
        barcode: None,
        name: "Хліб".into(),
        quantity: "1".into(),
        price: "1.00".into(),
        total: "1.00".into(),
        tax_rate: "0".into(),
    }];
    let payments = [Payment {
        code: "0".into(),
        name: Some("ГОТІВКА".into()),
        amount: "1.00".into(),
        change: None,
    }];
    let totals = Totals {
        fiscal_number: Some(1),
        total: "1.00".into(),
        se: Some("1.00".into()),
        tax_rate: "0".into(),
        tax_percent: Some("20.00".into()),
        tax_total: Some("0.17".into()),
        dtpr: Some("0.00".into()),
        dtsm: Some("0".into()),
        tax_type: Some("0".into()),
        tax_algorithm: Some("0".into()),
        ..Default::default()
    };
    // c1 (DI=2): без <H> в тілі; MAC порожній
    let c1 = b
        .build_receipt_xml("0", &items, &payments, &totals, TS, &[], None, None)
        .unwrap();
    assert!(!c1.contains("<H "), "тіло чеку не містить <H>: {c1}");
    let m1 = b.build_message(&c1, None, "", true).unwrap();
    assert!(m1.contains("<MAC ID=\"\"></MAC>"), "{m1}");
    let mac1 = compute_mac(&m1).unwrap();
    assert_eq!(
        mac1,
        "7b427da3a6706ae9f30fce38ed92e3e6b7f9f934468aaf6fb2d1ae3a41fa2722"
    );

    // c2 (DI=3): MAC = hash повного RQ(c1)
    let c2 = b
        .build_receipt_xml("0", &items, &payments, &totals, TS, &[], None, None)
        .unwrap();
    assert!(!c2.contains("<H "), "тіло чеку не містить <H>: {c2}");
    let m2 = b.build_message(&c2, Some(&mac1), "", true).unwrap();
    assert!(
        m2.contains(&format!("<MAC ID=\"\">{mac1}</MAC>")),
        "MAC(c2)=hash RQ(c1): {m2}"
    );
    let mac2 = compute_mac(&m2).unwrap();
    assert_eq!(
        mac2,
        "854824e258225f63c414553340c5bb32145f194ad87a90c3cd2a89256b543ce0"
    );
    assert_ne!(mac1, mac2);
}

#[test]
fn golden_service_112_reserve() {
    // F/I: 112 → `<C T="112"><H SIZE="150"></H></C>`; <MAC> без ID.
    let mut b = builder_from(6);
    let out = b.build_service_check_xml("112", TS, 150).unwrap();
    assert_eq!(
        out,
        r#"<DAT DI="7" FN="4538765845" TN="345612052809" V="1" ZN="АА57506761"><C T="112"><H SIZE="150"></H></C><TS>20260807112601</TS></DAT>"#
    );
    let chain = "7b427da3a6706ae9f30fce38ed92e3e6b7f9f934468aaf6fb2d1ae3a41fa2722";
    let msg = b.build_message(&out, Some(chain), "", true).unwrap();
    assert!(msg.contains(&format!("<MAC>{chain}</MAC>")), "{msg}");
    assert!(!msg.contains("<MAC ID"), "{msg}");
}

#[test]
fn golden_canonical_check() {
    let out = canonicalize(r#"<C T="0">  <P N="1" C="120" NM="Хліб"/> </C>"#).unwrap();
    assert_eq!(out, r#"<C T="0"><P C="120" N="1" NM="Хліб"></P></C>"#);
}

#[test]
fn golden_to_cents() {
    assert_eq!(to_cents("1.37").unwrap(), 137);
}

#[test]
fn golden_to_thousandths() {
    assert_eq!(to_thousandths("0.370").unwrap(), 370);
}
