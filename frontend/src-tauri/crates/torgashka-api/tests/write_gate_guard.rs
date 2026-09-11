//! CI-guard відсутності обходу WriteGate (ADR-0007 §11.4, критерій E контракту).
//!
//! 1. **Статичний скан** `crates/*/src/**/*.rs` (усі 6 крейтів, §11.7): кожен
//!    DML-літерал (`INSERT INTO` / `UPDATE <таблиця>` / `DELETE FROM` /
//!    `CREATE ROLE` / `ALTER ROLE`) мусить мати або (а) виняток приймача
//!    (`sync.rs`, `sync_receivers.rs`), або (б) рядок у `POLICY_TABLE`
//!    (§11.1/§11.7) для таблиці чи для її БАТЬКА-документа (`satellite`,
//!    §11.7 п.2), або (в) належати шару локальної SQLite-копії
//!    (`is_local_sqlite_layer`: `offline/**`, `standby_heartbeat.rs`).
//!    Новий DML без класу → тест падає.
//! 2. **Негативний контроль**: синтетичний текст із `INSERT INTO
//!    non_policy_table` ДОВОДИТЬ, що скан реально ловить порушення
//!    (відтворювано в репо, без «зламай-і-відкоти»).
//! 3. **Повнота**: реєстр 41 write-точки §3 (згруповано за класами §3.5)
//!    звіряється з `POLICY_TABLE` автоматично; розбіжність → падіння.
//!
//! Тест не потребує PostgreSQL і не робить жодного I/O у мережу.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use axum::http::Method;
use torgashka_api::write_gate::{
    classify_request, is_local_sqlite_layer, policy_for, policy_for_dml_table, satellite_parent,
    WritePolicy, POLICY_TABLE, SATELLITE_TABLE,
};

// ─────────────────────────────────────────────────────────────────────────────
// Чиста функція скану (тестується на синтетичних джерелах — негативний контроль)
// ─────────────────────────────────────────────────────────────────────────────

/// Файли-приймачі: DML тут — частина приймача черги кас (§11.4 п.1 «виняток
/// приймача»); їхні сутності все одно перевіряються, якщо є в `POLICY_TABLE`.
const RECEIVER_FILES: &[&str] = &["sync.rs", "sync_receivers.rs"];

/// Сутність DDL-провіжну ролі реплікації (§3.2 #34–#35).
const DDL_ENTITY: &str = "replication_ddl_role";

/// Позначка DML із ДИНАМІЧНОЮ назвою таблиці (`DELETE FROM {table}`).
const DYNAMIC_SENTINEL: &str = "{dynamic}";

/// Динамічні DML-точки: `(файл:рядок, whitelist таблиць)` (§11.7 п.5).
///
/// Динамічна назва таблиці не класифікується за назвою, тому guard вимагає
/// ЯВНОЇ декларації точки: назва + повний whitelist значень, які ця точка може
/// підставити. Кожна таблиця whitelist'у мусить мати політику §11.1/§11.7 або
/// батька-`satellite` — інакше guard валить. Нова динамічна точка без рядка тут
/// → падіння `real_all_crates_tree_has_no_unclassified_dml`.
///
/// `documents.rs:1013 delete_document(id, document_type)` — generic-видалення
/// чернетки документа: `match document_type` (закритий перелік) → таблиця.
const DYNAMIC_TABLE_POINTS: &[(&str, &[&str])] = &[(
    "torgashka-infrastructure/src/repositories/documents.rs:1064",
    &[
        "invoices",
        "transfers",
        "write_offs",
        "return_invoices",
        "purchase_orders",
    ],
)];

/// Чи рядок є частиною DML-літерала (не коментар, не doc-коментар).
fn is_dml_line(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
        return false;
    }
    t.contains("INSERT INTO")
        || t.contains("DELETE FROM")
        || t.contains("CREATE ROLE")
        || t.contains("ALTER ROLE")
        || dml_update_entity(t).is_some()
}

/// Витягує `UPDATE <таблиця>` (з можливим префіксом схеми `public.`).
///
/// Враховує лише ПОЧАТОК statement'а (`"UPDATE ...`, `(UPDATE ...`, `;UPDATE`,
/// початок рядка) — інакше `ON CONFLICT ... DO UPDATE SET ...` дало б хибну
/// «сутність» `SET`.
fn dml_update_entity(line: &str) -> Option<String> {
    for (idx, _) in line.match_indices("UPDATE ") {
        let before = line[..idx].trim_end();
        let at_statement_start = before.is_empty()
            || before.ends_with('"')
            || before.ends_with('(')
            || before.ends_with(';');
        if !at_statement_start {
            continue;
        }
        let rest = &line[idx + "UPDATE ".len()..];
        let word: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        if word.is_empty() {
            continue;
        }
        let entity = strip_schema(&word);
        // Ключові слова SQL (SET/WHERE/VALUES) — не сутність.
        if entity.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            continue;
        }
        return Some(entity);
    }
    None
}

fn strip_schema(name: &str) -> String {
    name.rsplit('.').next().unwrap_or(name).to_string()
}

/// Витягує сутність (таблицю/DDL-механізм) з DML-рядка.
pub fn dml_entity(line: &str) -> Option<String> {
    let t = line.trim_start();
    if let Some(idx) = t.find("INSERT INTO ") {
        let rest = &t[idx + "INSERT INTO ".len()..];
        let word: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        if word.is_empty() && rest.trim_start().starts_with('{') {
            return Some(DYNAMIC_SENTINEL.to_string());
        }
        return Some(strip_schema(&word));
    }
    if let Some(idx) = t.find("DELETE FROM ") {
        let rest = &t[idx + "DELETE FROM ".len()..];
        let word: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        if word.is_empty() && rest.trim_start().starts_with('{') {
            // Динамічна назва таблиці (`DELETE FROM {table}`): класифікується
            // ТІЛЬКИ через явний whitelist у DYNAMIC_TABLE_POINTS (§11.7 п.5).
            return Some(DYNAMIC_SENTINEL.to_string());
        }
        return Some(strip_schema(&word));
    }
    if t.contains("CREATE ROLE") || t.contains("ALTER ROLE") {
        return Some(DDL_ENTITY.to_string());
    }
    dml_update_entity(t)
}

/// Скан джерел: `(шлях файлу, текст)` → список порушень.
///
/// Порушення = DML-рядок, чия ТАБЛИЦЯ не має ні власної політики
/// (`POLICY_TABLE` §11.1/§11.7), ні політики батька (`SATELLITE_TABLE`
/// §11.7 п.2), і чий файл не є ні файлом-приймачем (§11.4 п.1), ні файлом
/// шару локальної SQLite-копії (§11.7 п.4).
pub fn scan_dml_violations(sources: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, text) in sources {
        let base = Path::new(name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| name.clone());
        let receiver = RECEIVER_FILES.contains(&base.as_str());
        let sqlite_layer = is_local_sqlite_layer(name);
        for (i, line) in text.lines().enumerate() {
            if !is_dml_line(line) {
                continue;
            }
            let Some(entity) = dml_entity(line) else {
                continue;
            };
            // Винятки за шаром приймаються ПЕРШИМИ: DML у приймачі (§11.4 п.1) і в
            // шарі локальної SQLite (§11.7 п.4) не є записом у репліку, навіть
            // якщо назва таблиці динамічна (`offline/transactions.rs:212`).
            if receiver || sqlite_layer {
                continue;
            }
            if entity == DYNAMIC_SENTINEL {
                let loc = format!("{name}:{}", i + 1);
                match DYNAMIC_TABLE_POINTS
                    .iter()
                    .find(|(decl, _)| loc.ends_with(*decl))
                {
                    // Точка декларована: усі таблиці whitelist'у мусять мати клас.
                    Some((_, tables)) => {
                        for table in *tables {
                            if policy_for_dml_table(table).is_none() {
                                out.push(format!(
                                    "{loc}: динамічна таблиця — '{table}' з whitelist'у без політики (§11.1/§11.7)"
                                ));
                            }
                        }
                    }
                    None => out.push(format!(
                        "{loc}: `{}` — ДИНАМІЧНА назва таблиці без декларації в DYNAMIC_TABLE_POINTS (§11.7 п.5)",
                        line.trim()
                    )),
                }
                continue;
            }
            if policy_for_dml_table(&entity).is_some() || receiver || sqlite_layer {
                continue;
            }
            let hint = match satellite_parent(&entity) {
                Some(parent) => format!("батько '{parent}' відсутній у POLICY_TABLE"),
                None => "таблиця відсутня в POLICY_TABLE (§11.1/§11.7)".to_string(),
            };
            out.push(format!(
                "{}:{}: `{}` — таблиця '{entity}': {hint}",
                name,
                i + 1,
                line.trim()
            ));
        }
    }
    out
}

/// Крейти воркспейсу, які сканує guard (§11.7: «поверхня всього DML»).
const CRATES: &[&str] = &[
    "torgashka-api",
    "torgashka-application",
    "torgashka-domain",
    "torgashka-infrastructure",
    "torgashka-ocr",
    "torgashka-prro",
];

/// Скан `crates/*/src/**/*.rs` — уся PG-поверхня DML, не лише фасад (§11.7).
fn read_all_crates_src() -> Vec<(String, String)> {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf();
    let mut acc = Vec::new();
    for c in CRATES {
        let src = crates_dir.join(c).join("src");
        assert!(src.is_dir(), "немає теки {c}/src — онови список CRATES");
        read_rs_files(&src, &mut acc);
    }
    acc
}

fn read_rs_files(dir: &Path, acc: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path.is_dir() {
            read_rs_files(&path, acc);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                acc.push((path.to_string_lossy().to_string(), text));
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Негативний контроль (обовʼязковий): скан РЕАЛЬНО падає на новому DML
// ─────────────────────────────────────────────────────────────────────────────

fn synthetic(name: &str, sql: &str) -> (String, String) {
    (
        name.to_string(),
        format!("pub async fn h() {{\n    let q = \"{sql}\";\n}}\n"),
    )
}

#[test]
fn synthetic_dml_without_policy_is_a_violation() {
    // (i) новий DML без політики → НЕпорожній список порушень
    let bad = vec![synthetic(
        "src/new_feature.rs",
        "INSERT INTO non_policy_table (a) VALUES ($1)",
    )];
    let found = scan_dml_violations(&bad);
    assert_eq!(
        found.len(),
        1,
        "guard мусить підняти рівно одне порушення, маємо: {found:?}"
    );
    assert!(found[0].contains("non_policy_table"), "{}", found[0]);
    eprintln!("[guard] негативний контроль (INSERT без політики): {found:?}");

    // (ii) той самий DML із сутністю з таблиці політик → порожній список
    let ok = vec![synthetic(
        "src/admin.rs",
        "INSERT INTO stores (name) VALUES ($1)",
    )];
    assert!(
        scan_dml_violations(&ok).is_empty(),
        "INSERT INTO stores має політику (§11.1) → порушень бути не може"
    );

    // (iii) виняток приймача: файл sync_receivers.rs + сутність поза таблицею
    let receiver = vec![synthetic(
        "src/sync_receivers.rs",
        "INSERT INTO stock (store_id) VALUES ($1)",
    )];
    assert!(
        scan_dml_violations(&receiver).is_empty(),
        "приймачі (sync.rs/sync_receivers.rs) — дозволений виняток §11.4"
    );

    // (iv) коментарі та doc-коментарі DML не вважаються порушенням
    let comment = vec![(
        "src/doc.rs".to_string(),
        "/// Історично тут був INSERT INTO non_policy_table\n// UPDATE other\n".to_string(),
    )];
    assert!(scan_dml_violations(&comment).is_empty());
}

#[test]
fn synthetic_infrastructure_dml_is_scanned_and_satellite_inherits_policy() {
    // (i) файл поза фасадом (`torgashka-infrastructure`) з DML без класу →
    //     порушення: доводить, що crates-скан справді працює не лише в api.
    let infra = vec![synthetic(
        "torgashka-infrastructure/src/repositories/synthetic_new.rs",
        "INSERT INTO synthetic_unclassified_table (a) VALUES ($1)",
    )];
    let found = scan_dml_violations(&infra);
    assert_eq!(
        found.len(),
        1,
        "crates-скан мусить ловити infrastructure: {found:?}"
    );
    assert!(
        found[0].contains("synthetic_unclassified_table")
            && found[0].contains("torgashka-infrastructure"),
        "{}",
        found[0]
    );
    eprintln!("[guard] негативний контроль (infrastructure, INSERT без класу): {found:?}");

    // (ii) супутня таблиця успадковує політику батька → порушення немає
    for (child, parent) in SATELLITE_TABLE {
        assert_eq!(satellite_parent(child), Some(*parent));
        let sat = vec![synthetic(
            "torgashka-infrastructure/src/repositories/synthetic_child.rs",
            &format!("INSERT INTO {child} (document_id) VALUES ($1)"),
        )];
        assert!(
            scan_dml_violations(&sat).is_empty(),
            "satellite '{child}' мусить успадкувати політику батька '{parent}'"
        );
    }

    // (iii) `*_items`-сирота без батька → порушення (satellite не рятує)
    let orphan = vec![synthetic(
        "torgashka-infrastructure/src/repositories/synthetic_orphan.rs",
        "INSERT INTO synthetic_orphan_items (document_id) VALUES ($1)",
    )];
    assert_eq!(
        scan_dml_violations(&orphan).len(),
        1,
        "сирота `synthetic_orphan_items` без батька мусить падати"
    );

    // (iv) виняток шару локальної SQLite привʼязаний до ШЛЯХУ, не глобальний:
    //      той самий DML у `offline/**` — не порушення, у `repositories/**` — порушення
    let sql = "INSERT INTO synthetic_unclassified_table (a) VALUES ($1)";
    for ok_path in [
        "torgashka-infrastructure/src/offline/synthetic_local.rs",
        "torgashka-infrastructure/src/standby_heartbeat.rs",
    ] {
        assert!(
            scan_dml_violations(&[synthetic(ok_path, sql)]).is_empty(),
            "{ok_path}: DML шару локальної SQLite-копії не є записом у репліку"
        );
    }
    assert_eq!(
        scan_dml_violations(&[synthetic(
            "torgashka-infrastructure/src/repositories/synthetic_new.rs",
            sql
        )])
        .len(),
        1,
        "той самий DML поза шаром локальної SQLite мусить падати"
    );
    // (v) динамічна назва таблиці без декларації → порушення (§11.7 п.5)
    let dynamic = vec![synthetic(
        "torgashka-infrastructure/src/repositories/synthetic_dyn.rs",
        "let q = format!(\"DELETE FROM {table} WHERE id = $1\");",
    )];
    let found = scan_dml_violations(&dynamic);
    assert_eq!(found.len(), 1, "динамічний DML без декларації: {found:?}");
    assert!(found[0].contains("DYNAMIC_TABLE_POINTS"), "{}", found[0]);
    eprintln!("[guard] негативний контроль (динамічна таблиця без whitelist): {found:?}");

    assert!(is_local_sqlite_layer(
        "crates/torgashka-infrastructure/src/offline/db.rs"
    ));
    assert!(!is_local_sqlite_layer(
        "crates/torgashka-infrastructure/src/repositories/pos.rs"
    ));
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Реальне дерево `crates/torgashka-api/src`
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn real_src_tree_has_no_unclassified_dml() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    read_rs_files(&src, &mut sources);
    assert!(
        sources.len() > 30,
        "скан мусить бачити файли фасаду, знайдено {}",
        sources.len()
    );
    let violations = scan_dml_violations(&sources);
    let dml_count: usize = sources
        .iter()
        .map(|(_, t)| t.lines().filter(|l| is_dml_line(l)).count())
        .sum();
    eprintln!(
        "[guard] проскановано {} файлів, DML-рядків: {dml_count}, порушень: {}",
        sources.len(),
        violations.len()
    );
    assert!(
        violations.is_empty(),
        "DML без політики гейта (§11.1) — додай рядок у POLICY_TABLE або обґрунтуй виняток:\n{}",
        violations.join("\n")
    );
    assert!(
        dml_count >= 30,
        "очікували ≥30 DML-точок, маємо {dml_count}"
    );
}

#[test]
fn real_all_crates_tree_has_no_unclassified_dml() {
    let sources = read_all_crates_src();
    assert!(
        sources.len() > 100,
        "скан мусить бачити всі 6 крейтів (§11.7), знайдено {} файлів",
        sources.len()
    );
    let violations = scan_dml_violations(&sources);
    let (mut pg_dml, mut sqlite_dml) = (0usize, 0usize);
    let mut pg_tables: BTreeSet<String> = BTreeSet::new();
    let mut sqlite_tables: BTreeSet<String> = BTreeSet::new();
    for (name, text) in &sources {
        let sqlite = is_local_sqlite_layer(name);
        for line in text.lines() {
            if !is_dml_line(line) {
                continue;
            }
            let Some(entity) = dml_entity(line) else {
                continue;
            };
            if sqlite {
                sqlite_dml += 1;
                sqlite_tables.insert(entity);
            } else {
                pg_dml += 1;
                if entity != DYNAMIC_SENTINEL {
                    pg_tables.insert(entity);
                }
            }
        }
    }
    eprintln!(
        "[guard] §11.7: файлів={}, PG-таблиць={}, PG-DML={}, SQLite-таблиць={}, SQLite-DML={}, порушень={}",
        sources.len(),
        pg_tables.len(),
        pg_dml,
        sqlite_tables.len(),
        sqlite_dml,
        violations.len()
    );
    assert!(
        violations.is_empty(),
        "DML без класу (§11.1/§11.7) — додай рядок у POLICY_TABLE/SATELLITE_TABLE або обґрунтуй шар:
{}",
        violations.join("\n")
    );
    assert!(pg_dml >= 260, "PG-шар: очікували ≥260 DML-точок, маємо {pg_dml}");
    assert!(
        sqlite_dml >= 60,
        "шар локальної SQLite: очікували ≥60 DML-точок, маємо {sqlite_dml}"
    );
    let scanned: BTreeSet<&str> = pg_tables.iter().map(|s| s.as_str()).collect();
    let registered: BTreeSet<&str> = PG_TABLE_REGISTRY.iter().map(|(t, _)| *t).collect();
    assert_eq!(
        scanned, registered,
        "реєстр §11.7 ↔ скан розійшлися (лише в сканi: {:?}; лише в реєстрі: {:?})",
        scanned.difference(&registered).collect::<Vec<_>>(),
        registered.difference(&scanned).collect::<Vec<_>>()
    );
    for table in SQLITE_ONLY_TABLES {
        assert!(
            sqlite_tables.contains(*table),
            "таблиця SQLite-шару '{table}' зникла зі скану — онови §11.7"
        );
        assert!(
            !pg_tables.contains(*table),
            "таблиця '{table}' з'явилася в PG-шарі — потрібна політика §11.1/§11.7"
        );
    }
    for table in NO_DML_TABLES {
        assert!(
            !pg_tables.contains(*table) && !sqlite_tables.contains(*table),
            "таблиця '{table}' отримала DML-точку — онови §11.7"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Повнота: реєстр §3 (41 write-точка) ↔ POLICY_TABLE
// ─────────────────────────────────────────────────────────────────────────────

/// Клас write-точки за ADR-0007 §2.2/§3.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdrClass {
    UpstreamNow,
    DisabledOnStandby,
    Queue,
    LocalSqlite,
    /// §3.6: накладна `invoice` — клас `LocalOutbox` (гейт: `LocalOutbox`).
    LocalOutboxInvoice,
}

impl AdrClass {
    fn gate_policy(self) -> WritePolicy {
        match self {
            AdrClass::UpstreamNow => WritePolicy::ProxyToPrimary,
            AdrClass::DisabledOnStandby => WritePolicy::DisabledOnStandby,
            AdrClass::Queue | AdrClass::LocalSqlite | AdrClass::LocalOutboxInvoice => {
                WritePolicy::LocalOutbox
            }
        }
    }
}

/// Реєстр §3 ADR-0007 — 41 write-точка (`file:line`, сутність гейта, клас).
///
/// `line` = файл:рядок із §3 (там, де рядок змістився після рефакторингу, взято
/// фактичний — позначено в АНОМАЛІЯХ звіту).
const ADR_REGISTRY: &[(&str, &str, AdrClass)] = &[
    // §3.1 — адмін/мережа (UPSTREAM_NOW, 20)
    ("admin.rs:313", "stores", AdrClass::UpstreamNow),
    ("admin.rs:331", "user_stores", AdrClass::UpstreamNow),
    ("admin.rs:377", "stores", AdrClass::UpstreamNow),
    ("admin.rs:433", "stores", AdrClass::UpstreamNow),
    ("admin.rs:448", "devices", AdrClass::UpstreamNow),
    ("admin.rs:576", "stores", AdrClass::UpstreamNow),
    ("admin.rs:703", "user_stores", AdrClass::UpstreamNow),
    (
        "admin_migrate.rs:157",
        "migrate_legacy",
        AdrClass::UpstreamNow,
    ),
    (
        "admin_migrate.rs:231",
        "migrate_legacy",
        AdrClass::UpstreamNow,
    ),
    ("admin_prro.rs:348", "prro_settings", AdrClass::UpstreamNow),
    ("network.rs:274", "audit_log", AdrClass::UpstreamNow),
    ("network.rs:305", "network_events", AdrClass::UpstreamNow),
    ("network.rs:390", "devices", AdrClass::UpstreamNow),
    (
        "network.rs:459",
        "store_activation_codes",
        AdrClass::UpstreamNow,
    ),
    ("network.rs:594", "devices", AdrClass::UpstreamNow),
    ("network.rs:660", "devices", AdrClass::UpstreamNow),
    (
        "network_nodes.rs:306",
        "network_nodes",
        AdrClass::UpstreamNow,
    ),
    (
        "network_nodes.rs:477",
        "network_nodes",
        AdrClass::UpstreamNow,
    ),
    (
        "network_nodes.rs:702",
        "network_nodes",
        AdrClass::UpstreamNow,
    ),
    (
        "network_nodes.rs:887",
        "network_nodes",
        AdrClass::UpstreamNow,
    ),
    // §3.2 — приймачі/сервіс-job/DDL (DISABLED_ON_STANDBY, 16)
    (
        "lib.rs:1307",
        "network_nodes_offline_job",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync.rs:179",
        "store_sync_state",
        AdrClass::DisabledOnStandby,
    ),
    ("sync.rs:970", "sync_log", AdrClass::DisabledOnStandby),
    // §3.2 #24–#33: statements приймачів — їх несе одна HTTP-поверхня /sync/push
    (
        "sync_receivers.rs:192",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync_receivers.rs:219",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync_receivers.rs:269",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync_receivers.rs:293",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync_receivers.rs:338",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync_receivers.rs:364",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync_receivers.rs:437",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync_receivers.rs:459",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync_receivers.rs:514",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "sync_receivers.rs:537",
        "sync_push",
        AdrClass::DisabledOnStandby,
    ),
    (
        "network_nodes.rs:513",
        "replication_ddl_role",
        AdrClass::DisabledOnStandby,
    ),
    (
        "network_nodes.rs:521",
        "replication_ddl_role",
        AdrClass::DisabledOnStandby,
    ),
    // §3.2 #36 — promote: виконується ПІСЛЯ pg_promote (вузол уже primary).
    // HTTP-поверхні, що підлягає гейту, немає → перевіряється окремо нижче.
    (
        "promote.rs:172",
        NON_HTTP_MARKER,
        AdrClass::DisabledOnStandby,
    ),
    // §3.3 — операційні дані вузла (QUEUE, 1)
    ("store_context.rs:108", "device_heartbeat", AdrClass::Queue),
    // §3.4 — логін/логаут (LOCAL_SQLITE, 3)
    (
        "repositories/auth.rs:111",
        "work_session",
        AdrClass::LocalSqlite,
    ),
    (
        "repositories/auth.rs:126",
        "work_session",
        AdrClass::LocalSqlite,
    ),
    (
        "repositories/auth.rs:274",
        "work_session",
        AdrClass::LocalSqlite,
    ),
    // §3.6 — накладна (LocalOutbox, 1)
    (
        "repositories/invoices.rs:263",
        "invoice",
        AdrClass::LocalOutboxInvoice,
    ),
    // §11.6 — касова операція каси (Queue → LocalOutbox, 1)
    (
        "repositories/pos.rs:3603",
        "cash_operation",
        AdrClass::Queue,
    ),
    // §11.6 — довідник причин списання (UPSTREAM_NOW → ProxyToPrimary, 1)
    (
        "repositories/pos.rs:3200",
        "write_off_reason",
        AdrClass::UpstreamNow,
    ),
];

/// Позначка «механізм без HTTP-поверхні» (§3.2 #36: після `pg_promote`).
const NON_HTTP_MARKER: &str = "—";

/// Сутності POS-документів: канал SQLite (не літерали DML у `src/`) — доказ
/// §11.1 рядок 1 (`sync_push.rs:112-219`, `transactions.rs:153-204`).
const DOCUMENT_CHANNEL: &[&str] = &[
    "receipt",
    "return_receipt",
    "purchase_order",
    "inventory",
    "transfer",
    "write_off",
    "invoice",
];

#[test]
fn adr_registry_has_all_points_and_matching_policies() {
    assert_eq!(
        ADR_REGISTRY.len(),
        43,
        "реєстр §3/§11.6 мусить містити 43 write-точки (41 §3.5 + 2 §11.6)"
    );
    let mut counts = [0usize; 4];
    let mut exemptions = 0usize;
    for (loc, entity, class) in ADR_REGISTRY {
        // Клас рахуємо для ВСІХ 41 точки (§3.5).
        counts[match class {
            AdrClass::UpstreamNow => 0,
            AdrClass::DisabledOnStandby => 1,
            AdrClass::Queue | AdrClass::LocalSqlite => 2,
            AdrClass::LocalOutboxInvoice => 3,
        }] += 1;
        if *entity == NON_HTTP_MARKER {
            // Єдиний дозволений виняток: механізм без HTTP-поверхні.
            assert_eq!(loc, &"promote.rs:172", "невідомий виняток: {loc}");
            exemptions += 1;
            continue;
        }
        let expected = class.gate_policy();
        let policy = policy_for(entity).unwrap_or_else(|| {
            panic!("{loc}: сутність '{entity}' відсутня в POLICY_TABLE (§11.1)")
        });
        assert_eq!(
            policy, expected,
            "{loc}: політика '{entity}' ≠ клас ADR {class:?}"
        );
    }
    // §3.5: UPSTREAM_NOW 20, DISABLED 16, QUEUE+LOCAL_SQLITE 4, LocalOutbox(накладна) 1
    assert_eq!(counts[0], 21, "UPSTREAM_NOW (§3.1–§3.2 20 + write_off_reason §11.6)");
    assert_eq!(counts[1], 16, "DISABLED_ON_STANDBY (§3.5)");
    assert_eq!(counts[2], 5, "QUEUE 1 + LOCAL_SQLITE 3 (§3.5) + cash_operation §11.6");
    assert_eq!(counts[3], 1, "LocalOutbox-накладна (§3.6)");
    assert_eq!(exemptions, 1, "єдиний виняток — promote.rs:172 (§3.2 #36)");
    eprintln!(
        "[guard] реєстр §3/§11.6: 43 точки — UPSTREAM_NOW={}, DISABLED={}, QUEUE+LOCAL_SQLITE={}, invoice={}, винятків={}",
        counts[0], counts[1], counts[2], counts[3], exemptions
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Реєстр §11.7 — УСІ таблиці PG-шару зі скану `crates/*/src/**`
// ─────────────────────────────────────────────────────────────────────────────

/// Клас таблиці за реєстром §11.7 (той самий сенс, що `WritePolicy`, плюс
/// шар локальної SQLite, який політики PG не має).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableClass {
    LocalOutbox,
    ProxyToPrimary,
    DisabledOnStandby,
    /// Таблиця існує ЛИШЕ в шарі локальної SQLite-копії вузла (§11.7 п.4).
    LocalSqliteLayer,
}

impl TableClass {
    fn policy(self) -> Option<WritePolicy> {
        match self {
            TableClass::LocalOutbox => Some(WritePolicy::LocalOutbox),
            TableClass::ProxyToPrimary => Some(WritePolicy::ProxyToPrimary),
            TableClass::DisabledOnStandby => Some(WritePolicy::DisabledOnStandby),
            TableClass::LocalSqliteLayer => None,
        }
    }
}

/// Реєстр §11.7: 45 таблиць PG-шару (266 DML-точок). Таблиці-`satellite`
/// (`*_items`) стоять тут зі своїм класом, але власного рядка в `POLICY_TABLE`
/// НЕ мають — політику беруть від батька через `SATELLITE_TABLE`.
const PG_TABLE_REGISTRY: &[(&str, TableClass)] = &[
    // POS-документи каси (LocalOutbox, 9)
    ("receipts", TableClass::LocalOutbox),
    ("write_offs", TableClass::LocalOutbox),
    ("transfers", TableClass::LocalOutbox),
    ("invoices", TableClass::LocalOutbox),
    ("purchase_orders", TableClass::LocalOutbox),
    ("return_invoices", TableClass::LocalOutbox),
    ("inventories", TableClass::LocalOutbox),
    ("cash_operations", TableClass::LocalOutbox),
    ("prro_queue_items", TableClass::LocalOutbox),
    // Супутні таблиці документів (satellite, 7) — політика батька
    ("receipt_items", TableClass::LocalOutbox),
    ("write_off_items", TableClass::LocalOutbox),
    ("transfer_items", TableClass::LocalOutbox),
    ("invoice_items", TableClass::LocalOutbox),
    ("purchase_order_items", TableClass::LocalOutbox),
    ("return_invoice_items", TableClass::LocalOutbox),
    ("inventory_items", TableClass::LocalOutbox),
    // Спільні супутні дані документів (LocalOutbox, 2)
    ("stock", TableClass::LocalOutbox),
    ("supplier_ledger", TableClass::LocalOutbox),
    // Борги (§11.6.4 варіант 1 — LocalOutbox, 2)
    ("debtors", TableClass::LocalOutbox),
    ("debtor_payments", TableClass::LocalOutbox),
    // Сесії/логін вузла (LocalOutbox, 2)
    ("work_sessions", TableClass::LocalOutbox),
    ("users", TableClass::LocalOutbox),
    // Довідники/референси + адмін/мережа (ProxyToPrimary, 18)
    ("products", TableClass::ProxyToPrimary),
    ("categories", TableClass::ProxyToPrimary),
    ("product_images", TableClass::ProxyToPrimary),
    ("barcodes", TableClass::ProxyToPrimary),
    ("suppliers", TableClass::ProxyToPrimary),
    ("owners_db", TableClass::ProxyToPrimary),
    ("print_templates", TableClass::ProxyToPrimary),
    ("system_settings", TableClass::ProxyToPrimary),
    ("stores", TableClass::ProxyToPrimary),
    ("user_stores", TableClass::ProxyToPrimary),
    ("devices", TableClass::ProxyToPrimary),
    ("network_nodes", TableClass::ProxyToPrimary),
    ("prro_settings", TableClass::ProxyToPrimary),
    ("prro_shifts", TableClass::ProxyToPrimary),
    ("audit_log", TableClass::ProxyToPrimary),
    ("network_events", TableClass::ProxyToPrimary),
    ("store_activation_codes", TableClass::ProxyToPrimary),
    ("write_off_reasons", TableClass::ProxyToPrimary),
    // Агрегатор-only (DisabledOnStandby, 3)
    ("store_sync_state", TableClass::DisabledOnStandby),
    ("sync_log", TableClass::DisabledOnStandby),
    ("replication_ddl_role", TableClass::DisabledOnStandby),
    // Службові таблиці DDL-міток (Фаза 2.2 — ізоляція тестів, DisabledOnStandby, 2)
    ("schema_revision", TableClass::DisabledOnStandby),
    ("ddl_markers", TableClass::DisabledOnStandby),
];

/// Таблиці, що існують ЛИШЕ в шарі локальної SQLite-копії (69 DML-точок,
/// `offline/**` + `standby_heartbeat.rs`): політики PG не мають (§11.7 п.4).
const SQLITE_ONLY_TABLES: &[&str] = &[
    "cash_balance",
    "employees",
    "outbox",
    "products_v2",
    "settings",
    "stock_norms",
    "sync_meta",
    "t_user",
];

/// Таблиці з ADR §11.7 без жодної DML-точки в скан (лише читання/DDL) —
/// рядок реєстру існує, щоб зміна цього факту впала як порушення.
const NO_DML_TABLES: &[&str] = &["price_tags"];

#[test]
fn pg_table_registry_is_complete_and_consistent() {
    assert_eq!(
        PG_TABLE_REGISTRY.len(),
        45,
        "§11.7: 45 таблиць PG-шару зі скану crates/*/src/**"
    );
    let mut names: BTreeSet<&str> = BTreeSet::new();
    let (mut lo, mut px, mut ds) = (0usize, 0usize, 0usize);
    for (table, class) in PG_TABLE_REGISTRY {
        assert!(names.insert(table), "дубль таблиці '{table}' у реєстрі §11.7");
        let policy = policy_for_dml_table(table)
            .unwrap_or_else(|| panic!("§11.7: таблиця '{table}' без політики (§11.1/§11.7)"));
        assert_eq!(
            Some(policy),
            class.policy(),
            "'{table}': політика {policy:?} ≠ клас реєстру {class:?}"
        );
        match class {
            TableClass::LocalOutbox => lo += 1,
            TableClass::ProxyToPrimary => px += 1,
            TableClass::DisabledOnStandby => ds += 1,
            TableClass::LocalSqliteLayer => unreachable!("шар SQLite не в реєстрі PG"),
        }
    }
    assert_eq!(lo, 22, "§11.7: LocalOutbox — 22 таблиці");
    assert_eq!(px, 18, "§11.7: ProxyToPrimary — 18 таблиць");
    assert_eq!(ds, 5, "§11.7: DisabledOnStandby — 5 таблиць");

    // satellite: власного рядка немає, політика = політика батька
    assert_eq!(SATELLITE_TABLE.len(), 7, "§11.7 п.2: 7 супутніх таблиць");
    for (child, parent) in SATELLITE_TABLE {
        assert_eq!(
            satellite_parent(child),
            Some(*parent),
            "мапінг '{child}' → '{parent}'"
        );
        assert!(
            policy_for(child).is_none(),
            "satellite '{child}' мусить успадковувати політику '{parent}', а не мати власний рядок"
        );
        assert_eq!(
            policy_for_dml_table(child),
            policy_for(parent),
            "політика satellite '{child}' ≠ батько '{parent}'"
        );
    }

    // шар локальної SQLite: політики PG немає
    assert_eq!(SQLITE_ONLY_TABLES.len(), 8, "§11.7 п.4: 8 таблиць лише в SQLite");
    for table in SQLITE_ONLY_TABLES {
        assert!(
            policy_for(table).is_none(),
            "таблиця SQLite-шару '{table}' не мусить мати політику PG"
        );
    }
    assert_eq!(
        TableClass::LocalSqliteLayer.policy(),
        None,
        "шар локальної SQLite не має політики PG (§11.7 п.4)"
    );

    // POLICY_TABLE = 25 рядків гейт-сутностей (§11.1 + §11.6) + 28 таблиць
    // PG-шару (§11.7) + 4 поверхневих сутності (§11.7.9, Фази 3.2/3.8).
    assert_eq!(
        POLICY_TABLE.len(),
        57,
        "POLICY_TABLE: 25 гейт-сутностей + 28 таблиць PG-шару + 4 поверхневих §11.7.9"
    );
    assert_eq!(
        PG_TABLE_REGISTRY.len() + SQLITE_ONLY_TABLES.len() + NO_DML_TABLES.len(),
        54,
        "§11.7: 45 таблиць PG + 8 лише-SQLite + 1 без DML = 54 рядки реєстру"
    );
    eprintln!(
        "[guard] §11.7: таблиць=54 (PG={lo}, лише-SQLite={}, без-DML={}; класи PG: LocalOutbox={lo}, ProxyToPrimary={px}, Disabled={ds}), satellites={}, POLICY_TABLE={}",
        SQLITE_ONLY_TABLES.len(),
        NO_DML_TABLES.len(),
        SATELLITE_TABLE.len(),
        POLICY_TABLE.len()
    );
}

#[test]
fn every_policy_entity_is_covered_by_adr_registry_or_document_channel() {
    let mut covered: BTreeSet<&str> = BTreeSet::new();
    for (_, entity, _) in ADR_REGISTRY {
        covered.insert(entity);
    }
    for e in DOCUMENT_CHANNEL {
        covered.insert(e);
    }
    for e in SURFACE_ENTITIES {
        covered.insert(e);
    }
    for (table, _) in PG_TABLE_REGISTRY {
        covered.insert(table);
    }
    for (child, _) in SATELLITE_TABLE {
        covered.insert(child);
    }
    for table in SQLITE_ONLY_TABLES {
        covered.insert(table);
    }
    for table in NO_DML_TABLES {
        covered.insert(table);
    }
    let missing: Vec<&str> = POLICY_TABLE
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !covered.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "сутності POLICY_TABLE без рядка в §3/§11.1/§11.7: {missing:?}"
    );
    assert_eq!(
        POLICY_TABLE.len(),
        57,
        "таблиця політик = 23 рядки §11.1 + 2 §11.6 + 28 таблиць PG-шару §11.7 + 4 поверхневих §11.7.9"
    );
    eprintln!(
        "[guard] POLICY_TABLE: {} рядків, усі покриті реєстром §3/§11.6/§11.7 або каналом документів",
        POLICY_TABLE.len()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// §11.7.9 (Фаза 3.2): аудит «HTTP-поверхня → клас» — машина, не ручний перелік
// ─────────────────────────────────────────────────────────────────────────────

/// Файли фасаду, де маршрути реєструються літерально (`Router::new().route(`).
/// Поверхневі сутності §11.7.9 (Фаза 3.2): не таблиці, а HTTP-поверхні, чиї
/// хендлери пишуть PG через mode-agnostic сервіс, а таблиці-цілі належать
/// різним класам. Мусять мати рядок у `POLICY_TABLE` (інакше `admin_pool`
/// поверне локальну РЕПЛІКУ на standby — F5).
const SURFACE_ENTITIES: &[&str] = &[
    "catalog_directories",
    "documents_batch",
    "setup",
    // ФАЗА 3.8: drain черги вузла у власний PG — поверхня, що пише агрегати
    // різних класів (receipts/invoices/return_invoices/inventories/...)
    // через ядро `sync::process_push_item`.
    "outbox_drain",
];

const ROUTE_FILES: &[&str] = &[
    "crates/torgashka-api/src/router_v1.rs",
    "crates/torgashka-api/src/return_invoices.rs",
    "crates/torgashka-api/src/route_local.rs",
    "crates/torgashka-api/src/promote.rs",
];

/// Поверхні, які СВІДОМО лишаються `Pass` на standby: вони не роблять жодного
/// запису в PostgreSQL (пишуть локальний файл/пристрій цього вузла або лише
/// читають). Формат `("МЕТОД /шлях", "причина")`; причина обов'язкова — новий
/// `None` без пояснення тест не пропускає.
const CONSCIOUS_PASS: &[(&str, &str)] = &[
    (
        "POST /api/v1/local/promote",
        "DR: підвищення ЦЬОГО вузла (pg_promote) + запис реєстру вже в стані primary (§3 #36); \
         Фаза 3.8 — напр. drain залишку SQLite-черги у власний PG (поверхня `outbox_drain`, клас LocalOutbox)",
    ),
    (
        "POST /api/v1/local/repoint-primary",
        "DR: локальний конфіг вузла (файли), жодного DML",
    ),
    (
        "POST /api/v1/admin/db-sources",
        "локальний `db_sources.toml` цього вузла (файл), не PG — `classify_request` повертає None свідомо",
    ),
    ("PUT /api/v1/admin/db-sources/:id", "локальний `db_sources.toml` (файл)"),
    ("DELETE /api/v1/admin/db-sources/:id", "локальний `db_sources.toml` (файл)"),
    ("POST /api/v1/admin/db-sources/:id/test", "перевірка з’єднання (файл+мережа), не PG"),
    ("POST /api/v1/admin/db-sources/:id/activate", "локальний `db_sources.toml` (файл)"),
    ("POST /api/v1/admin/db-sources/provision", "provisioning локальної БД (pg_dump/pg_restore), не PG-таблиця"),
    ("POST /api/v1/admin/db-sources/export-dump", "pg_dump у файл, DML немає"),
    ("POST /api/v1/admin/db-sources/import-dump", "pg_restore з файлу, DML у реєстрі §11.7 немає"),
    ("POST /api/v1/ocr/invoice", "OCR-аналіз: читання товарів + відповідь, DML немає"),
    ("POST /api/v1/invoice-ocr/analyze", "OCR-аналіз: читання товарів + відповідь, DML немає"),
    ("POST /api/v1/print/test", "пробний друк на пристрій вузла, DML немає"),
    ("POST /api/v1/print/price-tags/render", "рендер цінників: читання + файл, DML немає"),
    ("POST /api/v1/print/labels/render", "рендер етикеток: читання + файл, DML немає"),
    ("POST /api/v1/print-templates/:template_id/render", "рендер шаблону: читання + файл, DML немає"),
    (
        "POST /api/v1/admin/network-config/export",
        "експорт реєстру мережі у ФАЙЛ: лише читання таблиць, DML немає",
    ),
    ("POST /api/v2/prro/test-connection", "перевірка зв’язку з ПРРО, DML немає"),
];

/// Результат одного маршруту: ключ `МЕТОД /шлях` → рядок класу (для звіту §11.7.9).
fn audit_route(method: &str, path: &str) -> Option<String> {
    let concrete = concrete_path(path);
    let m = Method::from_bytes(method.as_bytes()).ok()?;
    classify_request(&m, &concrete).map(|e| e.to_string())
}

/// Динамічні сегменти (`:id`, `:barcode`) → конкретні значення, бо
/// `classify_request` працює з реальним шляхом.
fn concrete_path(path: &str) -> String {
    const UUID_SAMPLE: &str = "11111111-2222-3333-4444-555555555555";
    path.split('/')
        .map(|seg| {
            if !seg.starts_with(':') {
                return seg.to_string();
            }
            match seg.trim_start_matches(':') {
                "barcode" => "4820000000001".to_string(),
                _ => UUID_SAMPLE.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Парсер `.route("...", <chain>)`: повертає `(метод, шлях)` для всіх
/// write-методів у ланцюжку (`post(..).put(..)` тощо).
fn parse_write_routes(src: &str) -> Vec<(String, String)> {
    let ch: Vec<char> = src.chars().collect();
    let needle: Vec<char> = ".route(".chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + needle.len() <= ch.len() {
        if ch[i..i + needle.len()] != needle[..] {
            i += 1;
            continue;
        }
        let mut j = i + needle.len();
        while j < ch.len() && ch[j].is_whitespace() {
            j += 1;
        }
        if j >= ch.len() || ch[j] != '"' {
            i += needle.len();
            continue;
        }
        let start = j + 1;
        let mut end = start;
        while end < ch.len() && ch[end] != '"' {
            end += 1;
        }
        let path: String = ch[start..end].iter().collect();
        // Баланс дужок від позиції `.route(` — кінець ланцюжка.
        let mut depth = 0usize;
        let mut k = i;
        let mut chain_end = ch.len();
        while k < ch.len() {
            match ch[k] {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        chain_end = k;
                        break;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        let chain: String = ch[i + needle.len()..chain_end].iter().collect();
        for (m, tok) in [
            ("POST", "post("),
            ("PUT", "put("),
            ("PATCH", "patch("),
            ("DELETE", "delete("),
        ] {
            let mut from = 0usize;
            while let Some(rel) = chain[from..].find(tok) {
                let idx = from + rel;
                let boundary = chain[..idx]
                    .chars()
                    .next_back()
                    .map(|c| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(true);
                if boundary {
                    out.push((m.to_string(), path.clone()));
                }
                from = idx + tok.len();
            }
        }
        i += needle.len();
    }
    out
}

/// Кожна write-поверхня фасаду мусить мати ЯВНИЙ клас (§11.7.9, Фаза 3.2):
/// або рядок у `classify_request`, або обґрунтований запис у `CONSCIOUS_PASS`.
/// Інакше на standby такий маршрут отримує `None` → `Pass` → DML у
/// read-only репліку → сирий 500 (дефект `cash_operations` до §11.6).
#[test]
fn every_write_route_has_explicit_class() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut bad: Vec<String> = Vec::new();

    for rel in ROUTE_FILES {
        let src = std::fs::read_to_string(root.join(rel))
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        for (method, path) in parse_write_routes(&src) {
            if !seen.insert((method.clone(), path.clone())) {
                continue;
            }
            let key = format!("{method} {path}");
            let entity = audit_route(&method, &path);
            let class = match entity {
                Some(e) => match policy_for(&e) {
                    Some(p) => format!("{e} -> {p:?}"),
                    None => {
                        bad.push(format!("{key}: сутність `{e}` без рядка в POLICY_TABLE"));
                        continue;
                    }
                },
                None => {
                    if CONSCIOUS_PASS.iter().any(|(k, _)| *k == key) {
                        "СВІДОМО Pass (немає DML у PG; причина в CONSCIOUS_PASS)".to_string()
                    } else {
                        bad.push(format!("{key}: classify_request -> None"));
                        continue;
                    }
                }
            };
            rows.push((key, class));
        }
    }

    for (key, class) in &rows {
        eprintln!("[routes] {key} | {class}");
    }
    eprintln!(
        "[routes] усього write-поверхонь={} | класифіковано={} | свідомо Pass={}",
        rows.len(),
        rows.iter().filter(|(_, c)| !c.starts_with("СВІДОМО")).count(),
        rows.iter().filter(|(_, c)| c.starts_with("СВІДОМО")).count()
    );

    assert!(
        bad.is_empty(),
        "write-поверхні без явного класу ({}):\n  {}",
        bad.len(),
        bad.join("\n  ")
    );

    // Застарілий запис у CONSCIOUS_PASS (маршрут зник/перекласифікований) → падіння.
    let stale: Vec<&str> = CONSCIOUS_PASS
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !seen.iter().any(|(m, p)| format!("{m} {p}") == *k))
        .collect();
    assert!(stale.is_empty(), "застарілі CONSCIOUS_PASS: {stale:?}");
}

/// Друга лінія захисту `admin_pool(state, "X")` мусить мати рядок у
/// `POLICY_TABLE`: інакше `decide(Standby, Some("X"))` → `None` → `Pass` →
/// хендлер отримує локальну **репліку** замість відмови (F5). Знайдено аудитом
/// Фази 3.2: `crud.rs:165` передавав `catalog_directories`, якого в таблиці не
/// було (рядок додано — §11.7.9).
#[test]
fn every_admin_pool_entity_has_policy() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    read_rs_files(&src, &mut sources);
    let mut found = 0usize;
    let mut missing: Vec<String> = Vec::new();
    for (path, text) in &sources {
        for (i, line) in text.lines().enumerate() {
            if line.contains("fn admin_pool") || line.trim_start().starts_with("//") {
                continue;
            }
            let Some(idx) = line.find("admin_pool(") else {
                continue;
            };
            let tail = &line[idx..];
            let Some(q1) = tail.find('"') else {
                continue;
            };
            let rest = &tail[q1 + 1..];
            let Some(q2) = rest.find('"') else {
                continue;
            };
            let entity = &rest[..q2];
            found += 1;
            if policy_for(entity).is_none() {
                missing.push(format!(
                    "{path}:{}: admin_pool(\"{entity}\") — немає рядка в POLICY_TABLE",
                    i + 1
                ));
            }
        }
    }
    // Фаза 3.3a: `crud::require_admin` переведено на `write_gate::read_pool`
    // (роль-чек — SELECT, читання з репліки дозволене §10; `admin_pool` там давав
    // 403 на standby і блокував LocalOutbox-інвентаризацію). Лишилось 8 викликів
    // `admin_pool` — усі в місцях, де пул справді використовується для запису.
    assert!(
        found >= 8,
        "очікували ≥8 викликів admin_pool у src, знайшли {found}"
    );
    assert!(
        missing.is_empty(),
        "друга лінія захисту без політики (на standby поверне репліку):\n{}",
        missing.join("\n")
    );
    eprintln!("[guard] admin_pool: {found} викликів, усі мають політику");
}
