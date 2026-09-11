import api from './api';

/**
 * Міграція існуючих інсталяцій (Етап 6, ТЗ §9).
 *
 * POST /api/v1/admin/migrate/legacy — перетворення одиночної інсталяції на
 * мережу: перша точка (або наявна) + автоматична реєстрація цієї каси як
 * device зі статусом active БЕЗ коду активації (source='legacy_migration').
 * Endpoint ідемпотентний: повторний виклик не дублює device і не пише
 * зайвий audit-запис.
 *
 * Роут /api/v1/admin/* — поза store_middleware (X-Store-Id не потрібен);
 * JWT додається інтерцептором api.ts автоматично.
 */

export interface LegacyMigrateDevice {
    id: string;
    store_id: string;
    name: string;
    status: 'active' | 'blocked' | 'deleted';
    /** 'legacy_migration' — пристрій, зареєстрований міграцією §9. */
    source: string | null;
}

export interface LegacyMigrateResult {
    created_store: boolean;
    store: { id: string; name: string };
    created_device: boolean;
    device: LegacyMigrateDevice;
}

export const adminMigrateService = {
    /** Виконати міграцію legacy-інсталяції (owner|admin). */
    async migrateLegacy(): Promise<LegacyMigrateResult> {
        const response = await api.post<LegacyMigrateResult>('/admin/migrate/legacy', {});
        return response.data;
    },
};
