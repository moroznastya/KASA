// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Tauri Команди «Підключені пристрої»
// ─────────────────────────────────────────────────────────────────────────────
//
// Керування POS-обладнанням:
//   - COM-ваги (serialport): фоновий потік читає порт, парсить вагу та надсилає
//     подію "weight-updated". Протоколи: CAS-подібний (за замовчуванням) або
//     ВТА-60/…-5-АС (KLARUS): 2 (ENQ/ACK/DC1 + кадр SOH..EOT), 3 (пасивний,
//     3 рядки по 9 символів), 5 (бінарний BCD). Вибір — поле "protocol" у конфізі.
//   - WiFi/ethernet-термінали ПриватБанку (TCP): фоновий потік періодично
//     (кожні 5с) перевіряє доступність коротким connect_timeout і надсилає
//     подію "device-status-changed" лише при зміні статусу.
//   - Карткові операції (Purchase/Refund/Withdrawal/Ping) — через клієнт
//     протоколу ПриватБанк ECR (JSON), модуль pb_protocol.
//
// Конфіги пристроїв зберігаються у JSON-файлі:
//   app_data_dir/devices.json  (формат: масив DeviceConfig)
//
// КОНТРАКТ (camelCase) — фронтенд пише під ці структури та команди.
// ─────────────────────────────────────────────────────────────────────────────

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::terminal;

// ── Структури (контракт з фронтендом) ──────────────────────────────────────

/// Конфігурація підключеного пристрою
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceConfig {
    /// uuid v4 (генерується на бекенді, якщо порожній)
    pub id: String,
    /// назва пристрою
    pub name: String,
    /// "scale" | "terminal"
    pub device_type: String,
    /// автопідключення при старті
    pub enabled: bool,
    /// scale: { "port": "/dev/ttyUSB0", "baudRate": 9600, "protocol": "cas"|"vta2"|"vta3"|"vta5" }
    /// terminal: { "ip": "192.168.1.50", "tcpPort": 2024 }
    pub config: serde_json::Value,
}

/// Поточний статус пристрою
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatus {
    pub id: String,
    /// "connected" | "disconnected" | "error"
    pub status: String,
    pub error: Option<String>,
    pub last_weight: Option<f64>,
}

// ── CUPS-принтери та автовиявлення (контракт з фронтендом) ─────────────────

/// Інформація про системний CUPS-принтер (lpstat -p -d)
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrinterInfo {
    /// назва CUPS-принтера
    pub name: String,
    /// "idle" | "printing" | "disabled" | "error"
    pub status: String,
    /// системний за замовчуванням
    pub is_default: bool,
}

/// USB-пристрій (логіка та сама, що в commands/system.rs get_usb_devices).
/// Deserialize — щоб конвертувати json з system::get_usb_devices.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsbDevice {
    /// ім'я вузла /sys/bus/usb/devices (напр. "1-1.2")
    pub name: String,
    /// PRODUCT= з uevent
    pub product: String,
    /// DEVICE= з uevent
    pub device: String,
}

/// Сканер, виявлений через SANE (`scanimage -L`).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScannerInfo {
    /// ідентифікатор SANE, напр. "genesys:libusb:001:003"
    pub device: String,
    /// опис, напр. "CANON CanoScan LiDE 110 flatbed scanner"
    pub name: String,
}

/// Результат автовиявлення всіх підключених пристроїв
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DetectedDevices {
    pub printers: Vec<PrinterInfo>,
    pub serial_ports: Vec<String>,
    pub usb_devices: Vec<UsbDevice>,
    pub scanners: Vec<ScannerInfo>,
}

// ── Глобальний стан активних з'єднань ──────────────────────────────────────

/// Дескриптор активного фонового з'єднання
struct ConnectionHandle {
    /// прапорець зупинки потоку
    stop: Arc<AtomicBool>,
    /// потік драйвера
    thread: Option<JoinHandle<()>>,
    /// поточний статус (оновлюється потоком)
    status: Arc<Mutex<DeviceStatus>>,
    /// поточна ціна для ваг (протокол 5, Режим 2). None = Режим 3 (без ціни).
    price: Arc<Mutex<Option<f64>>>,
}

fn connections() -> &'static Mutex<HashMap<String, ConnectionHandle>> {
    static CONNECTIONS: OnceLock<Mutex<HashMap<String, ConnectionHandle>>> = OnceLock::new();
    CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

// ── Робота з файлом конфігів ────────────────────────────────────────────────

fn devices_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("створення директорії: {e}"))?;
    Ok(dir.join("devices.json"))
}

fn load_devices(app: &AppHandle) -> Result<Vec<DeviceConfig>, String> {
    let path = devices_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("читання devices.json: {e}"))?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).map_err(|e| format!("парсинг devices.json: {e}"))
}

fn save_devices(app: &AppHandle, devices: &[DeviceConfig]) -> Result<(), String> {
    let path = devices_path(app)?;
    let raw = serde_json::to_string_pretty(devices).map_err(|e| format!("серіалізація: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("запис devices.json: {e}"))
}

// ── Статуси та події ────────────────────────────────────────────────────────

fn set_status(
    status: &Arc<Mutex<DeviceStatus>>,
    new_status: &str,
    error: Option<String>,
    last_weight: Option<f64>,
) {
    if let Ok(mut st) = status.lock() {
        st.status = new_status.to_string();
        st.error = error;
        if last_weight.is_some() {
            st.last_weight = last_weight;
        }
    }
}

fn emit_status(app: &AppHandle, id: &str, status: &Arc<Mutex<DeviceStatus>>) {
    let payload = status
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|_| DeviceStatus {
            id: id.to_string(),
            status: "error".to_string(),
            error: Some("статус недоступний".to_string()),
            last_weight: None,
        });
    let _ = app.emit("device-status-changed", payload);
}

// ── Парсинг ваги (CAS-подібний протокол) ───────────────────────────────────
// Без regex: збираємо байти в рядок, витягуємо перше число з плаваючою
// крапкою у правдоподібному діапазоні ваги (0.001..=10000).

/// Толерантність стабільності ваги, кг (±2 г): зміни в межах цього значення
/// вважаються шумом і НЕ надсилаються фронтенду.
const WEIGHT_TOLERANCE: f64 = 0.002;

/// Нормалізація сирого буфера COM-ваги:
/// - 0x08 (backspace) — стирає попередній символ (CAS "виправляє" передачу);
/// - 0x02/0x03 (STX/ETX), 0x0D/0x0A (CR/LF) — службові, ігноруються;
/// - решта символів (цифри, крапка/кома, пробіли, літери "kg") — зберігаються.
fn clean_scale_buffer(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c as u32 {
            0x08 => {
                out.pop();
            }
            0x02 | 0x03 | 0x0D | 0x0A => {}
            _ => out.push(c),
        }
    }
    out
}

/// Фільтр стабільності ваги: подія емітиться лише коли значення
/// (а) підтверджене двома послідовними читаннями в межах tolerance і
/// (б) відрізняється від останнього відправленого більш ніж на tolerance.
/// Повертає true, якщо поточне читання треба відправити фронтенду.
fn should_emit_weight(
    last_emitted: &mut Option<f64>,
    pending: &mut Option<f64>,
    weight: f64,
) -> bool {
    if let Some(p) = *pending {
        if (weight - p).abs() <= WEIGHT_TOLERANCE {
            // підтверджено двома послідовними читаннями
            *pending = None;
            let changed = last_emitted
                .map(|le| (weight - le).abs() > WEIGHT_TOLERANCE)
                .unwrap_or(true);
            if changed {
                *last_emitted = Some(weight);
            }
            return changed;
        }
    }
    *pending = Some(weight);
    false
}

fn parse_weight(s: &str) -> Option<f64> {
    // 1) backspace + викидання службових символів (STX/ETX/CR/LF)
    let cleaned = clean_scale_buffer(s);
    // 2) CAS-подібний протокол: цифри приходять окремими токенами через
    //    пробіли ("1 2 3 . 4 5" = 123.45) — склеюємо пробіли/таби.
    //    Суфікс "kg" при цьому лишається природним розділювачем числа.
    let compact: String = cleaned
        .chars()
        .filter(|c| *c != ' ' && *c != '\t')
        .collect();
    // 3) витягуємо перше число з плаваючою крапкою
    let chars: Vec<char> = compact.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() || chars[i] == '-' {
            let start = i;
            let mut has_sep = false;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == ','
                    || chars[i] == '-')
            {
                if chars[i] == '.' || chars[i] == ',' {
                    has_sep = true;
                }
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            // «Число з плаваючою крапкою»: вимагаємо розділювач (крапку/кому)
            if has_sep {
                if let Ok(v) = token.replace(',', ".").parse::<f64>() {
                    if (0.001..=10000.0).contains(&v) {
                        return Some(v);
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

// ── Парсинг ВТА-60 (KLARUS): протоколи 2, 3, 5 ─────────────────────────────
// Торгові ваги ВТА-60/…-5-АС (KLARUS) мають три апаратні протоколи RS-232:
//   2 — ASCII запит-відповідь (ENQ→ACK→DC1, кадр SOH..EOT з BCC);
//   3 — ASCII пасивний (3 рядки по 9 символів + CR LF);
//   5 — бінарний BCD (запит 8 байт, відповідь 17 байт).

/// Парсинг кадру протоколу 2 (ВТА-60). Очікує рівно 15 байт:
/// SOH(0x01) STX(0x02) STA SIGN W5 W4 W3 W2 W1 W0 UN1 UN2 BCC ETX(0x03) EOT(0x04).
///
/// - STA: лише 'S' (стабільна маса); 'U' (нестабільна) — ігнор;
/// - SIGN: лише ' ' (додатна); '-' (від'ємна) і 'F' (перевантаження) — відкинути;
/// - W5..W0: 6 ASCII цифр (W5 — старший розряд);
/// - UN1 UN2: одиниці; наявність 'k' → кг (raw/1000.0), інакше грами (raw);
/// - BCC: XOR. Толерантно: XOR від STX до UN2 включно, або XOR від STA до UN2.
fn parse_vta2_frame(frame: &[u8]) -> Option<f64> {
    if frame.len() < 15 {
        return None;
    }
    let f = &frame[..15];
    if f[0] != 0x01 || f[1] != 0x02 || f[13] != 0x03 || f[14] != 0x04 {
        return None;
    }
    // приймаємо ТІЛЬКИ стабільну масу
    if f[2] != b'S' {
        return None;
    }
    // тільки додатна маса (SIGN = пробіл)
    if f[3] != b' ' {
        return None;
    }
    let digits = &f[4..10];
    // Новий формат (реальні ВАГ-60): десяткова крапка/кома в полі маси,
    // напр. " 0.000", " 5.123", "60.000". Пробіли та кома — допустимі.
    let decimal_format = digits.contains(&b'.') || digits.contains(&b',');
    // BCC: два варіанти XOR (від STX або від STA, до UN2 включно)
    let bcc_stx = f[1..12].iter().fold(0u8, |acc, b| acc ^ b);
    let bcc_sta = f[2..12].iter().fold(0u8, |acc, b| acc ^ b);
    if f[12] != bcc_stx && f[12] != bcc_sta {
        return None;
    }
    let in_kg = f[10] == b'k' || f[11] == b'k';
    if decimal_format {
        // trim пробілів, ',' → '.', parse::<f64>
        let s: String = digits
            .iter()
            .map(|b| {
                let ch = *b as char;
                if ch == ',' {
                    '.'
                } else {
                    ch
                }
            })
            .collect();
        let value = s.trim().parse::<f64>().ok()?;
        // 'k' → значення ВЖЕ в кг (як є); інакше грами → /1000
        return if in_kg {
            Some(value)
        } else {
            Some(value / 1000.0)
        };
    }
    // Старий формат: 6 чистих ASCII цифр (без крапки)
    if !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let raw: i64 = digits
        .iter()
        .fold(0i64, |acc, b| acc * 10 + i64::from(b - b'0'));
    if in_kg {
        Some(raw as f64 / 1000.0)
    } else {
        Some(raw as f64)
    }
}

/// Пошук кадру протоколу 2 у потоці байтів. Повертає (вага, скільки байтів
/// спожито) або None, якщо повного кадру ще немає.
fn parse_vta2_stream(buf: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    while i + 15 <= buf.len() {
        if buf[i] == 0x01 && buf[i + 1] == 0x02 {
            if let Some(w) = parse_vta2_frame(&buf[i..i + 15]) {
                return Some((w, i + 15));
            }
        }
        i += 1;
    }
    None
}

/// Витяг першого числа з десятковою крапкою з 9-символьного рядка протоколу 3
/// (рядок може містити пробіли й суфікс одиниць, напр. "  1.500kg").
fn parse_vta3_line(line: &[u8]) -> Option<f64> {
    let s = std::str::from_utf8(line).ok()?;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() || chars[i] == '-' || chars[i] == '+' {
            let start = i;
            let mut has_dot = false;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == '-'
                    || chars[i] == '+')
            {
                if chars[i] == '.' {
                    has_dot = true;
                }
                i += 1;
            }
            if has_dot {
                let token: String = chars[start..i].iter().collect();
                return token.parse::<f64>().ok();
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Парсинг потоку протоколу 3 (ВТА-60, пасивний): 3 рядки по 9 символів +
/// CR LF після кожного. Рядок 1 = маса. Шукаємо "9 символів + CR LF" з
/// десятковою крапкою в перших 7 символах; рядки без крапки ігноруються.
/// Повертає (маса, спожито байтів).
fn parse_vta3_stream(buf: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    while i + 11 <= buf.len() {
        if buf[i + 9] == 0x0D && buf[i + 10] == 0x0A {
            let line = &buf[i..i + 9];
            if line[..7].contains(&b'.') {
                if let Some(v) = parse_vta3_line(line) {
                    return Some((v, i + 11));
                }
            }
        }
        i += 1;
    }
    None
}

/// Парсинг відповіді протоколу 5 (ВТА-60, бінарний BCD). Очікує 17 байт:
/// М0..М5 (маса, М0 — молодший розряд), Ц0..Ц4 (ціна), С0..С5 (вартість).
/// Кожен байт — неупакований BCD (старша тетрада = 0). Маса = цифри у
/// зворотному порядку (М5..М0), /1000.0 (кг, 3 десяткові знаки).
fn parse_vta5_frame(frame: &[u8]) -> Option<f64> {
    if frame.len() < 17 {
        return None;
    }
    let f = &frame[..17];
    // усі байти мають бути неупакованим BCD (0x00..=0x09)
    if !f.iter().all(|b| *b <= 0x09) {
        return None;
    }
    // М5..М0 у зворотному порядку: М5 — найстарший розряд
    let raw: i64 = f[..6]
        .iter()
        .rev()
        .fold(0i64, |acc, b| acc * 10 + i64::from(*b));
    Some(raw as f64 / 1000.0)
}

// ── Фонові потоки ───────────────────────────────────────────────────────────

/// Драйвер COM-ваги: відкриває порт і запускає цикл драйвера за протоколом.
/// protocol: "cas" (за замовчуванням) | "vta2" | "vta3" | "vta5".
fn spawn_scale(
    app: AppHandle,
    id: String,
    port: String,
    baud_rate: u32,
    protocol: String,
    status: Arc<Mutex<DeviceStatus>>,
    stop: Arc<AtomicBool>,
    price: Arc<Mutex<Option<f64>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        // Протокол 5 (ВТА-60, BCD) працює з парністю EVEN (8E1) — обов'язково.
        // Решта протоколів — 8N1 (парність за замовчуванням serialport).
        let mut builder = serialport::new(&port, baud_rate).timeout(Duration::from_millis(500));
        if protocol == "vta5" {
            builder = builder.parity(serialport::Parity::Even);
        }
        let serial = match builder.open()
        {
            Ok(p) => {
                crate::embedded_pg::pg_log(
                    "INFO",
                    &format!(
                        "scale {id}: порт {port} відкрито ({baud_rate} бод, протокол {protocol})"
                    ),
                );
                p
            }
            Err(e) => {
                crate::embedded_pg::pg_log(
                    "ERROR",
                    &format!("scale {id}: помилка відкриття порту {port}: {e}"),
                );
                set_status(
                    &status,
                    "error",
                    Some(format!("відкриття {port}: {e}")),
                    None,
                );
                emit_status(&app, &id, &status);
                return;
            }
        };
        set_status(&status, "connected", None, None);
        emit_status(&app, &id, &status);

        // id/port/app/status рухаються в драйвер — для фінального emit тримаємо копії
        let id_disp = id.clone();
        let app_disp = app.clone();
        let status_disp = status.clone();
        match protocol.as_str() {
            "vta2" => scale_vta2_loop(serial, app, id, port, status, stop),
            "vta3" => scale_vta3_loop(serial, app, id, port, status, stop),
            "vta5" => scale_vta5_loop(serial, app, id, port, status, stop, price),
            _ => scale_cas_loop(serial, app, id, port, status, stop),
        }

        set_status(&status_disp, "disconnected", None, None);
        emit_status(&app_disp, &id_disp, &status_disp);
    })
}

/// CAS-драйвер (протокол за замовчуванням): циклічно читає, парсить вагу та
/// емітить подію "weight-updated". Статус оновлюється через "device-status-changed".
///
/// АДАПТИВНИЙ ЗАПИТ (ФІКС 2026-08-21, ваги «Пром Прилад»): більшість
/// українських торгових ваг працюють у режимі ЗАПИТ-ВІДПОВІДЬ, а не
/// постійної передачі. Якщо дані не надходили останні 1.5с — драйвер
/// надсилає запит (по черзі ENQ 0x05 → 'W' 0x57 → 'P' 0x50, пауза 300мс),
/// поки не прийде відповідь. Щойно дані прийшли — запити припиняються
/// (continuous mode ваг не ламається).
fn scale_cas_loop(
    mut serial: Box<dyn serialport::SerialPort>,
    app: AppHandle,
    id: String,
    port: String,
    status: Arc<Mutex<DeviceStatus>>,
    stop: Arc<AtomicBool>,
) {
    let mut buf = [0u8; 256];
    let mut acc = String::new();
    // фільтр стабільності: останнє відправлене значення + кандидат на підтвердження
    let mut last_emitted: Option<f64> = None;
    let mut pending: Option<f64> = None;
    // ── адаптивний запит ──
    let mut last_data = std::time::Instant::now();
    let mut request_index: usize = 0; // ENQ → 'W' → 'P' по колу
    let mut last_request_at: Option<std::time::Instant> = None;
    let mut silence_logged = false; // перша спроба запиту після тиші → в лог
    let mut write_error_logged = false; // перша помилка запису підряд → в лог
    const SILENCE: Duration = Duration::from_millis(1500);
    const REQUEST_GAP: Duration = Duration::from_millis(300);
    const PROBES: [u8; 3] = [0x05, b'W', b'P'];

    while !stop.load(Ordering::Relaxed) {
        // Тиша > 1.5с → ваги в режимі запит-відповідь: надсилаємо наступний
        // запит (не частіше ніж раз на 300мс — не спамимо).
        if last_data.elapsed() > SILENCE {
            let gap_ok = match last_request_at {
                Some(t) => t.elapsed() >= REQUEST_GAP,
                None => true,
            };
            if gap_ok {
                let b = PROBES[request_index % PROBES.len()];
                request_index += 1;
                let ok = serial.write(&[b]).is_ok() && serial.flush().is_ok();
                if ok {
                    if !silence_logged {
                        crate::embedded_pg::pg_log(
                            "INFO",
                            &format!("scale {id}: тиша >1.5с — запит ваги 0x{b:02X}"),
                        );
                        silence_logged = true;
                    }
                    write_error_logged = false;
                } else if !write_error_logged {
                    crate::embedded_pg::pg_log(
                        "ERROR",
                        &format!("scale {id}: помилка запису запиту 0x{b:02X} — продовжую"),
                    );
                    write_error_logged = true;
                }
                last_request_at = Some(std::time::Instant::now());
            }
        } else {
            // Дані приходять постійно — запити припинено.
            silence_logged = false;
            write_error_logged = false;
        }

        match serial.read(&mut buf) {
            Ok(n) if n > 0 => {
                last_data = std::time::Instant::now();
                silence_logged = false;
                write_error_logged = false;
                acc.push_str(&String::from_utf8_lossy(&buf[..n]));
                // обмежуємо буфер, щоб не розрісся
                if acc.len() > 1024 {
                    acc = acc
                        .chars()
                        .rev()
                        .take(512)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                }
                if let Some(weight) = parse_weight(&acc) {
                    acc.clear();
                    handle_weight(&app, &id, &status, &mut last_emitted, &mut pending, weight);
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                // таймаут читання — просто продовжуємо цикл
            }
            Err(e) => {
                crate::embedded_pg::pg_log(
                    "ERROR",
                    &format!("scale {id}: втрата порту {port}: {e}"),
                );
                set_status(
                    &status,
                    "error",
                    Some(format!("втрата порту {port}: {e}")),
                    None,
                );
                emit_status(&app, &id, &status);
                break;
            }
        }
    }
}

/// Оновлення last_weight + подія "weight-updated" (з фільтром стабільності ±2г).
fn handle_weight(
    app: &AppHandle,
    id: &str,
    status: &Arc<Mutex<DeviceStatus>>,
    last_emitted: &mut Option<f64>,
    pending: &mut Option<f64>,
    weight: f64,
) {
    if let Ok(mut st) = status.lock() {
        st.last_weight = Some(weight);
    }
    if should_emit_weight(last_emitted, pending, weight) {
        let _ = app.emit("weight-updated", json!({ "deviceId": id, "value": weight }));
    }
}

/// Сон з перевіркою прапора зупинки (швидкий вихід із циклів драйвера).
fn sleep_interruptible(stop: &AtomicBool, dur: Duration) {
    let mut slept = Duration::ZERO;
    while slept < dur && !stop.load(Ordering::Relaxed) {
        let step = std::cmp::min(Duration::from_millis(100), dur - slept);
        thread::sleep(step);
        slept += step;
    }
}

/// Очікування ACK (0x06) після ENQ: таймаут ~500мс.
fn wait_vta2_ack(serial: &mut Box<dyn serialport::SerialPort>, stop: &AtomicBool) -> bool {
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut b = [0u8; 1];
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        match serial.read(&mut b) {
            Ok(n) if n > 0 && b[0] == 0x06 => return true,
            Ok(_) => {} // інші байти ігноруємо
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => return false,
        }
    }
    false
}

/// Читання кадру протоколу 2: накопичує байти до знаходження валідного
/// кадру SOH..EOT (таймаут ~1.5с).
fn read_vta2_frame(serial: &mut Box<dyn serialport::SerialPort>, stop: &AtomicBool) -> Option<f64> {
    let mut buf: Vec<u8> = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        let mut tmp = [0u8; 64];
        match serial.read(&mut tmp) {
            Ok(n) if n > 0 => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some((w, _)) = parse_vta2_stream(&buf) {
                    return Some(w);
                }
                // тримаємо останні 64 байти (кадр 15 байт — з запасом)
                if buf.len() > 128 {
                    buf.drain(..buf.len() - 64);
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => return None,
        }
    }
    None
}

/// Читання відповіді протоколу 5: 17 байт неупакованого BCD. Ковзне вікно —
/// захист від зсуву (залишкові байти попередніх відповідей).
fn read_vta5_response(
    serial: &mut Box<dyn serialport::SerialPort>,
    stop: &AtomicBool,
) -> Option<f64> {
    let mut buf: Vec<u8> = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        let mut tmp = [0u8; 32];
        match serial.read(&mut tmp) {
            Ok(n) if n > 0 => {
                buf.extend_from_slice(&tmp[..n]);
                while buf.len() >= 17 {
                    if let Some(w) = parse_vta5_frame(&buf[..17]) {
                        return Some(w);
                    }
                    buf.remove(0);
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => return None,
        }
    }
    None
}

/// Драйвер протоколу 2 (ВТА-60): ENQ → ACK → DC1 → кадр SOH..EOT.
fn scale_vta2_loop(
    mut serial: Box<dyn serialport::SerialPort>,
    app: AppHandle,
    id: String,
    port: String,
    status: Arc<Mutex<DeviceStatus>>,
    stop: Arc<AtomicBool>,
) {
    let mut last_emitted: Option<f64> = None;
    let mut pending: Option<f64> = None;
    const MAX_ENQ_ATTEMPTS: u32 = 3;

    while !stop.load(Ordering::Relaxed) {
        // 1) ENQ, поки не прийде ACK (таймаут ~500мс, максимум N спроб)
        let mut acked = false;
        for _ in 0..MAX_ENQ_ATTEMPTS {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if serial.write(&[0x05]).is_err() || serial.flush().is_err() {
                crate::embedded_pg::pg_log(
                    "ERROR",
                    &format!("scale {id}: помилка запису ENQ на {port}"),
                );
                set_status(&status, "error", Some(format!("запис ENQ на {port}")), None);
                emit_status(&app, &id, &status);
                return;
            }
            if wait_vta2_ack(&mut serial, &stop) {
                acked = true;
                break;
            }
        }
        if !acked {
            // ваги не відповіли — пауза 1с і новий цикл ENQ
            crate::embedded_pg::pg_log(
                "WARN",
                &format!("scale {id}: немає ACK після {MAX_ENQ_ATTEMPTS}×ENQ — пауза 1с"),
            );
            sleep_interruptible(&stop, Duration::from_secs(1));
            continue;
        }
        // 2) DC1 — запит даних маси
        if serial.write(&[0x11]).is_err() || serial.flush().is_err() {
            crate::embedded_pg::pg_log(
                "ERROR",
                &format!("scale {id}: помилка запису DC1 на {port}"),
            );
            set_status(&status, "error", Some(format!("запис DC1 на {port}")), None);
            emit_status(&app, &id, &status);
            return;
        }
        // 3) читання кадру SOH..EOT
        if let Some(weight) = read_vta2_frame(&mut serial, &stop) {
            handle_weight(&app, &id, &status, &mut last_emitted, &mut pending, weight);
        }
    }
}

/// Драйвер протоколу 3 (ВТА-60, пасивний): ваги самі передають 3 рядки по
/// 9 символів + CR LF; беремо перший рядок з крапкою (масу).
fn scale_vta3_loop(
    mut serial: Box<dyn serialport::SerialPort>,
    app: AppHandle,
    id: String,
    port: String,
    status: Arc<Mutex<DeviceStatus>>,
    stop: Arc<AtomicBool>,
) {
    let mut last_emitted: Option<f64> = None;
    let mut pending: Option<f64> = None;
    let mut buf: Vec<u8> = Vec::new();

    while !stop.load(Ordering::Relaxed) {
        let mut tmp = [0u8; 64];
        match serial.read(&mut tmp) {
            Ok(n) if n > 0 => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some((weight, consumed)) = parse_vta3_stream(&buf) {
                    buf.drain(..consumed);
                    handle_weight(&app, &id, &status, &mut last_emitted, &mut pending, weight);
                }
                // обмежуємо буфер
                if buf.len() > 128 {
                    buf.drain(..buf.len() - 64);
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                crate::embedded_pg::pg_log(
                    "ERROR",
                    &format!("scale {id}: втрата порту {port}: {e}"),
                );
                set_status(
                    &status,
                    "error",
                    Some(format!("втрата порту {port}: {e}")),
                    None,
                );
                emit_status(&app, &id, &status);
                return;
            }
        }
    }
}

/// Побудова запиту протоколу 5 (ВТА-60, бінарний BCD):
/// - Режим 2 (з ціною): 0x00 0x00 0x02 + 5 байт ціни BCD (неупакований,
///   молодший розряд першим, ціна в копійках, максимум 999.99 грн).
/// - Режим 3 (без ціни): 0x00 0x00 0x03 + 5 нульових байт.
/// УВАГА: Режим 3 ПЕРЕДАЄ нульову ціну у ваги — він обнуляє ціну ваг.
fn vta5_build_request(price: Option<f64>) -> [u8; 8] {
    let mut req = [0x00u8; 8];
    match price {
        None => {
            // Режим 3: запит без ціни (5 нульових байт ціни)
            req[2] = 0x03;
        }
        Some(p) => {
            // Режим 2: запит з ціною. Ціна в копійках, до 99_999 (999.99 грн).
            let cents = ((p * 100.0).round() as u64).min(99_999);
            req[2] = 0x02;
            let mut v = cents;
            for i in 0..5 {
                req[3 + i] = (v % 10) as u8; // Ц0 (молодший) — перший
                v /= 10;
            }
        }
    }
    req
}

/// Драйвер протоколу 5 (ВТА-60, бінарний BCD): періодичний запит
/// (кожні ~500мс), відповідь 17 байт, читання з таймаутом.
/// Ціна береться зі спільного стану: якщо задана — Режим 2 (ваги тримають
/// ціну і рахують вартість), якщо None — Режим 3 (без ціни).
fn scale_vta5_loop(
    mut serial: Box<dyn serialport::SerialPort>,
    app: AppHandle,
    id: String,
    port: String,
    status: Arc<Mutex<DeviceStatus>>,
    stop: Arc<AtomicBool>,
    price: Arc<Mutex<Option<f64>>>,
) {
    let mut last_emitted: Option<f64> = None;
    let mut pending: Option<f64> = None;
    let mut last_read: Option<f64> = None; // остання ПРОЧИТАНА вага (для логу змін)
    let mut last_sent_price: Option<u64> = None; // остання надіслана ціна (копійки)

    while !stop.load(Ordering::Relaxed) {
        let cur = *price.lock().expect("lock poisoned: price");
        // Копійки: логуємо зміну ціни, але запит надсилаємо КОЖЕН цикл —
        // протокол 5 це запит-відповідь: без запиту ваги мовчать.
        let cur_cents = cur.map(|p| ((p * 100.0).round() as u64).min(99_999));
        if cur_cents != last_sent_price {
            last_sent_price = cur_cents;
            crate::embedded_pg::pg_log(
                "INFO",
                &format!("scale {id}: ціна для ваг = {:?} коп.", cur_cents),
            );
        }
        let req = vta5_build_request(cur);
        if serial.write(&req).is_err() || serial.flush().is_err() {
            crate::embedded_pg::pg_log(
                "ERROR",
                &format!("scale {id}: помилка запису запиту BCD на {port}"),
            );
            set_status(
                &status,
                "error",
                Some(format!("запис запиту на {port}")),
                None,
            );
            emit_status(&app, &id, &status);
            return;
        }
        if let Some(weight) = read_vta5_response(&mut serial, &stop) {
            if last_read.map(|lr| (weight - lr).abs() > 0.0005).unwrap_or(true) {
                crate::embedded_pg::pg_log(
                    "INFO",
                    &format!("scale {id}: вага = {weight} кг"),
                );
            }
            last_read = Some(weight);
            handle_weight(&app, &id, &status, &mut last_emitted, &mut pending, weight);
        }
        // періодичність ~500мс
        sleep_interruptible(&stop, Duration::from_millis(500));
    }
}

/// Примусово закрити TCP-сокет через RST (SO_LINGER=0) замість звичайного FIN.
/// Потрібно для термінала Newland N950: він не відповідає на FIN, тому
/// звичайне закриття лишає сокет у стані FIN-WAIT-2 (накопичення).
#[cfg(unix)]
fn force_rst_close(stream: &std::net::TcpStream) {
    use std::os::fd::AsRawFd;
    let linger = libc::linger {
        l_onoff: 1,
        l_linger: 0,
    };
    // Безпечно: fd належить stream, linger — стекова змінна на час виклику
    unsafe {
        libc::setsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            &linger as *const libc::linger as *const libc::c_void,
            std::mem::size_of::<libc::linger>() as libc::socklen_t,
        );
    }
}

/// Windows: та сама логіка через Winsock2. Сигнатура setsockopt тут інша —
/// (SOCKET, c_int, c_int, *const c_char, c_int) замість (fd, ..., socklen_t).
///
/// ⚠️ УВАГА: crate libc на Windows визначає ТІЛЬКИ setsockopt — тип `linger`,
/// константи SOL_SOCKET/SO_LINGER існують лише в unix-модулях libc. Тому все
/// необхідне описано тут напряму за Winsock2 (WS2DEF.H): linger = {u16,u16},
/// SOL_SOCKET = 0xffff, SO_LINGER = 0x0080.
#[cfg(windows)]
fn force_rst_close(stream: &std::net::TcpStream) {
    use std::os::windows::io::AsRawSocket;
    // Winsock2 WS2DEF.H: typedef struct linger { u_short l_onoff; u_short l_linger; } LINGER;
    #[repr(C)]
    struct Linger {
        l_onoff: u16,
        l_linger: u16,
    }
    const SOL_SOCKET: libc::c_int = 0xffff; // Winsock2 winsock.h
    const SO_LINGER: libc::c_int = 0x0080; // Winsock2 winsock.h
    let linger = Linger {
        l_onoff: 1,
        l_linger: 0,
    };
    // Безпечно: SOCKET належить stream, linger — стекова змінна на час виклику
    unsafe {
        libc::setsockopt(
            stream.as_raw_socket() as libc::SOCKET,
            SOL_SOCKET,
            SO_LINGER,
            &linger as *const Linger as *const libc::c_char,
            std::mem::size_of::<Linger>() as libc::c_int,
        );
    }
}

/// Драйвер TCP-термінала: періодичний моніторинг доступності (БЕЗ постійного
/// з'єднання). Термінал Newland N950 приймає лише ОДНЕ TCP-з'єднання — тому
/// утримувати постійне з'єднання заборонено: воно монополізує порт і блокує
/// terminal_payment та нові підключення. Перевіряємо доступність коротким
/// connect_timeout (3с), сокет одразу закривається.
fn spawn_terminal(
    app: AppHandle,
    id: String,
    ip: String,
    tcp_port: u16,
    status: Arc<Mutex<DeviceStatus>>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let addr = format!("{ip}:{tcp_port}");
        let socket_addr: std::net::SocketAddr = match addr.parse() {
            Ok(a) => a,
            Err(e) => {
                set_status(
                    &status,
                    "error",
                    Some(format!("некоректна адреса {addr}: {e}")),
                    None,
                );
                emit_status(&app, &id, &status);
                return;
            }
        };

        // emit_status надсилаємо лише при зміні стану (патерн як у spawn_printer_monitor)
        let mut prev_key: Option<(String, Option<String>)> = None;
        while !stop.load(Ordering::Relaxed) {
            // Якщо зараз виконується операція з терміналом (terminal_payment,
            // terminal_refund тощо) — пропускаємо перевірку: термінал зайнятий,
            // а конкуруюче з'єднання створювати не можна
            let op_busy = TERMINAL_OP_LOCK
                .get()
                .map(|l| l.try_lock().is_err())
                .unwrap_or(false);
            if op_busy {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            match TcpStream::connect_timeout(&socket_addr, Duration::from_secs(3)) {
                Ok(_s) => {
                    // Примусове RST-закриття (SO_LINGER=0): термінал Newland N950
                    // не відповідає на FIN — без RST сокети залишаються у
                    // FIN-WAIT-2 та накопичуються
                    force_rst_close(&_s);
                    set_status(&status, "connected", None, None);
                }
                Err(e) => {
                    set_status(&status, "error", Some(format!("TCP {addr}: {e}")), None);
                }
            }

            let curr_key = status
                .lock()
                .map(|st| (st.status.clone(), st.error.clone()))
                .unwrap_or_else(|_| ("error".to_string(), None));
            if prev_key != Some(curr_key.clone()) {
                emit_status(&app, &id, &status);
                prev_key = Some(curr_key);
            }

            // Пауза 5 секунд (перевірка stop кожні 100мс — швидкий вихід)
            let mut waited = 0;
            while waited < 50 && !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
                waited += 1;
            }
        }
        set_status(&status, "disconnected", None, None);
        emit_status(&app, &id, &status);
    })
}

/// Драйвер CUPS-принтера: кожні ~10с опитує lpstat, шукає свій printerName
/// та оновлює статус. Подія "device-status-changed" надсилається лише при
/// зміні статусу (щоб не спамити фронтенд).
fn spawn_printer_monitor(
    app: AppHandle,
    id: String,
    printer_name: String,
    status: Arc<Mutex<DeviceStatus>>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut prev_key: Option<(String, Option<String>)> = None;
        loop {
            let printers = get_system_printers().unwrap_or_default();
            let found = printers.iter().find(|p| p.name == printer_name);

            match found {
                Some(p) if p.status == "idle" || p.status == "printing" => {
                    set_status(&status, "connected", None, None);
                }
                Some(p) if p.status == "disabled" => {
                    set_status(
                        &status,
                        "error",
                        Some("Принтер вимкнено в CUPS".to_string()),
                        None,
                    );
                }
                Some(p) => {
                    set_status(
                        &status,
                        "error",
                        Some(format!("Помилка CUPS-принтера: {}", p.status)),
                        None,
                    );
                }
                None => {
                    set_status(
                        &status,
                        "error",
                        Some("Принтер не знайдено в системі".to_string()),
                        None,
                    );
                }
            }

            let curr_key = status
                .lock()
                .map(|st| (st.status.clone(), st.error.clone()))
                .unwrap_or_else(|_| ("error".to_string(), None));
            if prev_key != Some(curr_key.clone()) {
                emit_status(&app, &id, &status);
                prev_key = Some(curr_key);
            }

            // пауза ~10с (з перевіркою stop щосекунди для швидкого виходу)
            let mut waited = 0;
            while waited < 10 && !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(1));
                waited += 1;
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }
        set_status(&status, "disconnected", None, None);
        emit_status(&app, &id, &status);
    })
}

// ── Управління з'єднаннями ──────────────────────────────────────────────────

/// Запустити фонове підключення за конфігом. Якщо пристрій уже підключений —
/// повертає поточний статус; якщо у стані error/disconnected — перезапускає.
fn start_connection(app: &AppHandle, cfg: &DeviceConfig) -> Result<DeviceStatus, String> {
    let mut map = connections().lock().expect("lock poisoned: connections");

    if let Some(h) = map.get(&cfg.id) {
        let st = h.status.lock().expect("lock poisoned: status").clone();
        if st.status == "connected" {
            return Ok(st);
        }
        // error/disconnected — перезапускаємо
        if let Some(mut h) = map.remove(&cfg.id) {
            h.stop.store(true, Ordering::Relaxed);
            if let Some(t) = h.thread.take() {
                let _ = t.join();
            }
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let status = Arc::new(Mutex::new(DeviceStatus {
        id: cfg.id.clone(),
        status: "disconnected".to_string(),
        error: None,
        last_weight: None,
    }));
    let price = Arc::new(Mutex::new(None::<f64>));

    let handle = match cfg.device_type.as_str() {
        "scale" => {
            let port = cfg
                .config
                .get("port")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "scale: не вказано port".to_string())?
                .to_string();
            let baud_rate = cfg
                .config
                .get("baudRate")
                .and_then(|v| v.as_u64())
                .unwrap_or(9600) as u32;
            // протокол ваг: "cas" (за замовчуванням) | "vta2" | "vta3" | "vta5"
            let protocol = cfg
                .config
                .get("protocol")
                .and_then(|v| v.as_str())
                .unwrap_or("cas")
                .to_string();
            spawn_scale(
                app.clone(),
                cfg.id.clone(),
                port,
                baud_rate,
                protocol,
                status.clone(),
                stop.clone(),
                price.clone(),
            )
        }
        "terminal" => {
            let ip = cfg
                .config
                .get("ip")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "terminal: не вказано ip".to_string())?
                .to_string();
            let tcp_port = cfg
                .config
                .get("tcpPort")
                .and_then(|v| v.as_u64())
                .unwrap_or(2024) as u16;
            spawn_terminal(
                app.clone(),
                cfg.id.clone(),
                ip,
                tcp_port,
                status.clone(),
                stop.clone(),
            )
        }
        "printer" => {
            let printer_name = cfg
                .config
                .get("printerName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "printer: не вказано printerName".to_string())?
                .to_string();
            spawn_printer_monitor(
                app.clone(),
                cfg.id.clone(),
                printer_name,
                status.clone(),
                stop.clone(),
            )
        }
        other => return Err(format!("невідомий тип пристрою: {other}")),
    };

    map.insert(
        cfg.id.clone(),
        ConnectionHandle {
            stop,
            thread: Some(handle),
            status: status.clone(),
            price: price.clone(),
        },
    );

    let current = status.lock().expect("lock poisoned: status").clone();
    Ok(current)
}

/// Зупинити фонове з'єднання (якщо активне) і прибрати з глобального стану.
fn stop_connection(id: &str) {
    let mut map = connections().lock().expect("lock poisoned: connections");
    if let Some(h) = map.remove(id) {
        h.stop.store(true, Ordering::Relaxed);
        if let Some(t) = h.thread {
            let _ = t.join();
        }
    }
}

/// Ініціалізація при старті: для кожного enabled-пристрою запускає фонове
/// підключення. Викликається з setup; помилки не падають — лог у stderr.
pub fn init_auto_connect(app: &AppHandle) {
    let devices = match load_devices(app) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("devices: не вдалося завантажити конфіги: {e}");
            return;
        }
    };
    for cfg in devices {
        if cfg.enabled {
            let app = app.clone();
            thread::spawn(move || {
                if let Err(e) = start_connection(&app, &cfg) {
                    eprintln!("devices: автопідключення '{}' не вдалося: {e}", cfg.name);
                }
            });
        }
    }
}

// ── Tauri команди ───────────────────────────────────────────────────────────

/// Список доступних COM-портів (serialport::available_ports).
#[tauri::command]
pub fn get_available_ports() -> Result<Vec<String>, String> {
    let ports = serialport::available_ports().map_err(|e| e.to_string())?;
    // /dev/ttyS* — вбудовані UART материнської плати, майже завжди порожні; фільтруємо як шум.
    Ok(ports
        .into_iter()
        .map(|p| p.port_name)
        .filter(|name| !name.starts_with("/dev/ttyS"))
        .collect())
}

/// Прочитати всі збережені конфіги пристроїв.
#[tauri::command]
pub fn get_devices(app: AppHandle) -> Result<Vec<DeviceConfig>, String> {
    load_devices(&app)
}

/// Зберегти конфіг пристрою. Якщо id порожній — генерує uuid v4.
/// Якщо enabled — автоматично підключає у фоновому потоці.
#[tauri::command]
pub fn save_device_config(
    app: AppHandle,
    mut config: DeviceConfig,
) -> Result<DeviceConfig, String> {
    if config.id.trim().is_empty() {
        config.id = Uuid::new_v4().to_string();
    }
    let mut devices = load_devices(&app)?;
    if let Some(existing) = devices.iter_mut().find(|d| d.id == config.id) {
        *existing = config.clone();
    } else {
        devices.push(config.clone());
    }
    save_devices(&app, &devices)?;
    if config.enabled {
        if let Err(e) = start_connection(&app, &config) {
            eprintln!("devices: автопідключення '{}' не вдалося: {e}", config.name);
        }
    }
    Ok(config)
}

/// Видалити конфіг пристрою і розірвати з'єднання.
#[tauri::command]
pub fn delete_device(app: AppHandle, id: String) -> Result<(), String> {
    stop_connection(&id);
    let mut devices = load_devices(&app)?;
    devices.retain(|d| d.id != id);
    save_devices(&app, &devices)
}

/// Запустити фонове підключення пристрою за id.
#[tauri::command]
pub fn connect_device(app: AppHandle, id: String) -> Result<DeviceStatus, String> {
    let devices = load_devices(&app)?;
    let cfg = devices
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| format!("пристрій {id} не знайдено"))?
        .clone();
    start_connection(&app, &cfg)
}

/// Розірвати з'єднання пристрою.
#[tauri::command]
pub fn disconnect_device(id: String) -> Result<DeviceStatus, String> {
    let mut map = connections().lock().expect("lock poisoned: connections");
    if let Some(h) = map.remove(&id) {
        h.stop.store(true, Ordering::Relaxed);
        if let Some(t) = h.thread {
            let _ = t.join();
        }
        Ok(h.status.lock().expect("lock poisoned: status").clone())
    } else {
        Ok(DeviceStatus {
            id,
            status: "disconnected".to_string(),
            error: None,
            last_weight: None,
        })
    }
}

/// Встановити ціну для ваг (протокол 5, Режим 2). Ваги тримають ціну і
/// рахують вартість = маса × ціна. `None` — повернутись до Режиму 3
/// (запит без ціни; увага: Режим 3 передає у ваги нульову ціну).
#[tauri::command]
pub fn set_scale_price(device_id: String, price: Option<f64>) -> Result<(), String> {
    let map = connections().lock().expect("lock poisoned: connections");
    let h = map
        .get(&device_id)
        .ok_or_else(|| format!("пристрій {device_id} не підключено"))?;
    *h.price.lock().expect("lock poisoned: price") = price;
    crate::embedded_pg::pg_log(
        "INFO",
        &format!("scale {device_id}: ціну встановлено {:?}", price),
    );
    Ok(())
}

/// Поточні статуси всіх збережених пристроїв.
#[tauri::command]
pub fn get_devices_status(app: AppHandle) -> Result<Vec<DeviceStatus>, String> {
    let devices = load_devices(&app)?;
    let map = connections().lock().expect("lock poisoned: connections");
    let mut result = Vec::with_capacity(devices.len());
    for cfg in &devices {
        if let Some(h) = map.get(&cfg.id) {
            result.push(h.status.lock().expect("lock poisoned: status").clone());
        } else {
            result.push(DeviceStatus {
                id: cfg.id.clone(),
                status: "disconnected".to_string(),
                error: None,
                last_weight: None,
            });
        }
    }
    Ok(result)
}

/// Список системних CUPS-принтерів (парсинг `lpstat -p -d`).
/// Якщо lpstat відсутній або помилка — порожній список (Linux без CUPS не падає).
#[tauri::command]
pub fn get_system_printers() -> Result<Vec<PrinterInfo>, String> {
    let output = match std::process::Command::new("lpstat")
        .args(["-p", "-d"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return Ok(Vec::new()),
    };
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut printers: Vec<PrinterInfo> = Vec::new();
    let mut default_name: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("printer ") {
            // "printer NAME is STATUS. enabled..." → PrinterInfo
            if let Some(idx) = rest.find(" is ") {
                let name = rest[..idx].trim().to_string();
                let status_part = &rest[idx + 4..];
                let status = if status_part.starts_with("idle") {
                    "idle".to_string()
                } else if status_part.starts_with("printing") || status_part.starts_with("busy") {
                    "printing".to_string()
                } else if status_part.starts_with("disabled") {
                    "disabled".to_string()
                } else {
                    "error".to_string()
                };
                printers.push(PrinterInfo {
                    name,
                    status,
                    is_default: false,
                });
            }
        } else if let Some(rest) = line.strip_prefix("system default destination:") {
            default_name = Some(rest.trim().to_string());
        }
    }

    if let Some(def) = default_name {
        for p in printers.iter_mut() {
            if p.name == def {
                p.is_default = true;
            }
        }
    }
    Ok(printers)
}

/// Виявлення сканерів через SANE (`scanimage -L`).
/// Якщо scanimage відсутній або сканерів немає — порожній список (НЕ помилка).
#[tauri::command]
pub fn get_scanners() -> Result<Vec<ScannerInfo>, String> {
    let output = match std::process::Command::new("scanimage").arg("-L").output() {
        Ok(o) => o,
        Err(_) => return Ok(Vec::new()),
    };
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if text.contains("No scanners were identified") {
        return Ok(Vec::new());
    }

    let mut scanners = Vec::new();
    for line in text.lines() {
        // Формат SANE:  device `DEVICE_ID' is a DESCRIPTION
        // відкриваюча лапка — зворотній апостроф `, закриваюча — звичайний '
        let Some(open) = line.find('`') else { continue };
        let Some(close_rel) = line[open + 1..].find('\'') else {
            continue;
        };
        let close = open + 1 + close_rel;
        let device = line[open + 1..close].trim().to_string();

        // після закриваючої лапки йде " is a DESCRIPTION"
        let rest = &line[close..];
        let Some(is_a) = rest.find(" is a ") else {
            continue;
        };
        let name = rest[is_a + " is a ".len()..].trim().to_string();

        if !device.is_empty() && !name.is_empty() {
            scanners.push(ScannerInfo { device, name });
        }
    }
    Ok(scanners)
}

/// Перелік USB-пристроїв (для налагодження та автовиявлення).
/// Перенесено з commands/system.rs::get_usb_devices (етап 0, без зміни поведінки).
pub fn list_usb_devices() -> Vec<serde_json::Value> {
    let mut devices: Vec<serde_json::Value> = Vec::new();

    #[cfg(unix)]
    {
        // Читаємо /sys/bus/usb/devices/
        if let Ok(entries) = std::fs::read_dir("/sys/bus/usb/devices/") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Пропускаємо інтерфейси (usb1, usb2, ...)
                if name.starts_with("usb") || !name.contains('-') || name == "devices" {
                    continue;
                }

                let uevent_path = entry.path().join("uevent");
                if let Ok(content) = std::fs::read_to_string(&uevent_path) {
                    let mut product = String::new();
                    let mut vendor = String::new();

                    for line in content.lines() {
                        if let Some(val) = line.strip_prefix("PRODUCT=") {
                            product = val.to_string();
                        }
                        if let Some(val) = line.strip_prefix("DEVICE=") {
                            vendor = val.to_string();
                        }
                    }

                    devices.push(serde_json::json!({
                        "name": name,
                        "product": product,
                        "device": vendor,
                    }));
                }
            }
        }
    }

    devices
}

/// Автовиявлення всіх підключених пристроїв:
/// CUPS-принтери + COM-порти (serialport) + USB-пристрої (system::get_usb_devices).
#[tauri::command]
pub fn get_detected_devices() -> Result<DetectedDevices, String> {
    let printers = get_system_printers()?;
    let serial_ports = serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default();
    let usb_raw = list_usb_devices();
    let usb_devices = usb_raw
        .into_iter()
        .filter_map(|v| serde_json::from_value::<UsbDevice>(v).ok())
        .collect();
    Ok(DetectedDevices {
        printers,
        serial_ports,
        usb_devices,
        scanners: get_scanners().unwrap_or_default(),
    })
}

/// Перевірка з'єднання без збереження конфіга.
/// - "terminal": TCP-підключення до ip:tcpPort (~2с), потім закрити.
/// - "scale": спроба відкрити COM-порт на baudRate з конфіга (9600 за замовчуванням).
#[tauri::command]
pub fn test_connection(device_type: String, config: serde_json::Value) -> Result<bool, String> {
    match device_type.as_str() {
        "terminal" => {
            let ip = config
                .get("ip")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "terminal: не вказано ip".to_string())?;
            let tcp_port = config
                .get("tcpPort")
                .and_then(|v| v.as_u64())
                .unwrap_or(2024) as u16;
            let addr = format!("{ip}:{tcp_port}")
                .parse::<std::net::SocketAddr>()
                .map_err(|e| format!("некоректна адреса {ip}:{tcp_port}: {e}"))?;
            let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
                .map_err(|e| e.to_string())?;
            drop(stream);
            Ok(true)
        }
        "scale" => {
            let port = config
                .get("port")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "scale: не вказано port".to_string())?;
            let baud_rate = config
                .get("baudRate")
                .and_then(|v| v.as_u64())
                .unwrap_or(9600) as u32;
            let protocol = config
                .get("protocol")
                .and_then(|v| v.as_str())
                .unwrap_or("cas")
                .to_string();
            // Протокол 5 (ВТА-60, BCD) — парність EVEN (8E1)
            let mut builder = serialport::new(port, baud_rate);
            if protocol == "vta5" {
                builder = builder.parity(serialport::Parity::Even);
            }
            let _serial = builder
                .open()
                .map_err(|e| format!("відкриття {port}: {e}"))?;
            Ok(true)
        }
        "printer" => {
            let printer_name = config
                .get("printerName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "printer: не вказано printerName".to_string())?;
            let printers = get_system_printers().unwrap_or_default();
            Ok(printers.iter().any(|p| p.name == printer_name))
        }
        other => Err(format!("невідомий тип пристрою: {other}")),
    }
}

/// Знайти підключений термінал та його адресу (IP:port) з конфігурації каси
fn find_terminal(app: &AppHandle) -> Result<(String, String, u16), String> {
    let devices = load_devices(app)?;
    let terminal = devices
        .iter()
        .find(|d| d.device_type == "terminal")
        .ok_or_else(|| {
            "Термінал не додано. Додайте термінал у Налаштуваннях → Підключені пристрої".to_string()
        })?;
    let ip = terminal
        .config
        .get("ip")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Термінал «{}»: не вказано IP-адресу", terminal.name))?;
    let tcp_port = terminal
        .config
        .get("tcpPort")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| format!("Термінал «{}»: не вказано порт", terminal.name))?
        as u16;
    Ok((terminal.name.clone(), ip.to_string(), tcp_port))
}

/// Серіалізація операцій з терміналом: термінал приймає 1 операцію за раз,
/// несервісний запит під час операції → deviceBusy. Також цей lock бачать
/// spawn_terminal (пропускає перевірку під час операції) та всі команди.
static TERMINAL_OP_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn terminal_op_guard() -> std::sync::MutexGuard<'static, ()> {
    TERMINAL_OP_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Оплата карткою: передати суму на термінал ПриватБанку (метод Purchase).
/// Викликається з каси при виборі способу оплати «Картка».
#[tauri::command]
pub fn terminal_payment(
    app: AppHandle,
    amount: f64,
) -> Result<terminal::TerminalPaymentResult, String> {
    let (name, ip, port) = find_terminal(&app)?;
    // Монопольність: чекаємо завершення попередньої операції
    let _guard = terminal_op_guard();
    terminal::purchase(&ip, port, &name, amount)
}

/// Повернення коштів на картку (метод Refund). rrn — RRN оригінальної
/// транзакції, яку повертаємо.
#[tauri::command]
pub fn terminal_refund(
    app: AppHandle,
    amount: f64,
    rrn: String,
) -> Result<terminal::TerminalPaymentResult, String> {
    let (name, ip, port) = find_terminal(&app)?;
    let _guard = terminal_op_guard();
    terminal::refund(&ip, port, &name, amount, &rrn)
}

/// Скасування транзакції в межах поточного пакета (метод Withdrawal).
/// invoice_number — номер чека оригінальної транзакції.
#[tauri::command]
pub fn terminal_cancel(
    app: AppHandle,
    invoice_number: String,
) -> Result<terminal::TerminalPaymentResult, String> {
    let (name, ip, port) = find_terminal(&app)?;
    let _guard = terminal_op_guard();
    terminal::withdrawal(&ip, port, &name, &invoice_number)
}

/// Перевірка зв'язку з терміналом (хендшейк PingDevice + Identify).
#[tauri::command]
pub fn terminal_ping(app: AppHandle) -> Result<terminal::TerminalPingResult, String> {
    let (_, ip, port) = find_terminal(&app)?;
    let _guard = terminal_op_guard();
    terminal::ping(&ip, port)
}

// ── Юніт-тести ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_weight: CAS-подібний формат ─────────────────────────────────

    #[test]
    fn cas_spaced_digits() {
        // "1 2 3 . 4 5" = 123.45 кг
        assert_eq!(parse_weight("1 2 3 . 4 5"), Some(123.45));
    }

    #[test]
    fn cas_with_suffix() {
        assert_eq!(parse_weight("12.34kg"), Some(12.34));
        assert_eq!(parse_weight("  1 2 . 3 4  kg  "), Some(12.34));
    }

    #[test]
    fn comma_decimal_separator() {
        assert_eq!(parse_weight("12,5"), Some(12.5));
        assert_eq!(parse_weight("1 2 , 5"), Some(12.5));
    }

    #[test]
    fn backspace_erases_previous_char() {
        // "12" + backspace (стирає '2') + "4.5" → "14.5"
        assert_eq!(parse_weight("12\x084.5"), Some(14.5));
        // CAS: backspace після кількох цифр
        assert_eq!(parse_weight("123\x084.5"), Some(124.5));
        // backspace на порожньому буфері — безпечно
        assert_eq!(parse_weight("\x0812.34"), Some(12.34));
    }

    #[test]
    fn service_chars_ignored() {
        assert_eq!(parse_weight("\x0212.34\x03"), Some(12.34));
        assert_eq!(parse_weight("12.34\r\n"), Some(12.34));
        assert_eq!(parse_weight("\x021 2 . 3 4\x03\r\n"), Some(12.34));
    }

    #[test]
    fn noise_around_value() {
        assert_eq!(parse_weight("prefix 45.6kg suffix"), Some(45.6));
    }

    #[test]
    fn range_kept() {
        assert_eq!(parse_weight("0.001"), Some(0.001));
        assert_eq!(parse_weight("10000.0"), Some(10000.0));
        assert_eq!(parse_weight("0.0005"), None);
        assert_eq!(parse_weight("10000.1"), None);
    }

    #[test]
    fn integer_without_separator_rejected() {
        // без крапки/коми — не число ваги
        assert_eq!(parse_weight("12345"), None);
    }

    // ── should_emit_weight: фільтр стабільності ───────────────────────────

    #[test]
    fn emits_only_after_two_readings() {
        let mut last = None;
        let mut pending = None;
        // перше читання — лише кандидат, не емітимо
        assert!(!should_emit_weight(&mut last, &mut pending, 100.0));
        // друге в межах tolerance — підтверджено, емітимо
        assert!(should_emit_weight(&mut last, &mut pending, 100.001));
        assert_eq!(last, Some(100.001));
    }

    #[test]
    fn small_noise_not_emitted() {
        // підтверджене значення, але зміна ≤ 0.002 кг — шум, не емітимо
        let mut last = Some(100.0);
        let mut pending = None;
        assert!(!should_emit_weight(&mut last, &mut pending, 100.0));
        assert!(!should_emit_weight(&mut last, &mut pending, 100.001));
        assert_eq!(last, Some(100.0)); // last не змінився

        // дрейф у межах tolerance двома читаннями
        let mut last = Some(100.0);
        let mut pending = None;
        assert!(!should_emit_weight(&mut last, &mut pending, 100.002));
        assert!(!should_emit_weight(&mut last, &mut pending, 100.001));
        assert_eq!(last, Some(100.0));
    }

    #[test]
    fn real_change_emitted() {
        let mut last = Some(100.0);
        let mut pending = None;
        assert!(!should_emit_weight(&mut last, &mut pending, 500.0));
        assert!(should_emit_weight(&mut last, &mut pending, 500.001));
        assert_eq!(last, Some(500.001));
    }

    #[test]
    fn unstable_reading_never_emitted() {
        // вага "гойдається" — жодне значення не підтверджується двома читаннями
        let mut last = None;
        let mut pending = None;
        assert!(!should_emit_weight(&mut last, &mut pending, 100.0));
        assert!(!should_emit_weight(&mut last, &mut pending, 500.0));
        assert!(!should_emit_weight(&mut last, &mut pending, 100.0));
        assert!(!should_emit_weight(&mut last, &mut pending, 500.0));
        assert_eq!(last, None);
    }

    // ── ВТА-60 (KLARUS): протокол 2 ──────────────────────────────────────

    /// Побудова кадру протоколу 2 з коректним BCC.
    fn vta2_frame(
        sta: u8,
        sign: u8,
        digits: &[u8; 6],
        un1: u8,
        un2: u8,
        bcc: Option<u8>,
    ) -> [u8; 15] {
        let mut f = [0u8; 15];
        f[0] = 0x01; // SOH
        f[1] = 0x02; // STX
        f[2] = sta;
        f[3] = sign;
        f[4..10].copy_from_slice(digits);
        f[10] = un1;
        f[11] = un2;
        // BCC за замовчуванням — XOR від STX до UN2 включно
        let bcc_val = match bcc {
            Some(v) => v,
            None => f[1..12].iter().fold(0u8, |a, b| a ^ b),
        };
        f[12] = bcc_val;
        f[13] = 0x03; // ETX
        f[14] = 0x04; // EOT
        f
    }

    #[test]
    fn vta2_valid_frame_parses() {
        // SOH STX 'S' ' ' '0''0''1''5''0''0' 'k''g' BCC ETX EOT → 1.500 кг
        let f = vta2_frame(b'S', b' ', b"001500", b'k', b'g', None);
        assert_eq!(parse_vta2_frame(&f), Some(1.500));
    }

    #[test]
    fn vta2_decimal_dot_format_real_scale() {
        // Реальний кадр ВАГ-60: SOH STX 'S' ' ' " 0.000" 'k' 'g' BCC ETX EOT
        // BCC = XOR від f[2] до f[11] включно = 0x71 (перевірено розрахунком)
        let f = vta2_frame(b'S', b' ', b" 0.000", b'k', b'g', Some(0x71));
        assert_eq!(parse_vta2_frame(&f), Some(0.0));
    }

    #[test]
    fn vta2_decimal_dot_format_weight() {
        // " 5.123" кг → 5.123; BCC (XOR f[2..12]) = 0x74
        let f = vta2_frame(b'S', b' ', b" 5.123", b'k', b'g', Some(0x74));
        assert_eq!(parse_vta2_frame(&f), Some(5.123));
    }

    #[test]
    fn vta2_decimal_comma_format() {
        // кома замість крапки: " 5,123" → 5.123; BCC (XOR f[2..12]) = 0x74
        let f = vta2_frame(b'S', b' ', b" 5,123", b'k', b'g', Some(0x74));
        assert_eq!(parse_vta2_frame(&f), Some(5.123));
    }

    #[test]
    fn vta2_decimal_format_grams_divided() {
        // без 'k' в одиницях (грами) і крапка: " 5123" → грами, /1000 → 5.123
        let mut f = vta2_frame(b'S', b' ', b" 5.123", b'g', b'r', None);
        // BCC перераховуємо під нові одиниці: XOR від STX до UN2 включно
        f[12] = f[1..12].iter().fold(0u8, |a, b| a ^ b);
        assert_eq!(parse_vta2_frame(&f), Some(5.123 / 1000.0));
    }

    #[test]
    fn vta2_old_format_still_works() {
        // Старий формат без крапки: "000000" кг → 0.0; BCC (XOR f[2..12]) = 0x7F
        let f = vta2_frame(b'S', b' ', b"000000", b'k', b'g', Some(0x7F));
        assert_eq!(parse_vta2_frame(&f), Some(0.0));
        // і значуща вага "001500" кг → 1.500
        let f2 = vta2_frame(b'S', b' ', b"001500", b'k', b'g', None);
        assert_eq!(parse_vta2_frame(&f2), Some(1.500));
    }

    #[test]
    fn vta2_unstable_frame_ignored() {
        // 'U' (нестабільна) — приймати ТІЛЬКИ 'S'
        let f = vta2_frame(b'U', b' ', b"001500", b'k', b'g', None);
        assert_eq!(parse_vta2_frame(&f), None);
    }

    #[test]
    fn vta2_wrong_bcc_rejected() {
        let mut f = vta2_frame(b'S', b' ', b"001500", b'k', b'g', None);
        f[12] ^= 0xFF; // псуємо BCC
        assert_eq!(parse_vta2_frame(&f), None);
    }

    #[test]
    fn vta2_negative_rejected() {
        // SIGN '-' — від'ємна маса, відкидаємо
        let f = vta2_frame(b'S', b'-', b"001500", b'k', b'g', None);
        assert_eq!(parse_vta2_frame(&f), None);
    }

    #[test]
    fn vta2_bcc_both_variants_accepted() {
        // варіант 1: XOR від STX(0x02) до UN2 включно
        let f1 = vta2_frame(b'S', b' ', b"001500", b'k', b'g', None);
        assert_eq!(parse_vta2_frame(&f1), Some(1.500));
        // варіант 2: XOR від STA до UN2 включно — теж приймаємо (толерантно)
        let bcc_sta = f1[2..12].iter().fold(0u8, |a, b| a ^ b);
        let f2 = vta2_frame(b'S', b' ', b"001500", b'k', b'g', Some(bcc_sta));
        assert_eq!(parse_vta2_frame(&f2), Some(1.500));
        // обидва варіанти не збіглися → відкинути
        let f3 = vta2_frame(b'S', b' ', b"001500", b'k', b'g', Some(0x00));
        assert_eq!(parse_vta2_frame(&f3), None);
    }

    #[test]
    fn vta2_grams_without_k() {
        // без 'k' в одиницях — грами: raw = 1500
        let f = vta2_frame(b'S', b' ', b"001500", b'g', b'r', None);
        assert_eq!(parse_vta2_frame(&f), Some(1500.0));
    }

    #[test]
    fn vta2_stream_finds_frame_in_noise() {
        let mut noise: Vec<u8> = vec![0x00, 0x11, 0xAA];
        let f = vta2_frame(b'S', b' ', b"001500", b'k', b'g', None);
        noise.extend_from_slice(&f);
        noise.extend_from_slice(&[0x00, 0xFF]);
        assert_eq!(parse_vta2_stream(&noise).map(|(w, _)| w), Some(1.500));
    }

    // ── ВТА-60 (KLARUS): протокол 3 ──────────────────────────────────────

    #[test]
    fn vta3_three_lines_parses_mass() {
        // рядок 1 = маса "  1.500kg", рядок 2 = ціна, рядок 3 = вартість
        let mut stream = Vec::new();
        stream.extend_from_slice(b"  1.500kg\r\n");
        stream.extend_from_slice(b"   250.00\r\n");
        stream.extend_from_slice(b"   375.00\r\n");
        let (w, consumed) = parse_vta3_stream(&stream).expect("маса має знайтись");
        assert!((w - 1.500).abs() < 1e-9);
        assert_eq!(consumed, 11);
    }

    #[test]
    fn vta3_line_without_dot_ignored() {
        // рядок без крапки в перших 7 символах ігнорується; далі — маса
        let mut stream = Vec::new();
        stream.extend_from_slice(b"123456789\r\n"); // без крапки
        stream.extend_from_slice(b"  1.500kg\r\n");
        let (w, _) = parse_vta3_stream(&stream).expect("маса має знайтись");
        assert!((w - 1.500).abs() < 1e-9);
    }

    #[test]
    fn vta3_partial_line_accumulates() {
        // рядок приходить частинами — парсер чекає повні 9 символів + CR LF
        let mut stream = Vec::new();
        stream.extend_from_slice(b"  1.50");
        assert_eq!(parse_vta3_stream(&stream), None);
        stream.extend_from_slice(b"0kg\r\n");
        let (w, _) = parse_vta3_stream(&stream).expect("маса має знайтись");
        assert!((w - 1.500).abs() < 1e-9);
    }

    // ── ВТА-60 (KLARUS): протокол 5 ──────────────────────────────────────

    #[test]
    fn vta5_bcd_frame_parses() {
        // М0..М5 = 0,0,5,1,0,0 → цифри у зворотному порядку (М5..М0) = 0 0 1 5 0 0 → 1.500 кг
        let mut frame = [0u8; 17];
        frame[0] = 0x00; // М0 (молодший розряд)
        frame[1] = 0x00;
        frame[2] = 0x05;
        frame[3] = 0x01;
        frame[4] = 0x00;
        frame[5] = 0x00; // М5 (старший розряд)
                         // Ц0..Ц4 та С0..С5 — нулі (запит без ціни)
        assert_eq!(parse_vta5_frame(&frame), Some(1.500));
    }

    #[test]
    fn vta5_bcd_least_significant_first() {
        // М0=5 → п'ять тисячних кг: 0.005 кг
        let mut frame = [0u8; 17];
        frame[0] = 0x05;
        assert_eq!(parse_vta5_frame(&frame), Some(0.005));
    }

    #[test]
    fn vta5_non_bcd_byte_rejected() {
        let mut frame = [0u8; 17];
        frame[0] = 0x0A; // старша тетрада != 0 — не BCD
        assert_eq!(parse_vta5_frame(&frame), None);
    }
}
