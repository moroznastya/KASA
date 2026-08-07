// ─────────────────────────────────────────────────────────────────────────────
// sidecar — менеджмент Python-бекенду (FastAPI sidecar на :8001)
// ─────────────────────────────────────────────────────────────────────────────
// При старті Tauri: spawn Python (uvicorn app.main:app --port 8001,
// робоча директорія backend/, venv або системний python3).
// При виході: graceful shutdown — SIGTERM → очікування до 5s → SIGKILL.
// ─────────────────────────────────────────────────────────────────────────────

use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Порт Python sidecar (має збігатися з kasa_api::PYTHON_SIDECAR_PORT).
const SIDECAR_PORT: u16 = 8001;

/// Час очікування graceful shutdown після SIGTERM.
const GRACEFUL_TIMEOUT: Duration = Duration::from_secs(5);

/// Обгортка процесу Python sidecar.
pub struct PythonSidecar {
    child: Option<Child>,
}

impl PythonSidecar {
    /// Запускає Python sidecar (uvicorn на :8001) із backend/.
    ///
    /// Використовує venv, якщо існує (backend/venv/bin/python), інакше python3.
    /// Шлях до backend можна перевизначити через env KASA_BACKEND_DIR.
    pub fn start() -> Self {
        let child = match spawn_uvicorn() {
            Ok(child) => Some(child),
            Err(e) => {
                eprintln!("[sidecar] Python не запущено: {e}");
                None
            }
        };
        Self { child }
    }

    /// Graceful shutdown: SIGTERM → дочекатись → SIGKILL.
    pub fn shutdown(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        #[cfg(unix)]
        {
            // SIGTERM (std::process::Child::kill = SIGKILL, тому libc).
            let pid = child.id() as i32;
            // safety: pid валідний, поки child живий; перевірка try_wait нижче.
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
        #[cfg(not(unix))]
        {
            let _ = child.kill();
        }
        // Очікуємо graceful exit до GRACEFUL_TIMEOUT.
        let deadline = std::time::Instant::now() + GRACEFUL_TIMEOUT;
        while std::time::Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                eprintln!("[sidecar] Python зупинено gracefully");
                self.child = None;
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Не зупинився — SIGKILL.
        let _ = child.kill();
        let _ = child.wait();
        eprintln!("[sidecar] Python зупинено примусово (SIGKILL)");
        self.child = None;
    }
}

impl Drop for PythonSidecar {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Спавн uvicorn-процесу в backend/.
fn spawn_uvicorn() -> std::io::Result<Child> {
    let backend_dir = backend_dir();
    let python = pick_python(&backend_dir);
    Command::new(python)
        .args([
            "-m",
            "uvicorn",
            "app.main:app",
            "--host",
            "127.0.0.1",
            "--port",
            &SIDECAR_PORT.to_string(),
        ])
        .current_dir(&backend_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Робоча директорія backend/ (env override або відносно маніфесту крейта).
fn backend_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("KASA_BACKEND_DIR") {
        return std::path::PathBuf::from(dir);
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend")
}

/// Вибирає інтерпретатор: venv (якщо є) або системний python3.
fn pick_python(backend_dir: &std::path::Path) -> std::path::PathBuf {
    let venv = backend_dir.join("venv/bin/python");
    if venv.exists() {
        venv
    } else {
        std::path::PathBuf::from("python3")
    }
}

// ── Тести ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_dir_resolves_within_project() {
        let dir = backend_dir();
        assert!(dir.ends_with("backend"), "backend_dir = {dir:?}");
    }

    #[test]
    fn pick_python_prefers_venv_when_exists() {
        let fake = std::path::Path::new("/nonexistent/backend");
        // venv не існує → python3.
        assert_eq!(pick_python(fake), std::path::PathBuf::from("python3"));
    }
}
