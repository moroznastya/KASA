// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Керування грошовою скринькою (Cash Drawer)
// ─────────────────────────────────────────────────────────────────────────────
//
// Підтримувані методи:
//   1. ESC/POS команда (0x1B, 0x70, n, t) — через термопринтер
//   2. COM-порт (RS-232) — пряме підключення до скриньки
//
// Для більшості POS-систем достатньо ESC/POS команди,
// оскільки скринька під'єднана до принтера (RJ-11 кабель).
// ─────────────────────────────────────────────────────────────────────────────

use std::io::{self, Write};
use std::path::Path;

/// Помилки грошової скриньки
#[derive(Debug)]
pub enum CashDrawerError {
    /// Помилка введення/виведення
    Io(io::Error),
    /// Порт не знайдено
    PortNotFound,
    /// Непідтримуваний метод відкриття
    UnsupportedMethod,
    /// Загальна помилка
    General(String),
}

impl std::fmt::Display for CashDrawerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CashDrawerError::Io(e) => write!(f, "Помилка введення/виведення: {}", e),
            CashDrawerError::PortNotFound => write!(f, "Порт для грошової скриньки не знайдено"),
            CashDrawerError::UnsupportedMethod => write!(f, "Непідтримуваний метод відкриття скриньки"),
            CashDrawerError::General(msg) => write!(f, "Помилка скриньки: {}", msg),
        }
    }
}

impl From<io::Error> for CashDrawerError {
    fn from(e: io::Error) -> Self {
        CashDrawerError::Io(e)
    }
}

// ── ESC/POS команда для відкриття скриньки ──────────────────────────────

/// ESC/POS команда для відкриття грошової скриньки через принтер.
///
/// Специфікація: ESC p n t
///   ESC = 0x1B
///   p   = 0x70
///   n   = 0 (drawer 1), 1 (drawer 2)
///   t   = час імпульсу (зазвичай 0x32 = 50ms, 0x19 = 25ms)
///
/// Для більшості скриньок: `[0x1B, 0x70, 0x00, 0x32]`
/// Відкриває першу скриньку з імпульсом 50ms.
pub fn escpos_open_command(drawer: u8, pulse_time: u8) -> Vec<u8> {
    vec![0x1B, 0x70, drawer.min(1), pulse_time]
}

/// Відкрити скриньку через ESC/POS принтер (надсилає команду на принтер)
pub fn open_via_printer(device_path: &str) -> Result<(), CashDrawerError> {
    let path = Path::new(device_path);

    if !path.exists() {
        return Err(CashDrawerError::PortNotFound);
    }

    let command = escpos_open_command(0, 0x32); // drawer 1, 50ms

    // Пробуємо відкрити як файл (Linux: /dev/usb/lp*, /dev/ttyUSB*)
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)?;

    file.write_all(&command)?;
    file.flush()?;

    Ok(())
}

/// Відкрити скриньку через COM-порт (RS-232)
///
/// На Linux: `/dev/ttyS0`, `/dev/ttyUSB0`
/// На Windows: `COM1`, `COM2`
#[cfg(unix)]
pub fn open_via_com_port(port: &str) -> Result<(), CashDrawerError> {
    let path = Path::new(port);
    if !path.exists() {
        return Err(CashDrawerError::PortNotFound);
    }

    // Відкриваємо COM-порт
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .read(false)
        .open(path)?;

    let command = escpos_open_command(0, 0x32);
    file.write_all(&command)?;
    file.flush()?;

    Ok(())
}

/// Спробувати автоматично визначити скриньку та відкрити її
pub fn open_cash_drawer(device_path: Option<&str>) -> Result<(), CashDrawerError> {
    if let Some(path) = device_path {
        if !path.is_empty() {
            return open_via_printer(path);
        }
    }

    // Автоматичний пошук: спочатку /dev/usb/lp*, потім /dev/ttyUSB*
    #[cfg(unix)]
    {
        // Шукаємо USB принтер
        for i in 0..4 {
            let path = format!("/dev/usb/lp{}", i);
            if Path::new(&path).exists() {
                if let Ok(()) = open_via_printer(&path) {
                    return Ok(());
                }
            }
        }

        // Шукаємо USB serial
        for i in 0..4 {
            let path = format!("/dev/ttyUSB{}", i);
            if Path::new(&path).exists() {
                if let Ok(()) = open_via_com_port(&path) {
                    return Ok(());
                }
            }
        }
    }

    Err(CashDrawerError::PortNotFound)
}
