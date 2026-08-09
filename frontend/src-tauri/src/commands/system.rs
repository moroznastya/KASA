// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Tauri Команди системної інтеграції
// ─────────────────────────────────────────────────────────────────────────────
//
// Містить команди для:
//   - Гарячі клавіші (реєстрація/скасування)
//   - Сканер штрих-кодів (інформація про підключення)
//   - Статус системи (онлайн/офлайн, версія)
//   - Автозапуск
//   - Системні сповіщення (синхронізація, помилки ПРРО тощо)
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Tauri Команди
// ─────────────────────────────────────────────────────────────────────────────

/// Отримати версію застосунку
#[tauri::command]
pub fn get_app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

/// Отримати назву платформи
#[tauri::command]
pub fn get_platform() -> Result<String, String> {
    Ok(std::env::consts::OS.to_string())
}

/// Перевірити чи система онлайн
#[tauri::command]
pub fn check_online() -> bool {
    // Проста перевірка: пробуємо з'єднатися з localhost API
    std::net::TcpStream::connect_timeout(
        &"127.0.0.1:8000".parse().unwrap(),
        std::time::Duration::from_secs(2),
    )
    .is_ok()
}

/// Отримати інформацію про сканер штрих-кодів
///
/// USB/HID сканери штрих-кодів зазвичай працюють як клавіатурний ввід
/// (keyboard wedge mode), тому вони не потребують спеціальних драйверів.
/// Ця команда перевіряє наявність HID-пристроїв, які можуть бути сканерами.
#[tauri::command]
pub fn get_barcode_scanner_info() -> Result<serde_json::Value, String> {
    // На Linux: перевіряємо /dev/input/ та /dev/hidraw*
    let mut scanners = Vec::new();

    #[cfg(unix)]
    {
        for entry in std::fs::read_dir("/dev/input")
            .ok()
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("event") || name.starts_with("hidraw") {
                scanners.push(serde_json::json!({
                    "path": format!("/dev/input/{}", name),
                    "type": "hid",
                }));
            }
        }

        for entry in std::fs::read_dir("/dev/hidraw")
            .ok()
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().to_string();
            scanners.push(serde_json::json!({
                "path": format!("/dev/hidraw/{}", name),
                "type": "hidraw",
            }));
        }
    }

    Ok(serde_json::json!({
        "mode": "keyboard_wedge",
        "devices": scanners,
        "note": "Більшість сканерів штрих-кодів працюють в режимі клавіатурного введення та не потребують додаткових налаштувань."
    }))
}

/// Отримати список USB-пристроїв (для налагодження)
#[tauri::command]
pub fn get_usb_devices() -> Result<Vec<serde_json::Value>, String> {
    // Етап 0: тонка обгортка — логіка перенесена в torgashka-infrastructure::devices.
    Ok(torgashka_infrastructure::devices::list_usb_devices())
}

/// Отримати стан системи (для дашборду)
#[tauri::command]
pub fn get_system_status() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "online": check_online(),
        "hostname": std::env::var("HOSTNAME").unwrap_or_default(),
        "username": std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default(),
        "app_data_dir": dirs_next::data_dir().map(|d| d.join("torgashka").to_string_lossy().to_string()).unwrap_or_default(),
    }))
}

/// Отримати розкладку клавіатури
#[tauri::command]
pub fn get_keyboard_layout() -> Result<String, String> {
    let layout = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .unwrap_or_else(|_| "unknown".to_string());
    Ok(layout)
}

/// Відправити системне сповіщення
///
/// Використовується frontend для повідомлень про:
///   - синхронізацію офлайн-даних
///   - помилки ПРРО / фіскалізації
///   - завершення резервного копіювання тощо.
///
/// Приклад виклику з frontend:
///   invoke('send_notification', { title: 'Синхронізація', body: 'Дані оновлено' })
#[tauri::command]
pub fn send_notification(app: tauri::AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;

    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| format!("Помилка відправки сповіщення: {e}"))
}
