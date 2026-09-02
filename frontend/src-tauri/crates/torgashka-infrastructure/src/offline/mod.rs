//! Офлайн-шар: SQLite-база (rusqlite) + команди офлайн-режиму.
//!
//! Перенесено з моноліту: `src/db.rs` (OfflineDatabase) та
//! `src/commands/offline.rs` (Tauri-команди) — без зміни поведінки.

pub mod commands;
pub mod db;
pub mod migrations;
pub mod snapshots;
pub mod sync_pull;
pub mod stock;
pub mod sync_push;
pub mod transactions;
