//! CI-guard відсутності обходу WriteGate (ADR-0007 §11.4, критерій E контракту).
//!
//! 1. **Статичний скан** `crates/torgashka-api/src/**/*.rs`: кожен DML-літерал
//!    (`INSERT INTO` / `UPDATE <таблиця>` / `DELETE FROM` / `CREATE ROLE` /
//!    `ALTER ROLE`) мусить мати або (а) виняток приймача (`sync.rs`,
//!    `sync_receivers.rs`), або (б) сутність із `POLICY_TABLE` (§11.1).
//!    Новий DML без політики → тест падає.
//! 2. **Негативний контроль**: синтетичний текст із `INSERT INTO
//!    non_policy_table` ДОВОДИТЬ, що скан реально ловить порушення
//!    (відтворювано в репо, без «зламай-і-відкоти»).
//! 3. **Повнота**: реєстр 41 write-точки §3 (згруповано за класами §3.5)
//!    звіряється з `POLICY_TABLE` автоматично; розбіжність → падіння.
//!
//! Тест не потребує PostgreSQL і не робить жодного I/O у мережу.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use torgashka_api::write_gate::{policy_for, WritePolicy, POLICY_TABLE};

// ─────────────────────────────────────────────────────────────────────────────
// Чиста функція скану (тестується на синтетичних джерелах — негативний контроль)
// ─────────────────────────────────────────────────────────────────────────────

/// Файли-приймачі: DML тут — частина приймача черги кас (§11.4 п.1 «виняток
/// приймача»); їхні сутності все одно перевіряються, якщо є в `POLICY_TABLE`.
const RECEIVER_FILES: &[&str] = &["sync.rs", "sync_receivers.rs"];

/// Сутність DDL-провіжну ролі реплікації (§3.2 #34–#35).
const DDL_ENTITY: &str = "replication_ddl_role";

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
        return Some(strip_schema(&word));
    }
    if let Some(idx) = t.find("DELETE FROM ") {
        let rest = &t[idx + "DELETE FROM ".len()..];
        let word: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        return Some(strip_schema(&word));
    }
    if t.contains("CREATE ROLE") || t.contains("ALTER ROLE") {
        return Some(DDL_ENTITY.to_string());
    }
    dml_update_entity(t)
}

/// Скан джерел: `(імʼя файлу, текст)` → список порушень.
///
/// Порушення = DML-рядок, чия сутність відсутня в `POLICY_TABLE` і чий файл
/// не є файлом-приймачем.
pub fn scan_dml_violations(sources: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, text) in sources {
        let base = Path::new(name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| name.clone());
        let receiver = RECEIVER_FILES.contains(&base.as_str());
        for (i, line) in text.lines().enumerate() {
            if !is_dml_line(line) {
                continue;
            }
            let Some(entity) = dml_entity(line) else {
                continue;
            };
            if policy_for(&entity).is_some() || receiver {
                continue;
            }
            out.push(format!(
                "{}:{}: `{}` — сутність '{entity}' відсутня в POLICY_TABLE (§11.1)",
                name,
                i + 1,
                line.trim()
            ));
        }
    }
    out
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

#[test]
fn every_policy_entity_is_covered_by_adr_registry_or_document_channel() {
    let mut covered: BTreeSet<&str> = BTreeSet::new();
    for (_, entity, _) in ADR_REGISTRY {
        covered.insert(entity);
    }
    for e in DOCUMENT_CHANNEL {
        covered.insert(e);
    }
    let missing: Vec<&str> = POLICY_TABLE
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !covered.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "сутності POLICY_TABLE без write-точки в §3/§11.1: {missing:?}"
    );
    assert_eq!(
        POLICY_TABLE.len(),
        25,
        "таблиця політик §11.1 = 23 рядки + 2 рядки §11.6 (cash_operation, write_off_reason)"
    );
    eprintln!(
        "[guard] POLICY_TABLE: {} рядків, усі покриті реєстром §3 або каналом документів",
        POLICY_TABLE.len()
    );
}
