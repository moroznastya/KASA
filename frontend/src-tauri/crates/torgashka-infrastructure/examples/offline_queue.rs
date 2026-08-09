//! CLI-контур офлайн-черги (етап 5) — для E2E-скрипта scripts/e2e_stage5_tauri.sh.
//!
//! Використовує ТОЙ САМИЙ код, що Tauri-команди `offline::commands`
//! (`OfflineDatabase`), тож виконує РЕАЛЬНИЙ шлях збереження/читання
//! SQLite-черги на диску (без GUI).
//!
//! Ізоляція даних: `XDG_DATA_HOME=<dir>` → БД у `<dir>/torgashka/offline.db`.
//!
//! Підкоманди:
//!   save <json>   — зберегти чек у чергу, друкує id
//!   count         — кількість несинхронізованих чеків
//!   list          — список несинхронізованих чеків (JSON-рядки)
//!   mark <id>     — позначити чек синхронізованим
//!
//! Приклади:
//!   XDG_DATA_HOME=/tmp/torgashka-stage5 \
//!     cargo run -p torgashka-infrastructure --example offline_queue -- save '{"receipt_type":"sale"}'
//!   XDG_DATA_HOME=/tmp/torgashka-stage5 \
//!     cargo run -p torgashka-infrastructure --example offline_queue -- count

use torgashka_infrastructure::offline::db::OfflineDatabase;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("підкоманда: save <json> | count | list | mark <id>");
        std::process::exit(2);
    }

    let db = OfflineDatabase::new().expect("offline.db не відкрилась");

    match args[1].as_str() {
        "save" => {
            let json = args
                .get(2)
                .unwrap_or_else(|| {
                    eprintln!("save потребує JSON-аргумент");
                    std::process::exit(2);
                })
                .as_str();
            let id = db.save_receipt_offline(json).expect("save_receipt_offline");
            println!("{id}");
        }
        "count" => {
            println!("{}", db.count_unsynced_receipts().expect("count"));
        }
        "list" => {
            for r in db.get_unsynced_receipts().expect("get_unsynced_receipts") {
                println!("{}", serde_json::to_string(&r).expect("serialize"));
            }
        }
        "mark" => {
            let id: i64 = args
                .get(2)
                .unwrap_or_else(|| {
                    eprintln!("mark потребує id");
                    std::process::exit(2);
                })
                .parse()
                .expect("id — ціле число");
            db.mark_receipt_synced(id).expect("mark_receipt_synced");
            println!("ok");
        }
        other => {
            eprintln!("невідома підкоманда: {other}");
            std::process::exit(2);
        }
    }
}
