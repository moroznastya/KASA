//! Kasa POS — Infrastructure layer (етап 0 міграції).
//!
//! Реалізації портів application-шару та апаратні інтеграції, перенесені
//! з монолітного Tauri-пакета БЕЗ зміни поведінки (чистий рефакторинг):
//!
//! - [`devices`]    — ← src/commands/devices.rs (915 LOC): COM-ваги, TCP-термінали
//! - [`print`]      — ← src/print.rs (774) + src/commands/print.rs (428): ESC/POS, CUPS
//! - [`terminal`]   — ← src/commands/pb_protocol.rs (689): ПриватБанк ECR (JSON)
//! - [`cash_drawer`]— ← src/utils/cash_drawer.rs (140): грошова скринька
//! - [`offline`]    — ← src/db.rs (344) + src/commands/offline.rs: SQLite/rusqlite
//!
//! ⚠️ Тимчасово залежить від `tauri` (перенесені #[tauri::command] функції).
//! Наступні етапи: винести команди в kasa-tauri-shell, тут лишити чисту логіку.

pub mod cash_drawer;
pub mod db;
pub mod devices;
pub mod offline;
pub mod print;
pub mod prro;
pub mod repositories;
pub mod terminal;
