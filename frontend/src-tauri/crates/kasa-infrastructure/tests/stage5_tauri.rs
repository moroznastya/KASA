//! Тестовий контур етапу 5 (Tauri-обгортка): друк чеків + офлайн-черга.
//!
//! Друк (open→pay→close через системний друк Linux):
//!   Реальний конвеєр `print::print_raster_image` — той самий, що викликає
//!   Tauri-команда `print_image` (PNG → ESC/POS NV raster → запис у пристрій).
//!   Фізичного термопринтера на dev-машині немає (/dev/usb/lp* відсутні),
//!   тому як МОК-пристрій використовується звичайний файл: `find_printer_port`
//!   приймає будь-який існуючий шлях, `write_to_printer_port` пише у нього
//!   ESC/POS байти. Перевіряємо структуру потоку: ESC @ (init), GS v 0
//!   (raster header), ESC d 8 (подача) на кожну копію, GS V 0 (обрізка).
//!
//! Офлайн-черга:
//!   `OfflineDatabase` (SQLite на диску) в ІЗОЛЬОВАНІЙ XDG_DATA_HOME —
//!   реальні дані користувача (~/.local/share/kasa-pos/offline.db) не чіпаються.
//!   Цикл: save → count=1 → (новий процес/екземпляр — персистентність) →
//!   get (дані ідентичні) → mark_receipt_synced → count=0.
//!   Сценарій «сервер down → черга; сервер up → синхронізація» покриває
//!   E2E-скрипт scripts/e2e_stage5_tauri.sh (real API + real offline.db).

use image::Luma;

// ═════════════════════════════════════════════════════════════════════════
// 1. ДРУК: PNG чека → ESC/POS → мок-пристрій (файл)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn print_receipt_to_mock_device() {
    // ── 1. PNG «чека»: 384px ширина (58мм @ 203dpi) — аналог того, що
    //    html2canvas знімає з React-шаблону чеку на фронтенді.
    let (w, h) = (384u32, 200u32);
    let mut img = image::ImageBuffer::from_pixel(w, h, Luma([255u8]));
    // Імітуємо вміст чека: чорні «лінії тексту» та рамка.
    for y in 0..h {
        for x in 0..w {
            if x < 6 || y < 6 || y % 24 == 0 {
                img.put_pixel(x, y, Luma([0u8]));
            }
        }
    }
    let mut png: Vec<u8> = Vec::new();
    image::DynamicImage::ImageLuma8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("PNG згенеровано");
    assert!(png.len() > 100, "PNG не порожній ({} байт)", png.len());

    // ── 2. Мок-пристрій: звичайний файл у /tmp.
    let mock_dev = std::env::temp_dir().join("kasa_mock_print.bin");
    let _ = std::fs::remove_file(&mock_dev);
    std::fs::write(&mock_dev, b"").expect("мок-файл створено");

    // ── 3. Реальний конвеєр друку (2 копії, з обрізкою).
    kasa_infrastructure::print::print_raster_image(
        png,
        None,                             // printer_name: без CUPS (мок-пристрій)
        Some(mock_dev.to_str().unwrap()), // device_path → мок-файл
        2,                                // copies
        true,                             // auto_cut
        None,                             // width_mm (чек, не етикетка)
        None,                             // height_mm
        None,                             // dpi (дефолт 203)
    )
    .expect("друк у мок-пристрій не впав");

    // ── 4. Перевірка ESC/POS потоку.
    let data = std::fs::read(&mock_dev).expect("мок-файл читається");
    assert!(!data.is_empty(), "ESC/POS байти записані у пристрій");

    // ESC @ — ініціалізація принтера (один раз).
    assert_eq!(&data[..2], &[0x1B, 0x40], "потік починається з ESC @");

    // GS v 0 — NV raster header (друк зображення).
    assert!(
        data.windows(3).any(|w| w == [0x1D, 0x76, 0x30]),
        "присутній GS v 0 (raster block)"
    );

    // ESC d 8 — подача паперу ПІСЛЯ КОЖНОЇ копії (copies=2 → 2 рази).
    let feeds = data.windows(3).filter(|w| *w == [0x1B, 0x64, 0x08]).count();
    assert_eq!(feeds, 2, "2 копії → 2 подачі паперу, знайдено {feeds}");

    // GS V 0 — обрізка паперу в кінці (auto_cut=true).
    assert!(
        data.ends_with(&[0x1D, 0x56, 0x00]),
        "потік завершується GS V 0 (обрізка)"
    );

    eprintln!(
        "[stage5] ✅ ESC/POS: {} байт, ESC @ + GS v 0 + {}×подача + GS V",
        data.len(),
        feeds
    );

    let _ = std::fs::remove_file(&mock_dev);
}

// ═════════════════════════════════════════════════════════════════════════
// 2. ОФЛАЙН-ЧЕРГА: SQLite на диску, roundtrip save→count→get→mark
// ═════════════════════════════════════════════════════════════════════════

fn temp_data_home() -> std::path::PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "kasa-stage5-offline-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).expect("тимчасова XDG_DATA_HOME");
    tmp
}

#[test]
fn offline_queue_roundtrip_on_disk() {
    // Ізольована XDG_DATA_HOME — OfflineDatabase::new() створить
    // <tmp>/kasa-pos/offline.db (реальний файл SQLite, WAL).
    let tmp = temp_data_home();
    // Rust 2021: set_var безпечний (не unsafe, як в edition 2024).
    std::env::set_var("XDG_DATA_HOME", &tmp);

    // ── save ─────────────────────────────────────────────────────────
    let db = kasa_infrastructure::offline::db::OfflineDatabase::new()
        .expect("offline.db створена у тимчасовій XDG_DATA_HOME");
    assert!(
        db.get_db_path().starts_with(tmp.to_str().unwrap()),
        "БД лежить у тимчасовій директорії: {}",
        db.get_db_path()
    );

    let receipt = serde_json::json!({
        "receipt_type": "sale",
        "items": [{"product_id": "t-1", "quantity": 2, "price": "100.00", "tax_rate": 20}],
        "payment_method": "cash",
        "cash_amount": "200.00",
        "total_amount": "200.00",
    });
    let id = db
        .save_receipt_offline(&receipt.to_string())
        .expect("чек збережено у чергу");
    assert!(id > 0, "id у черзі: {id}");
    assert_eq!(
        db.count_unsynced_receipts().expect("count"),
        1,
        "1 чек у черзі після save"
    );

    // ── Персистентність: «перезапуск» — новий екземпляр, той самий файл ──
    drop(db);
    let db2 =
        kasa_infrastructure::offline::db::OfflineDatabase::new().expect("БД відкрита повторно");
    assert_eq!(
        db2.count_unsynced_receipts().expect("count"),
        1,
        "чек пережив перезапуск (збережено НА ДИСК)"
    );

    // ── get: дані ідентичні після roundtrip ──────────────────────────
    let unsynced = db2.get_unsynced_receipts().expect("get_unsynced");
    assert_eq!(unsynced.len(), 1, "1 чек у списку несинхронізованих");
    let saved: serde_json::Value =
        serde_json::from_str(unsynced[0]["data"].as_str().expect("data є рядком"))
            .expect("JSON валідний");
    assert_eq!(saved["payment_method"], "cash", "payment_method збережено");
    assert_eq!(saved["total_amount"], "200.00", "total_amount збережено");
    assert_eq!(
        unsynced[0]["id"].as_i64(),
        Some(id),
        "id у списку збігається"
    );

    // ── mark: синхронізовано → черга порожня ─────────────────────────
    db2.mark_receipt_synced(id).expect("mark_receipt_synced");
    assert_eq!(
        db2.count_unsynced_receipts().expect("count"),
        0,
        "черга порожня після синхронізації"
    );

    eprintln!("[stage5] ✅ офлайн-черга: save id={id} → count=1 → on-disk → get → mark → count=0");

    // ── Прибирання ────────────────────────────────────────────────────
    drop(db2);
    let _ = std::fs::remove_dir_all(&tmp);
    std::env::remove_var("XDG_DATA_HOME");
}
