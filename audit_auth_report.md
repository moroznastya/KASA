# 📋 ЗВІТ АУДИТУ АВТОРИЗАЦІЇ — Kasa POS API v1

**Дата:** 2026-07-30  
**Перевірено:** 118 endpointів у 20 файлах  
**Тип перевірки:** Наявність `Depends(AuthService.get_current_user)` / `Depends(AuthService.require_admin)` / `Depends(get_current_user_optional)`

---

## 1. Endpoints З авторизацією ✅

### `categories.py` — 6 endpoints (prefix: `/api/v1/categories`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/categories | `get_current_user` ✅ |
| GET | /api/v1/categories/tree | `get_current_user` ✅ |
| GET | /api/v1/categories/{category_id} | `get_current_user` ✅ |
| POST | /api/v1/categories | `require_admin` ✅ |
| PUT | /api/v1/categories/{category_id} | `require_admin` ✅ |
| DELETE | /api/v1/categories/{category_id} | `require_admin` ✅ |

### `debtors.py` — 8 endpoints (prefix: `/api/v1/debtors`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/debtors/search | `get_current_user` ✅ |
| GET | /api/v1/debtors | `get_current_user` ✅ |
| POST | /api/v1/debtors | `get_current_user` ✅ |
| GET | /api/v1/debtors/{debtor_id} | `get_current_user` ✅ |
| PUT | /api/v1/debtors/{debtor_id} | `get_current_user` ✅ |
| POST | /api/v1/debtors/{debtor_id}/pay | `get_current_user` ✅ |
| GET | /api/v1/debtors/{debtor_id}/receipts | `get_current_user` ✅ |
| GET | /api/v1/debtors/{debtor_id}/payments | `get_current_user` ✅ |

### `documents.py` — 6 endpoints (prefix: `/api/v1/documents`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/documents | `get_current_user` ✅ |
| POST | /api/v1/documents/batch-confirm | `require_admin` ✅ |
| DELETE | /api/v1/documents/{document_id} | `require_admin` ✅ |
| POST | /api/v1/documents/{document_id}/copy | `require_admin` ✅ |
| GET | /api/v1/documents/export | `get_current_user` ✅ |
| GET | /api/v1/documents/{document_id}/print | `get_current_user_optional` ✅ |

### `inventory.py` — 7 endpoints (prefix: `/api/v1/inventory`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/inventory | `get_current_user` ✅ |
| GET | /api/v1/inventory/counts | `get_current_user` ✅ |
| GET | /api/v1/inventory/{inventory_id} | `get_current_user` ✅ |
| POST | /api/v1/inventory | `require_admin` ✅ |
| PUT | /api/v1/inventory/{inventory_id} | `require_admin` ✅ |
| DELETE | /api/v1/inventory/{inventory_id} | `require_admin` ✅ |
| POST | /api/v1/inventory/{inventory_id}/confirm | `require_admin` ✅ |

### `invoice_ocr.py` — 1 endpoint (prefix: `/api/v1/invoice-ocr`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| POST | /api/v1/invoice-ocr/analyze | `get_current_user` ✅ |

### `invoices.py` — 7 endpoints (prefix: `/api/v1/invoices`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/invoices | `get_current_user` ✅ |
| GET | /api/v1/invoices/{invoice_id} | `get_current_user` ✅ |
| POST | /api/v1/invoices | `get_current_user` ✅ |
| PUT | /api/v1/invoices/{invoice_id} | `require_admin` ✅ |
| DELETE | /api/v1/invoices/{invoice_id} | `require_admin` ✅ |
| GET | /api/v1/invoices/{invoice_id}/payment-info | `get_current_user` ✅ |
| POST | /api/v1/invoices/{invoice_id}/confirm | `require_admin` ✅ |

### `ledger.py` — 3 endpoints (prefix: `/api/v1/ledger`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/ledger/balance/{supplier_id} | `get_current_user` ✅ |
| GET | /api/v1/ledger/{supplier_id} | `get_current_user` ✅ |
| POST | /api/v1/ledger | `require_admin` ✅ |

### `ocr.py` — 1 endpoint (prefix: `/api/v1/ocr`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| POST | /api/v1/ocr/invoice | `get_current_user` ✅ |

### `print.py` — 3 endpoints (prefix: `/api/v1/print`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| POST | /api/v1/print/price-tags/render | `get_current_user` ✅ |
| POST | /api/v1/print/labels/render | `get_current_user` ✅ |
| POST | /api/v1/print/test | `get_current_user` ✅ |

### `print_templates.py` — 9 endpoints (prefix: `/api/v1/print-templates`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/print-templates | `get_current_user` ✅ |
| GET | /api/v1/print-templates/all | `require_admin` ✅ |
| GET | /api/v1/print-templates/default | `get_current_user` ✅ |
| GET | /api/v1/print-templates/{template_id} | `get_current_user` ✅ |
| POST | /api/v1/print-templates | `require_admin` ✅ |
| PUT | /api/v1/print-templates/{template_id} | `require_admin` ✅ |
| DELETE | /api/v1/print-templates/{template_id} | `require_admin` ✅ |
| POST | /api/v1/print-templates/{template_id}/set-default | `require_admin` ✅ |
| POST | /api/v1/print-templates/{template_id}/render | `get_current_user` ✅ |

### `products.py` — 10 endpoints (prefix: `/api/v1/products`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/products | `get_current_user` ✅ |
| GET | /api/v1/products/barcode/{barcode} | `get_current_user` ✅ |
| GET | /api/v1/products/{product_id} | `get_current_user` ✅ |
| POST | /api/v1/products | `require_admin` ✅ |
| PUT | /api/v1/products/{product_id} | `require_admin` ✅ |
| DELETE | /api/v1/products/{product_id} | `require_admin` ✅ |
| POST | /api/v1/products/{product_id}/images | `get_current_user` ✅ |
| DELETE | /api/v1/products/{product_id}/images/{image_id} | `get_current_user` ✅ |
| POST | /api/v1/products/{product_id}/barcodes | `get_current_user` ✅ |
| DELETE | /api/v1/products/{product_id}/barcodes/{barcode_id} | `get_current_user` ✅ |

### `purchase_orders.py` — 6 endpoints (prefix: `/api/v1/purchase-orders`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/purchase-orders | `get_current_user` ✅ |
| GET | /api/v1/purchase-orders/{order_id} | `get_current_user` ✅ |
| POST | /api/v1/purchase-orders | `require_admin` ✅ |
| PUT | /api/v1/purchase-orders/{order_id} | `require_admin` ✅ |
| DELETE | /api/v1/purchase-orders/{order_id} | `require_admin` ✅ |
| POST | /api/v1/purchase-orders/{order_id}/confirm | `require_admin` ✅ |

### `receipts.py` — 7 endpoints (prefix: `/api/v1/receipts`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/receipts/stats/today | `get_current_user` ✅ |
| GET | /api/v1/receipts/search | `get_current_user` ✅ |
| GET | /api/v1/receipts/by-product/{query}/recent-sales | `get_current_user` ✅ |
| GET | /api/v1/receipts/products/{product_id}/returnable-quantity | `get_current_user` ✅ |
| GET | /api/v1/receipts/{receipt_id}/items | `get_current_user` ✅ |
| GET | /api/v1/receipts/{receipt_id} | `get_current_user` ✅ |
| GET | /api/v1/receipts | `get_current_user` ✅ |
| POST | /api/v1/receipts | `get_current_user` ✅ |

### `return_invoices.py` — 6 endpoints (prefix: `/api/v1/return-invoices`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/return-invoices | `get_current_user` ✅ |
| GET | /api/v1/return-invoices/{return_id} | `get_current_user` ✅ |
| POST | /api/v1/return-invoices | `require_admin` ✅ |
| PUT | /api/v1/return-invoices/{return_id} | `require_admin` ✅ |
| DELETE | /api/v1/return-invoices/{return_id} | `require_admin` ✅ |
| POST | /api/v1/return-invoices/{return_id}/confirm | `require_admin` ✅ |

### `settings.py` — 4 endpoints (prefix: `/api/v1/settings`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/settings | `get_current_user` ✅ |
| GET | /api/v1/settings/{module} | `get_current_user` ✅ |
| PUT | /api/v1/settings | `get_current_user` ✅ (+ role check) |
| PUT | /api/v1/settings/{key} | `get_current_user` ✅ (+ role check) |

### `suppliers.py` — 9 endpoints (prefix: `/api/v1/suppliers`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/suppliers | `get_current_user` ✅ |
| GET | /api/v1/suppliers/all | `get_current_user` ✅ |
| GET | /api/v1/suppliers/{supplier_id} | `get_current_user` ✅ |
| POST | /api/v1/suppliers | `require_admin` ✅ |
| PUT | /api/v1/suppliers/{supplier_id} | `require_admin` ✅ |
| DELETE | /api/v1/suppliers/{supplier_id} | `require_admin` ✅ |
| GET | /api/v1/suppliers/{supplier_id}/products | `get_current_user` ✅ |
| GET | /api/v1/suppliers/{supplier_id}/debts | `get_current_user` ✅ |

### `transfers.py` — 6 endpoints (prefix: `/api/v1/transfers`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/transfers | `get_current_user` ✅ |
| GET | /api/v1/transfers/{transfer_id} | `get_current_user` ✅ |
| POST | /api/v1/transfers | `require_admin` ✅ |
| PUT | /api/v1/transfers/{transfer_id} | `require_admin` ✅ |
| DELETE | /api/v1/transfers/{transfer_id} | `require_admin` ✅ |
| POST | /api/v1/transfers/{transfer_id}/confirm | `require_admin` ✅ |

### `users.py` — 12 endpoints (prefixes: `/api/v1/auth`, `/api/v1/users`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| POST | /api/v1/auth/login | **PUBLIC** (немає auth) ✅ |
| POST | /api/v1/auth/login-pin | **PUBLIC** (немає auth) ✅ |
| POST | /api/v1/auth/refresh | **PUBLIC** (немає auth) ✅ |
| POST | /api/v1/auth/logout | `get_current_user` ✅ |
| GET | /api/v1/auth/users-list | **PUBLIC** (немає auth) ✅ |
| GET | /api/v1/users | `require_admin` ✅ |
| GET | /api/v1/users/{user_id} | `require_admin` ✅ |
| POST | /api/v1/users | `require_admin` ✅ |
| PUT | /api/v1/users/{user_id} | `require_admin` ✅ |
| PUT | /api/v1/users/{user_id}/permissions | `require_admin` ✅ |
| PUT | /api/v1/users/{user_id}/hourly-rate | `require_admin` ✅ |
| DELETE | /api/v1/users/{user_id} | `require_admin` ✅ |
| GET | /api/v1/users/permissions/list | `require_admin` ✅ |

### `work_sessions.py` — 2 endpoints (prefix: `/api/v1/work-sessions`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/work-sessions/my | `get_current_user` ✅ |
| GET | /api/v1/work-sessions/report | `require_admin` ✅ |

### `write_offs.py` — 6 endpoints (prefix: `/api/v1/write-offs`)
| Метод | Шлях | Рівень |
|-------|------|--------|
| GET | /api/v1/write-offs | `get_current_user` ✅ |
| GET | /api/v1/write-offs/{write_off_id} | `get_current_user` ✅ |
| POST | /api/v1/write-offs | `require_admin` ✅ |
| PUT | /api/v1/write-offs/{write_off_id} | `require_admin` ✅ |
| DELETE | /api/v1/write-offs/{write_off_id} | `require_admin` ✅ |
| POST | /api/v1/write-offs/{write_off_id}/confirm | `require_admin` ✅ |

---

## 2. Endpoints БЕЗ авторизації (публічні) ⚠️

| Метод | Шлях | Статус |
|-------|------|--------|
| POST | /api/v1/auth/login | ⚠️ Публічний — OK (вхід в систему) |
| POST | /api/v1/auth/login-pin | ⚠️ Публічний — OK (вхід по PIN) |
| POST | /api/v1/auth/refresh | ⚠️ Публічний — OK (оновлення токена) |
| GET | /api/v1/auth/users-list | ⚠️ Публічний — OK (список для логін-форми) |

**Проблемних endpointів без авторизації: 0** ✅

---

## 3. 🔴 КРИТИЧНА ПРОБЛЕМА: Middleware авторизації

### Проблема: `"/print" in path` — надто широкий public-path

**Файл:** `backend/app/middleware/auth_middleware.py:157`

```python
# Шляхи друку (аутентифікація на рівні ендпоінта через get_current_user_optional)
if "/print" in path:
    return True
```

**Опис:** Middleware пропускає **ВСІ** шляхи, що містять підрядок `"/print"`, без перевірки токена на рівні middleware. Це робить публічними:

| Шлях | Проблема |
|------|---------|
| `/api/v1/print/price-tags/render` | Має `Depends` — захищений на рівні endpoint, але middleware не блокує |
| `/api/v1/print/labels/render` | Має `Depends` — захищений |
| `/api/v1/print/test` | Має `Depends` — захищений |
| **`/api/v1/print-templates/...` (9 шляхів)** | **ВСІ мають `Depends`, але middleware їх пропускає!** |
| **`/api/v1/documents/{id}/print`** | **Має `get_current_user_optional`, але middleware пропускає!** |

**Ризик:** Якщо новий endpoint буде додано під `/print` (наприклад, `/api/v1/print/report`) без явного `Depends(AuthService.get_current_user)`, він буде **повністю відкритий** без жодної авторизації.

### Проблема: Дві системи авторизації — плутанина

Система має **ДВА** рівні авторизації:
1. **Middleware** (`AuthMiddleware`) — перевіряє токен на ASGI рівні для всіх запитів, крім public-path
2. **Per-endpoint Depends** — кожен endpoint має `Depends(AuthService.get_current_user)` або `Depends(AuthService.require_admin)`

**Наслідок:** Для більшості endpointів auth перевіряється двічі. Але для `/print*` шляхів — тільки один раз (на рівні Depends). Це створює:
- Непотрібне дублювання (для 110+ endpointів)
- Нерівномірний захист (для print-endpoints)
- Плутанину при додаванні нових endpointів

---

## 4. Висновки

| Показник | Значення |
|----------|----------|
| **Загальна кількість endpointів** | **118** |
| **З авторизацією (`Depends`)** | **114** ✅ |
| **Публічних (login, refresh, users-list)** | **4** ⚠️ (всі свідомо публічні) |
| **Проблемних (без жодної авторизації)** | **0** ✅ |
| **Але: вразливих через middleware** | **~13** 🟡 (print/print-templates/document-print — захищені тільки per-endpoint) |

### Рекомендації

1. **🔴 HIGH:** Виправити `_is_public_path()` — замінити `"/print" in path` на точні шляхи:
   ```python
   PUBLIC_PATHS.update({
       "/api/v1/print/price-tags/render",
       "/api/v1/print/labels/render",
       "/api/v1/print/test",
   })
   ```
   І прибрати загальне `if "/print" in path`.

2. **🟡 MEDIUM:** Розглянути видалення `AuthMiddleware` і покладатися тільки на per-endpoint `Depends` — це стандартна практика FastAPI і усуває плутанину.

3. **🟢 LOW:** Документувати політику авторизації: всі endpoints мають мати явний `Depends(AuthService.get_current_user)` або `Depends(AuthService.require_admin)`.

### Загальний вердикт

Код endpointів **чистий** — жоден endpoint не пропустив `Depends`.  
Але **архітектура авторизації** (middleware + per-endpoint) має **критичний недолік** у вигляді надто широкого public-path для `/print`.
