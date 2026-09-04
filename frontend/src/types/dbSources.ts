// Джерело даних (Етап 3 адмін-панелі, ТЗ 2.4/5.8).
export interface DbSourceView {
  id: string;
  label: string;
  host: string;
  port: number;
  database: string;
  user: string;
  has_password: boolean;
  is_active: boolean;
}

export interface DbSourcesList {
  active: string | null;
  config_path: string;
  sources: DbSourceView[];
}

export interface DbSourceCreate {
  id: string;
  label?: string;
  host: string;
  port: number;
  database: string;
  user: string;
  password?: string;
}

export interface DbSourceUpdate {
  label?: string;
  host?: string;
  port?: number;
  database?: string;
  user?: string;
  /** '' — очистити пароль; задане значення — перешифрувати; undefined — без змін. */
  password?: string;
}

export interface ActivateResult {
  active: string;
  applied_immediately: boolean;
  message: string;
}

export interface ExportResult {
  file: string;
  path: string;
  size_bytes: number;
  source_id: string;
}

export interface DumpInfo {
  file: string;
  size_bytes: number;
  modified_at: string;
}

export interface ImportBody {
  source_id: string;
  file: string;
  clean?: boolean;
}
