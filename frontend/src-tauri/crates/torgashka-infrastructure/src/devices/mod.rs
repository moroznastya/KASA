// ─────────────────────────────────────────────────────────────────────────────
// Torgashka — Tauri Команди «Підключені пристрої»
// ─────────────────────────────────────────────────────────────────────────────
//
// Керування POS-обладнанням:
//   - COM-ваги (serialport, CAS-подібний протокол): фоновий потік читає порт,
//     парсить вагу (число з плаваючою крапкою) та надсилає подію
//     "weight-updated".
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
use std::io::Read;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
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
    /// scale: { "port": "/dev/ttyUSB0", "baudRate": 9600 }
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

// ── Фонові потоки ───────────────────────────────────────────────────────────

/// Драйвер COM-ваги: відкриває порт, циклічно читає, парсить вагу та емітить
/// подію "weight-updated". Статус оновлюється через "device-status-changed".
fn spawn_scale(
    app: AppHandle,
    id: String,
    port: String,
    baud_rate: u32,
    status: Arc<Mutex<DeviceStatus>>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut serial = match serialport::new(&port, baud_rate)
            .timeout(Duration::from_millis(500))
            .open()
        {
            Ok(p) => p,
            Err(e) => {
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

        let mut buf = [0u8; 256];
        let mut acc = String::new();
        // фільтр стабільності: останнє відправлене значення + кандидат на підтвердження
        let mut last_emitted: Option<f64> = None;
        let mut pending: Option<f64> = None;
        while !stop.load(Ordering::Relaxed) {
            match serial.read(&mut buf) {
                Ok(n) if n > 0 => {
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
                        if let Ok(mut st) = status.lock() {
                            st.last_weight = Some(weight);
                        }
                        if should_emit_weight(&mut last_emitted, &mut pending, weight) {
                            let _ = app.emit(
                                "weight-updated",
                                json!({ "deviceId": id.clone(), "value": weight }),
                            );
                        }
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    // таймаут читання — просто продовжуємо цикл
                }
                Err(e) => {
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
        set_status(&status, "disconnected", None, None);
        emit_status(&app, &id, &status);
    })
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
    const SO_LINGER: libc::c_int = 0x0080;  // Winsock2 winsock.h
    let linger = Linger { l_onoff: 1, l_linger: 0 };
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
            spawn_scale(
                app.clone(),
                cfg.id.clone(),
                port,
                baud_rate,
                status.clone(),
                stop.clone(),
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
/// - "scale": спроба відкрити COM-порт на 9600 бод.
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
            let _serial = serialport::new(port, 9600)
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
}
