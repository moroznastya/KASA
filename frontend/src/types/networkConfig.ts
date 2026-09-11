/**
 * Конфіг-файл мережі (schema_version=1) — UTF-8 JSON, призначений для
 * перенесення на нову касу (USB/обмінник). Відповідає Rust-моделі
 * admin_network_config.rs (NetCfgFile) і типу конфігу, який каса імпортує
 * ЛОКАЛЬНО в «Мережевій касі» (DeviceSyncPage).
 *
 * Приклад:
 *   {
 *     "schema_version": 1,
 *     "network_id": "<uuid власника мережі>",
 *     "server_url": "http://100.64.0.5:8000",
 *     "exported_at": "2026-09-04T09:02:57+03:00",
 *     "store": { "id": "<uuid>", "name": "Магазин", "activation_code": "ABCD2345" },
 *     "db": { "host": "...", "port": 5432, "database": "...", "user": "...",
 *             "password_encrypted": "<AES-256-GCM>" }   // лише якщо include_db_password
 *   }
 */
export interface NetworkConfigFileStore {
  id: string;
  name: string;
  activation_code: string;
}

export interface NetworkConfigFileDb {
  host: string;
  port: number;
  database: string;
  user: string;
  password_encrypted?: string;
}

export interface NetworkConfigFile {
  schema_version: number;
  network_id: string;
  server_url: string;
  exported_at: string;
  store: NetworkConfigFileStore;
  db?: NetworkConfigFileDb;
}

/** POST /admin/network-config/export. */
export interface ExportNetworkConfigBody {
  store_id: string;
  /** Повний URL фасаду мережі (напр. https://100.64.0.5:8000).
   *  Не задано → env TORGASHKA_FACADE_ADDR → http://127.0.0.1:8000. */
  server_url?: string;
  /** true → додати db.password_encrypted. */
  include_db_password?: boolean;
}

export interface ExportNetworkConfigResult {
  filename: string;
  /** Весь конфіг як JSON-рядок, готовий до збереження у файл. */
  content: string;
}

/** POST /admin/network-config/import. */
export interface ImportNetworkConfigResult {
  ok: boolean;
  network_id: string;
  store: { id: string; name: string };
  server_url: string;
}
