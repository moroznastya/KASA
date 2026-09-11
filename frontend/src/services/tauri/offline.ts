/**
 * Сервіс для роботи з офлайн-режимом через Tauri v2 Desktop API.
 *
 * Надає функції для:
 *   - Кешування товарів локально
 *   - Збереження чеків при відсутності інтернету
 *   - Синхронізації при поновленні з'єднання
 *   - Роботи з локальними налаштуваннями
 */

import { invoke } from '@tauri-apps/api/core';

// ─── Типи ───────────────────────────────────────────────────────────────────

export interface OfflineStats {
  products_cached: number;
  unsynced_receipts: number;
  db_size_bytes: number;
  db_path: string;
}

// ─── Офлайн-режим ───────────────────────────────────────────────────────────

/**
 * Перевірити чи доступний офлайн-режим.
 */
export async function isOfflineAvailable(): Promise<boolean> {
  return invoke<boolean>('is_offline_available');
}

/**
 * Перевірити доступність інтернету через Tauri.
 *
 * ⚠️ Правильна назва команди: `check_online` (не `is_online`).
 */
export async function checkOnline(): Promise<boolean> {
  return invoke<boolean>('check_online');
}

/**
 * Отримати кількість несинхронізованих чеків.
 */
export async function getUnsyncedCount(): Promise<number> {
  return invoke<number>('get_unsynced_count');
}

// ─── Синхронізація (outbox-push, ЕТАП 4/5 Rust) ─────────────────────────────────

/** Діагностика циклу синхронізації (команда `sync_health`).
 *
 * Контракт (torgashka-infrastructure/src/offline/health.rs):
 *   { outbox_pending, outbox_failed, stale_failed_since,
 *     last_push_ok_at, last_pull_ok_at, last_push_fail_at,
 *     last_error, degraded }
 *
 * `degraded = outbox_failed > 0` АБО є pending з next_attempt_at простроченим
 * > 3600с (стагнація: цикл push не рухає чергу ≥ 1 год). Споживається
 * SyncStatus — алерт на стагнацію (QA ЕТАП 7 §1.3).
 */
export interface SyncHealthResult {
  /** Усі записи outbox зі статусом 'pending' */
  outbox_pending: number;
  /** Записи зі статусом 'failed' (вичерпали спроби / валідація) */
  outbox_failed: number;
  /** ISO-час, з якого триває проблемний стан (failed/stale), null — чисто */
  stale_failed_since: string | null;
  /** ISO-час останнього УСПІШНОГО push, null — ще не було */
  last_push_ok_at: string | null;
  /** ISO-час останнього УСПІШНОГО pull, null — ще не було */
  last_pull_ok_at: string | null;
  /** ISO-час останньої НЕВДАЛОЇ спроби push, null — не було */
  last_push_fail_at: string | null;
  /** Остання помилка health-контексту (детальніша за sync_status.last_error) */
  last_error: string | null;
  /** true: failed>0 АБО stale pending (цикл не рухає чергу ≥ 1 год) */
  degraded: boolean;
}

/**
 * Статус outbox-черги (команда `sync_status`).
 *
 * Контракт (torgashka-infrastructure/src/offline/commands.rs):
 *   { pending_count, failed_count, last_error, last_sync_at }
 */
export interface SyncStatusResult {
  /** Записи в outbox зі статусом 'pending' */
  pending_count: number;
  /** Записи зі статусом 'failed' — потребують уваги */
  failed_count: number;
  /** Остання помилка push (null, якщо все чисто) */
  last_error: string | null;
  /** ISO-час останнього успішного push (MAX(pushed_at) по done), null — ще не було */
  last_sync_at: string | null;
  /** Діагностика циклу; відсутня у старих Rust-збірках (undefined) */
  health?: SyncHealthResult | null;
}

/**
 * Результат ручного push (команда `sync_now`).
 *
 * Контракт (torgashka-infrastructure/src/offline/commands.rs):
 *   { pushed, already_exists, failed, remaining, last_error }
 * Команда повторює батчі, поки є pending (або поки є мережевий прогрес).
 */
export interface SyncNowResult {
  /** Відправлено успішно (created) */
  pushed: number;
  /** Вже існували на сервері (ідемпотентний повтор) */
  already_exists: number;
  /** Не вдалося відправити (5xx/429/валідація) — outbox status='failed' */
  failed: number;
  /** Залишилось у черзі (pending після спроби) */
  remaining: number;
  /** Остання помилка push (null — все чисто) */
  last_error: string | null;
}

/**
 * Отримати статус outbox-черги.
 *
 * Каса без Rust/Tauri (або стара версія без команди): invoke падає →
 * повертає null (UI показує 'unavailable').
 */
export async function syncStatus(): Promise<SyncStatusResult | null> {
  try {
    return await invoke<SyncStatusResult>('sync_status');
  } catch {
    return null;
  }
}

/** Діагностика циклу синхронізації (sync_health).
 *
 * Викликається SyncStatus як fallback, коли `sync_status` ще не повертає
 * `health` (проміжна Rust-збірка). Каса без Tauri/Rust: invoke падає →
 * null (UI без degraded-алерта, старий стан).
 */
export async function syncHealth(): Promise<SyncHealthResult | null> {
  try {
    return await invoke<SyncHealthResult>('sync_health');
  } catch {
    return null;
  }
}

/**
 * Ручний тригер push: повторює батчі, поки є pending.
 *
 * Каса без Rust/Tauri: invoke падає → повертає null.
 */
export async function syncNow(): Promise<SyncNowResult | null> {
  try {
    return await invoke<SyncNowResult>('sync_now');
  } catch {
    return null;
  }
}

// ─── Товари (кеш) ───────────────────────────────────────────────────────────

/**
 * Кешувати товари локально (для поточної точки продажу).
 *
 * @param products - Масив товарів для кешування
 * @param storeId  - UUID точки продажу (мультиточковість, Етап 5)
 */
export async function cacheProducts(products: unknown[], storeId?: string | null): Promise<number> {
  return invoke<number>('cache_products', {
    productsJson: JSON.stringify(products),
    storeId: storeId ?? null,
  });
}

/**
 * Записати помилку фронтенду у /tmp/kasa-frontend.log (діагностика).
 * Використовується пасткою window.onerror / unhandledrejection у main.tsx.
 */
export async function logFrontendError(message: string): Promise<void> {
  try {
    await invoke('log_frontend_error', { message });
  } catch {
    // Лог — некритичний: якщо команда недоступна, мовчки ігноруємо
  }
}

/**
 * Отримати кешовані товари поточної точки.
 *
 * @param search  - Пошуковий запит (опціонально)
 * @param limit   - Максимальна кількість (опціонально, за замовчуванням 100)
 * @param storeId - UUID точки продажу (фільтр кешу, мультиточковість)
 */
export async function getCachedProducts(
  search?: string,
  limit?: number,
  storeId?: string | null,
): Promise<unknown[]> {
  const result = await invoke<string>('get_cached_products', {
    search: search ?? null,
    limit: limit ?? 100,
    storeId: storeId ?? null,
  });
  return JSON.parse(result);
}

/**
 * Очистити кеш товарів.
 */
export async function clearProductCache(): Promise<number> {
  return invoke<number>('clear_product_cache');
}

// ─── Чеки (офлайн) ──────────────────────────────────────────────────────────

/**
 * Зберегти чек локально (з точкою продажу).
 *
 * @param receipt - Дані чека
 * @param storeId - UUID точки продажу — зберігається в черзі, щоб при
 *                  синхронізації чек потрапив у правильну точку (Етап 5)
 * @returns ID збереженого чека
 */
export async function saveReceiptOffline(receipt: unknown, storeId?: string | null): Promise<number> {
  return invoke<number>('save_receipt_offline', {
    receiptJson: JSON.stringify(receipt),
    storeId: storeId ?? null,
  });
}

/**
 * Отримати несинхронізовані чеки (з store_id для коректної синхронізації).
 */
export interface UnsyncedReceipt {
  id: number;
  data: string;
  store_id: string | null;
}
export async function getUnsyncedReceipts(): Promise<UnsyncedReceipt[]> {
  return invoke<UnsyncedReceipt[]>('get_unsynced_receipts');
}

/**
 * Позначити чек як синхронізований.
 *
 * @param receiptId - ID чека
 */
export async function markReceiptSynced(receiptId: number): Promise<void> {
  return invoke<void>('mark_receipt_synced', { receiptId });
}

// ─── Налаштування ───────────────────────────────────────────────────────────

/**
 * Отримати локальне налаштування.
 *
 * @param key - Ключ налаштування
 */
export async function getSetting(key: string): Promise<string | null> {
  return invoke<string | null>('get_setting', { key });
}

/**
 * Зберегти локальне налаштування.
 *
 * @param key - Ключ налаштування
 * @param value - Значення
 */
export async function setSetting(key: string, value: string): Promise<void> {
  return invoke<void>('set_setting', { key, value });
}

/**
 * Зберегти облікові дані синхронізації (`server_url` + `api_token`).
 * Викликається після успішного логіна / оновлення токена (authStore).
 * Rust читає ці ключі з SQLite settings перед кожним push (sync_now).
 *
 * @returns true — збережено; false — не Tauri/помилка invoke
 */
export async function persistSyncCredentials(apiToken: string, serverUrl: string): Promise<boolean> {
  try {
    await invoke<void>('set_setting', { key: 'api_token', value: apiToken });
    await invoke<void>('set_setting', { key: 'server_url', value: serverUrl });
    return true;
  } catch {
    return false;
  }
}

/**
 * Зберегти активну торгову точку (`store_id`).
 * Викликається при зміні точки / автовиборі після логіна (storeStore).
 * Rust використовує store_id як контекст для POST /sync/push.
 *
 * @returns true — збережено; false — не Tauri/помилка invoke
 */
export async function persistSyncStore(storeId: string): Promise<boolean> {
  try {
    await invoke<void>('set_setting', { key: 'store_id', value: storeId });
    return true;
  } catch {
    return false;
  }
}

/**
 * Увімкнути device-режим синхронізації (`server_url` + `device_token`).
 * Викликається після успішної активації каси як мережевого пристрою
 * (POST /api/v1/devices/activate — Етап 3).
 *
 * Rust (read_sync_auth): непустий `device_token` має ПРІОРИТЕТ над
 * `api_token` (store_id не потрібен — сервер визначає точку з токена).
 * `set_setting` автоматично (пере)запускає фонові push/pull-цикли —
 * додаткових дій після виклику не потрібно.
 *
 * @returns true — збережено; false — не Tauri/помилка invoke
 */
export async function persistSyncDevice(serverUrl: string, deviceToken: string): Promise<boolean> {
  try {
    await invoke<void>('set_setting', { key: 'server_url', value: serverUrl });
    await invoke<void>('set_setting', { key: 'device_token', value: deviceToken });
    return true;
  } catch {
    return false;
  }
}

/**
 * Вимкнути device-режим: `device_token` = порожній рядок (ключ не
 * налаштовано; Rust повернеться до legacy api_token, якщо він є).
 * `server_url` не чіпаємо — він спільний для обох режимів.
 *
 * @returns true — збережено; false — не Tauri/помилка invoke
 */
export async function clearSyncDevice(): Promise<boolean> {
  try {
    await invoke<void>('set_setting', { key: 'device_token', value: '' });
    return true;
  } catch {
    return false;
  }
}


// ─── Статистика ─────────────────────────────────────────────────────────────

/**
 * Отримати статистику офлайн-бази (кількість товарів — поточної точки).
 */
export async function getOfflineStats(storeId?: string | null): Promise<OfflineStats> {
  return invoke<OfflineStats>('get_offline_stats', { storeId: storeId ?? null });
}

// ─── Операції каси поза продажем (ЕТАП 6 offline-first) ─────────────────────
//
// Контракт (torgashka-infrastructure/src/offline/transactions.rs):
//   save_*_offline(payload: String, store_id: String) → client_uuid (String)
// Payload — JSON, який фронт будує для серверного ендпоінта; Rust читає
// лише items[].product_id + quantity (або fact_quantity для інвентаризації)
// та from_store_id/to_store_id (переміщення). Весь payload зберігається
// в data агрегата (таблиці 0006) — для майбутнього push (ЕТАП 7).
// Stock-ефект і запис агрегата — в одній SQLite-транзакції.

/**
 * Локальна закупка/надходження: items[].product_id + quantity → stock +qty.
 * @returns client_uuid агрегата (ідемпотентний ключ майбутнього push)
 */
export async function savePurchaseOrderOffline(payload: unknown, storeId: string): Promise<string> {
  return invoke<string>('save_purchase_order_offline', {
    payload: JSON.stringify(payload),
    storeId,
  });
}

/**
 * Локальна інвентаризація: items[].product_id + fact_quantity (або quantity)
 * → stock = факт (абсолютний рівень). UI передає quantity=факт за перерахунком.
 * @returns client_uuid агрегата
 */
export async function saveInventoryOffline(payload: unknown, storeId: string): Promise<string> {
  return invoke<string>('save_inventory_offline', {
    payload: JSON.stringify(payload),
    storeId,
  });
}

/**
 * Локальне переміщення між точками: payload.from_store_id/to_store_id
 * визначають сторону каси (from=каса → −qty; to=каса → +qty; чуже → запис).
 * @returns client_uuid агрегата
 */
export async function saveTransferOffline(payload: unknown, storeId: string): Promise<string> {
  return invoke<string>('save_transfer_offline', {
    payload: JSON.stringify(payload),
    storeId,
  });
}

/**
 * Локальне списання: items[].product_id + quantity → stock −qty.
 * @returns client_uuid агрегата
 */
export async function saveWriteOffOffline(payload: unknown, storeId: string): Promise<string> {
  return invoke<string>('save_write_off_offline', {
    payload: JSON.stringify(payload),
    storeId,
  });
}

/** Елемент локальних залишків каталогу (get_stock_levels). */
export interface StockLevelItem {
  product_id: string;
  name: string;
  quantity: number;
}

/**
 * Локальні залишки всього каталогу точки: [{product_id, name, quantity}].
 * Товари без stock-рядка → quantity=0. Недоступно (браузер) → [].
 */
export async function getStockLevels(storeId: string): Promise<StockLevelItem[]> {
  try {
    return await invoke<StockLevelItem[]>('get_stock_levels', { storeId });
  } catch {
    return [];
  }
}

/**
 * Локальний залишок одного товару точки (одиниці; 0 — рядка немає).
 * Недоступно (браузер) → 0.
 */
export async function getStockLevel(productId: string, storeId: string): Promise<number> {
  try {
    return await invoke<number>('get_stock_level', { productId, storeId });
  } catch {
    return 0;
  }
}
