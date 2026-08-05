// ─────────────────────────────────────────────────────────────────────────────
// Kasa POS — Клієнт протоколу ПриватБанк ECR (JSON) для карткового термінала
// ─────────────────────────────────────────────────────────────────────────────
//
// Транспорт: TCP. Усі повідомлення — JSON, завершуються NULL-термінатором 0x00.
// Еталонна схема роботи (протокол, розділ 2):
//   1. connect на IP:port
//   2. хендшейк: 0x00 + {"method":"PingDevice","step":0} + 0x00 → відповідь
//   3. пауза ~1с після хендшейку (для Verifone 3–5с)
//   4. Identify (ServiceMessage identify) — для terminal_ping: vendor/model
//   5. основний метод (Purchase / Refund / Withdrawal) → фінальна відповідь
//   6. закриття з'єднання (RST — Newland N950 не відповідає на FIN)
//
// Таймаути: на з'єднання 10с; Purchase/Refund можуть тривати до 120с (PIN,
// картка); Withdrawal — до 60с. З'єднання не закриваємо одразу після запиту.
//
// Монопольність: термінал приймає 1 операцію за раз — серіалізація команд
// виконана на рівні devices.rs (глобальний Mutex TERMINAL_OP_LOCK).
// ─────────────────────────────────────────────────────────────────────────────

use serde::Serialize;
use serde_json::json;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

/// Результат фінансової операції термінала (Purchase / Refund / Withdrawal)
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TerminalPaymentResult {
    /// назва термінала з конфігурації каси
    pub terminal_name: String,
    /// сума операції (при Partial approval — часткова сума від термінала)
    pub amount: f64,
    /// true — операція успішна (RC 0000 або часткове схвалення 0010)
    pub success: bool,
    /// responseCode: "0000" успіх, "0010" Partial approval, "1000"/"1001" тощо
    pub response_code: Option<String>,
    /// опис помилки (заповнено при error:true або при 0010)
    pub error_description: Option<String>,
    pub rrn: Option<String>,
    pub approval_code: Option<String>,
    pub invoice_number: Option<String>,
    pub pan: Option<String>,
    /// текст чека (якщо каса друкує чеки — налаштування профілю термінала)
    pub receipt: Option<String>,
    /// дата транзакції (format: dd.MM.yyyy)
    pub transaction_date: Option<String>,
    /// час транзакції (format: HH:mm:ss)
    pub transaction_time: Option<String>,
    /// trnStatus: "1" = approved, "2" = declined, "3" = reversed, "4" = canceled
    pub trn_status: Option<String>,
}

/// Результат перевірки зв'язку з терміналом (хендшейк + Identify)
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TerminalPingResult {
    pub success: bool,
    pub response_code: Option<String>,
    pub error_description: Option<String>,
    /// вендор термінала (з Identify), напр. "PAX", "Newland"
    pub vendor: Option<String>,
    /// модель термінала, напр. "s800", "N950"
    pub model: Option<String>,
}

// ── Сесія: TCP-з'єднання + буфер дейтаграм ─────────────────────────────────

/// Сесія обміну з терміналом. Зберігає необроблені байти: дейтаграми можуть
/// приходити частинами (кількома TCP-пакетами) або кілька за один пакет.
struct PbSession {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl PbSession {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buf: Vec::new(),
        }
    }

    /// Надіслати JSON-повідомлення з NULL-термінатором (0x00)
    fn write(&mut self, payload: &serde_json::Value) -> Result<(), String> {
        let mut data = serde_json::to_vec(payload)
            .map_err(|e| format!("серіалізація JSON: {e}"))?;
        data.push(0x00);
        self.stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        self.stream
            .write_all(&data)
            .map_err(|e| format!("помилка надсилання терміналу: {e}"))
    }

    /// Прочитати одну JSON-дейтаграму (до 0x00).
    /// timeout — максимальний час очікування повної дейтаграми.
    fn read(&mut self, timeout: Duration) -> Result<serde_json::Value, String> {
        let deadline = Instant::now() + timeout;
        // Періодичні перевірки кожні 500мс — дає змогу контролювати deadline
        self.stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .map_err(|e| e.to_string())?;
        loop {
            // Спершу шукаємо повну дейтаграму в накопиченому буфері
            if let Some(pos) = self.buf.iter().position(|&b| b == 0x00) {
                let datagram: Vec<u8> = self.buf.drain(..=pos).collect();
                let json_str = String::from_utf8_lossy(&datagram[..datagram.len() - 1]);
                if json_str.trim().is_empty() {
                    continue; // порожня дейтаграма (напр., початковий 0x00) — пропускаємо
                }
                return serde_json::from_str(&json_str).map_err(|e| {
                    format!("некоректний JSON від термінала: {e}: {json_str}")
                });
            }
            if Instant::now() >= deadline {
                return Err("таймаут очікування відповіді термінала".to_string());
            }
            let mut chunk = [0u8; 1024];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err("термінал закрив з'єднання".to_string()),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e)
                    if e.kind() == ErrorKind::TimedOut
                        || e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => return Err(format!("помилка читання від термінала: {e}")),
            }
            if self.buf.len() > 256 * 1024 {
                return Err("переповнення буфера відповіді термінала".to_string());
            }
        }
    }
}

// ── Допоміжні функції ───────────────────────────────────────────────────────

/// Підключення до термінала: до 3 спроб (10с кожна) з паузою 1.5с —
/// Wi-Fi термінали з power-saving можуть «прокидатись» 5–15 секунд
fn connect(ip: &str, tcp_port: u16, terminal_name: &str) -> Result<TcpStream, String> {
    let addr = format!("{ip}:{tcp_port}");
    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|e| format!("Термінал «{terminal_name}»: некоректна адреса {addr}: {e}"))?;
    let mut last_err = String::new();
    for attempt in 1..=3 {
        match TcpStream::connect_timeout(&socket_addr, Duration::from_secs(10)) {
            Ok(s) => return Ok(s),
            Err(e) => {
                last_err = e.to_string();
                if attempt < 3 {
                    thread::sleep(Duration::from_millis(1500));
                }
            }
        }
    }
    Err(format!(
        "Термінал «{terminal_name}»: неможливо підключитись до {addr} після 3 спроб: {last_err}"
    ))
}

/// Примусове RST-закриття (SO_LINGER=0): термінал Newland N950 не відповідає
/// на FIN — без RST сокети залишаються у FIN-WAIT-2 та накопичуються
#[cfg(unix)]
fn rst_close(stream: &TcpStream) {
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

#[cfg(not(unix))]
fn rst_close(_stream: &TcpStream) {}

/// Хендшейк: 0x00 + {"method":"PingDevice","step":0} + 0x00, очікування
/// відповіді PingDevice з responseCode "0000"
fn handshake(session: &mut PbSession) -> Result<(), String> {
    let mut data = vec![0x00];
    data.extend_from_slice(br#"{"method":"PingDevice","step":0}"#);
    data.push(0x00);
    session
        .stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    session
        .stream
        .write_all(&data)
        .map_err(|e| format!("помилка надсилання хендшейку: {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("таймаут хендшейку (термінал не відповів на PingDevice)".to_string());
        }
        let v = session.read(remaining)?;
        if v.get("method").and_then(|m| m.as_str()) == Some("PingDevice") {
            let rc = v
                .pointer("/params/responseCode")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if rc != "0000" {
                return Err(format!("хендшейк: термінал відповів responseCode {rc}"));
            }
            return Ok(());
        }
        // Інші дейтаграми (напр., службові) — ігноруємо, чекаємо відповідь
    }
}

/// Identify: визначення вендора та моделі термінала
fn identify(session: &mut PbSession) -> Result<(Option<String>, Option<String>), String> {
    session.write(&json!({
        "method": "ServiceMessage",
        "step": 0,
        "params": { "msgType": "identify" }
    }))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("таймаут Identify (термінал не відповів)".to_string());
        }
        let v = session.read(remaining)?;
        if v.get("method").and_then(|m| m.as_str()) == Some("ServiceMessage")
            && v.pointer("/params/msgType").and_then(|m| m.as_str()) == Some("identify")
        {
            let vendor = v
                .pointer("/params/vendor")
                .and_then(|x| x.as_str())
                .map(String::from);
            let model = v
                .pointer("/params/model")
                .and_then(|x| x.as_str())
                .map(String::from);
            return Ok((vendor, model));
        }
    }
}

/// Виконати фінансову операцію: надіслати запит і дочекатись фінальної відповіді.
/// Службові повідомлення (status updates) ігноруємо; deviceBusy /
/// methodNotImplemented — завершуємо з помилкою.
fn run_financial(
    ip: &str,
    tcp_port: u16,
    terminal_name: &str,
    request: &serde_json::Value,
    expect_method: &str,
    read_timeout: Duration,
    fallback_amount: f64,
) -> Result<TerminalPaymentResult, String> {
    let mut session = PbSession::new(connect(ip, tcp_port, terminal_name)?);
    handshake(&mut session)?;
    // Пауза після хендшейку: рекомендовано ~1с (для Verifone 3–5с)
    thread::sleep(Duration::from_secs(1));
    session.write(request)?;

    let deadline = Instant::now() + read_timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!(
                "таймаут операції «{expect_method}» (термінал не відповів за {}с)",
                read_timeout.as_secs()
            ));
        }
        let v = session.read(remaining)?;
        match v.get("method").and_then(|m| m.as_str()) {
            Some(m) if m == expect_method => {
                rst_close(&session.stream);
                return Ok(parse_financial(&v, terminal_name, fallback_amount));
            }
            Some("ServiceMessage") => {
                let msg_type = v
                    .pointer("/params/msgType")
                    .and_then(|m| m.as_str())
                    .unwrap_or("");
                match msg_type {
                    "deviceBusy" => {
                        rst_close(&session.stream);
                        return Err(
                            "Термінал зайнятий іншою операцією (deviceBusy)".to_string()
                        );
                    }
                    "methodNotImplemented" => {
                        rst_close(&session.stream);
                        return Err(format!("Термінал не підтримує метод «{expect_method}»"));
                    }
                    _ => {} // статусні/службові повідомлення — ігноруємо, чекаємо фінал
                }
            }
            _ => {} // нерелевантні повідомлення — ігноруємо
        }
    }
}

/// Розбір фінальної відповіді фінансової операції.
/// Правила: при responseCode != "0000" термінал передає error:true,
/// виняток — RC=0010 (Partial approval) з error:false (успішна часткова оплата).
fn parse_financial(
    v: &serde_json::Value,
    terminal_name: &str,
    fallback_amount: f64,
) -> TerminalPaymentResult {
    let p = v.get("params").cloned().unwrap_or_default();
    let response_code = p
        .get("responseCode")
        .and_then(|c| c.as_str())
        .map(String::from);
    let error = v.get("error").and_then(|e| e.as_bool()).unwrap_or(false);
    let rc = response_code.as_deref().unwrap_or("");

    // Успіх: error:false та RC 0000 або 0010 (часткове схвалення — успішна оплата)
    let success = !error && (rc == "0000" || rc == "0010");

    let error_description = if error {
        let d = v
            .get("errorDescription")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        Some(if d.is_empty() {
            format!("Помилка термінала: responseCode {rc}")
        } else {
            d
        })
    } else if rc == "0010" {
        Some("Часткове схвалення (Partial approval) — сума змінена терміналом".to_string())
    } else {
        None
    };

    // Сума з відповіді (при Partial approval — часткова сума), інакше передана
    let amount = p
        .get("amount")
        .and_then(|a| a.as_str())
        .and_then(|s| s.replace(',', ".").parse::<f64>().ok())
        .unwrap_or(fallback_amount);

    let s = |k: &str| p.get(k).and_then(|x| x.as_str()).map(String::from);

    TerminalPaymentResult {
        terminal_name: terminal_name.to_string(),
        amount,
        success,
        response_code,
        error_description,
        rrn: s("rrn"),
        approval_code: s("approvalCode"),
        invoice_number: s("invoiceNumber"),
        pan: s("pan"),
        receipt: s("receipt"),
        transaction_date: s("date"),
        transaction_time: s("time"),
        trn_status: s("trnStatus"),
    }
}

// ── Публічні операції ────────────────────────────────────────────────────────

/// Оплата карткою (метод Purchase). amount — сума в гривнях, форматується
/// рядком з 2 знаками після коми ("0" -> "0.00").
pub fn purchase(
    ip: &str,
    tcp_port: u16,
    terminal_name: &str,
    amount: f64,
) -> Result<TerminalPaymentResult, String> {
    let request = json!({
        "method": "Purchase",
        "step": 0,
        "params": {
            "amount": format!("{:.2}", amount),
            "discount": "",
            "merchantId": "0",
            "facepay": "false",
            "subMerchant": ""
        }
    });
    run_financial(
        ip,
        tcp_port,
        terminal_name,
        &request,
        "Purchase",
        // Клієнт може вводити PIN/картку до 2 хвилин
        Duration::from_secs(120),
        amount,
    )
}

/// Повернення коштів на картку (метод Refund). rrn — RRN оригінальної
/// транзакції (обов'язковий).
pub fn refund(
    ip: &str,
    tcp_port: u16,
    terminal_name: &str,
    amount: f64,
    rrn: &str,
) -> Result<TerminalPaymentResult, String> {
    let request = json!({
        "method": "Refund",
        "step": 0,
        "params": {
            "amount": format!("{:.2}", amount),
            "discount": "",
            "merchantId": "0",
            "rrn": rrn,
            "subMerchant": ""
        }
    });
    run_financial(
        ip,
        tcp_port,
        terminal_name,
        &request,
        "Refund",
        Duration::from_secs(120),
        amount,
    )
}

/// Скасування транзакції в межах поточного пакета (метод Withdrawal).
/// invoice_number — номер чека оригінальної транзакції.
pub fn withdrawal(
    ip: &str,
    tcp_port: u16,
    terminal_name: &str,
    invoice_number: &str,
) -> Result<TerminalPaymentResult, String> {
    let request = json!({
        "method": "Withdrawal",
        "step": 0,
        "params": { "invoiceNumber": invoice_number }
    });
    run_financial(
        ip,
        tcp_port,
        terminal_name,
        &request,
        "Withdrawal",
        Duration::from_secs(60),
        0.0,
    )
}

/// Перевірка зв'язку: хендшейк + Identify. Повертає Ok(...) навіть при
/// помилці зв'язку (success:false) — фронтенд читає поле success.
pub fn ping(ip: &str, tcp_port: u16) -> Result<TerminalPingResult, String> {
    let terminal_name = "Термінал";
    let mut session = PbSession::new(connect(ip, tcp_port, terminal_name)?);
    if let Err(e) = handshake(&mut session) {
        rst_close(&session.stream);
        return Ok(TerminalPingResult {
            success: false,
            response_code: None,
            error_description: Some(e),
            vendor: None,
            model: None,
        });
    }
    // Пауза після хендшейку: рекомендовано ~1с
    thread::sleep(Duration::from_secs(1));
    let id_result = identify(&mut session);
    rst_close(&session.stream);
    match id_result {
        Ok((vendor, model)) => Ok(TerminalPingResult {
            success: true,
            response_code: Some("0000".to_string()),
            error_description: None,
            vendor,
            model,
        }),
        Err(e) => Ok(TerminalPingResult {
            success: false,
            response_code: None,
            error_description: Some(e),
            vendor: None,
            model: None,
        }),
    }
}

// ── Unit-тести ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as IoRead;
    use std::net::TcpListener;

    /// Міні-емулятор термінала ПриватБанку: приймає з'єднання, відповідає на
    /// хендшейк (PingDevice), читає один метод і відповідає terminal_response.
    fn run_fake_terminal(terminal_response: serde_json::Value) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut chunk = [0u8; 4096];

            // 1) Хендшейк: 0x00 + PingDevice + 0x00 — читаємо до першої дейтаграми
            let mut buf: Vec<u8> = Vec::new();
            loop {
                let n = s.read(&mut chunk).unwrap();
                if n == 0 {
                    return;
                }
                buf.extend_from_slice(&chunk[..n]);
                if buf.contains(&0x00) && buf.len() > 1 {
                    break;
                }
            }
            // Відповідь на хендшейк (JSON + 0x00)
            let mut ping_resp = br#"{"method":"PingDevice","step":0,"params":{"code":"00","responseCode":"0000"},"error":false,"errorDescription":""}"#.to_vec();
            ping_resp.push(0x00);
            s.write_all(&ping_resp).unwrap();

            // 2) Метод (Purchase/Refund/Withdrawal) — одна дейтаграма до 0x00
            let mut buf2: Vec<u8> = Vec::new();
            loop {
                let n = s.read(&mut chunk).unwrap();
                if n == 0 {
                    return;
                }
                buf2.extend_from_slice(&chunk[..n]);
                if buf2.last() == Some(&0x00) {
                    break;
                }
            }

            // 3) Відповідь термінала
            let mut resp = serde_json::to_vec(&terminal_response).unwrap();
            resp.push(0x00);
            s.write_all(&resp).unwrap();
        });
        (addr, handle)
    }

    #[test]
    fn test_amount_formatting() {
        // amount завжди рядок з 2 знаками після коми
        assert_eq!(format!("{:.2}", 0.0), "0.00");
        assert_eq!(format!("{:.2}", 0.6), "0.60");
        assert_eq!(format!("{:.2}", 2000.0), "2000.00");
        assert_eq!(format!("{:.2}", 0.1 + 0.2), "0.30");
    }

    #[test]
    fn test_purchase_success() {
        // Приклад успішної відповіді з протоколу (5.1.2)
        let resp = json!({
            "method": "Purchase",
            "step": 0,
            "params": {
                "amount": "0.60",
                "approvalCode": "999999",
                "cardHolderName": "INSTANT/ISSUE",
                "date": "02.10.2019",
                "invoiceNumber": "999999",
                "pan": "4731XXXXXXXX9838",
                "receipt": "ПРИВАТБАНК\nЧЕК ОПЛАТИ",
                "responseCode": "0000",
                "rrn": "9999999999999",
                "terminalId": "TSTSALE2",
                "time": "09:11:07",
                "trnStatus": "1",
                "txnType": "1"
            },
            "error": false,
            "errorDescription": ""
        });
        let (addr, handle) = run_fake_terminal(resp);
        let result = purchase(&addr.ip().to_string(), addr.port(), "Тест-термінал", 0.6).unwrap();
        handle.join().unwrap();

        assert!(result.success, "операція має бути успішною");
        assert_eq!(result.response_code.as_deref(), Some("0000"));
        assert_eq!(result.amount, 0.6);
        assert_eq!(result.rrn.as_deref(), Some("9999999999999"));
        assert_eq!(result.invoice_number.as_deref(), Some("999999"));
        assert_eq!(result.receipt.as_deref(), Some("ПРИВАТБАНК\nЧЕК ОПЛАТИ"));
        assert_eq!(result.trn_status.as_deref(), Some("1"));
        assert_eq!(result.transaction_date.as_deref(), Some("02.10.2019"));
    }

    #[test]
    fn test_purchase_partial_approval() {
        // RC=0010 (Partial approval): error:false — успішна часткова оплата
        let resp = json!({
            "method": "Purchase",
            "step": 0,
            "params": {
                "amount": "1500.00",
                "approvalCode": "999999",
                "invoiceNumber": "999999",
                "responseCode": "0010",
                "rrn": "9999999999999",
                "trnStatus": "1"
            },
            "error": false,
            "errorDescription": ""
        });
        let (addr, handle) = run_fake_terminal(resp);
        let result = purchase(&addr.ip().to_string(), addr.port(), "Тест-термінал", 2000.0).unwrap();
        handle.join().unwrap();

        assert!(result.success, "часткове схвалення — успішна операція");
        assert_eq!(result.response_code.as_deref(), Some("0010"));
        // Збережено часткову суму від термінала
        assert_eq!(result.amount, 1500.0);
        assert!(result.error_description.is_some());
    }

    #[test]
    fn test_purchase_declined() {
        // RC=1001: операція скасована користувачем, error:true
        let resp = json!({
            "method": "Purchase",
            "step": 0,
            "params": { "responseCode": "1001" },
            "error": true,
            "errorDescription": "Transaction canceled by user"
        });
        let (addr, handle) = run_fake_terminal(resp);
        let result = purchase(&addr.ip().to_string(), addr.port(), "Тест-термінал", 100.0).unwrap();
        handle.join().unwrap();

        assert!(!result.success);
        assert_eq!(result.response_code.as_deref(), Some("1001"));
        assert_eq!(
            result.error_description.as_deref(),
            Some("Transaction canceled by user")
        );
    }

    #[test]
    fn test_device_busy() {
        // Термінал зайнятий: ServiceMessage deviceBusy у відповідь на метод
        let resp = json!({
            "method": "ServiceMessage",
            "step": 0,
            "params": { "msgType": "deviceBusy" },
            "error": false,
            "errorDescription": ""
        });
        let (addr, handle) = run_fake_terminal(resp);
        let err = purchase(&addr.ip().to_string(), addr.port(), "Тест-термінал", 10.0).unwrap_err();
        handle.join().unwrap();
        assert!(err.contains("deviceBusy"), "очікуємо помилку deviceBusy, отримали: {err}");
    }

    #[test]
    fn test_withdrawal() {
        let resp = json!({
            "method": "Withdrawal",
            "step": 0,
            "params": {
                "amount": "0.60",
                "approvalCode": "999999",
                "invoiceNumber": "131220",
                "responseCode": "0000",
                "rrn": "9999999999999",
                "trnStatus": "1"
            },
            "error": false,
            "errorDescription": ""
        });
        let (addr, handle) = run_fake_terminal(resp);
        let result = withdrawal(&addr.ip().to_string(), addr.port(), "Тест-термінал", "131220").unwrap();
        handle.join().unwrap();
        assert!(result.success);
        assert_eq!(result.response_code.as_deref(), Some("0000"));
    }
}
