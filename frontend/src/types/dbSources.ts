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
  /** Статус провіжинінгу: "provisioned_pending_activation" = БД створено через
   *  /provision, джерело ще НЕ активоване власником. None = звичайне. */
  status?: string | null;
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

export interface DbSourceProvisionSuperuser {
  user: string;
  password: string;
}

/** POST /admin/db-sources/provision — створити НОВУ БД на кластері. */
export interface DbSourceProvision {
  id: string;
  label?: string;
  host: string;
  port: number;
  /** Ім'я НОВОЇ БД (має бути вільним): ^[a-z_][a-z0-9_]{0,62}$ */
  database: string;
  superuser: DbSourceProvisionSuperuser;
}

export interface DbSourceProvisionResult {
  source: DbSourceView;
  message: string;
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

/**
 * POST /admin/hub-snapshot — знімок УСІЄЇ БД хаба (pg_dump -Fc) для приєднання
 * нового вузла (ADR-0008, Контракт 3). snake_case — як решта DTO джерел даних.
 * Імʼя файла: pos_system_<дата>_<час>.dump; path — абсолютний шлях на сервері.
 */
export interface HubSnapshotResult {
  ok: boolean;
  file_name: string;
  bytes: number;
  sha256: string;
  path: string;
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
