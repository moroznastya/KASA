//! Життєвість device-каси у ЛОКАЛЬНОМУ SQLite вузла.
//!
//! Канал створено для вузла, який не міг писати в локальну БД; ADR-0008 §8 п.7
//! прямо лишає цей SQLite-канал присутності пристроїв (E7 видалив із модуля
//! лише standby-heartbeat до primary).
//!
//! Один тест у власному бінарі — щоб `XDG_DATA_HOME` (tempdir) не впливав на
//! інші тести (env процесу).

use torgashka_infrastructure::standby_heartbeat::record_device_seen;

#[test]
fn record_device_seen_writes_local_sqlite() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_var("XDG_DATA_HOME", dir.path());

    let device = "11111111-2222-3333-4444-555555555555";
    record_device_seen(device).expect("перше серцебиття");
    record_device_seen(device).expect("повторне серцебиття (ідемпотентно)");

    let path = torgashka_infrastructure::offline::db::OfflineDatabase::default_db_path()
        .expect("шлях SQLite");
    let conn = rusqlite::Connection::open(&path).expect("SQLite каси");
    let (rows, seen): (i64, String) = conn
        .query_row(
            "SELECT COUNT(*), MAX(last_seen_at) FROM device_heartbeats WHERE device_id = ?1",
            [device],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("SELECT device_heartbeats");
    assert_eq!(rows, 1, "рядок пристрою рівно один (UPSERT, не дубль)");
    assert!(!seen.is_empty(), "last_seen_at заповнено: {seen}");

    // PG-репліка не чіпається: жодного підключення до неї цей шлях не робить —
    // достатньо того, що запис стався ЛИШЕ в SQLite каси.
    eprintln!("[standby_heartbeat] ✅ локальний журнал: rows={rows}, last_seen_at={seen}");
}
