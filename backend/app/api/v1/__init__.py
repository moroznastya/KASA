"""
Ініціалізація API v1 роутерів.

Всі роутери версії 1 API.
"""

from fastapi import APIRouter

from app.api.v1.products import router as products_router
from app.api.v1.categories import router as categories_router
from app.api.v1.suppliers import router as suppliers_router
from app.api.v1.users import auth_router, users_router
from app.api.v1.invoices import router as invoices_router
from app.api.v1.transfers import router as transfers_router
from app.api.v1.write_offs import router as write_offs_router
from app.api.v1.return_invoices import router as return_invoices_router
from app.api.v1.purchase_orders import router as purchase_orders_router
from app.api.v1.receipts import router as receipts_router
from app.api.v1.ledger import router as ledger_router
from app.api.v1.documents import router as documents_router
from app.api.v1.debtors import router as debtors_router
from app.api.v1.ocr import router as ocr_router

# Головний роутер v1 API
api_v1_router = APIRouter(prefix="/api/v1")

# Підключаємо всі роутери
api_v1_router.include_router(auth_router)
api_v1_router.include_router(users_router)
api_v1_router.include_router(products_router)
api_v1_router.include_router(categories_router)
api_v1_router.include_router(suppliers_router)
api_v1_router.include_router(invoices_router)
api_v1_router.include_router(transfers_router)
api_v1_router.include_router(write_offs_router)
api_v1_router.include_router(return_invoices_router)
api_v1_router.include_router(purchase_orders_router)
api_v1_router.include_router(receipts_router)
api_v1_router.include_router(ledger_router)
api_v1_router.include_router(documents_router)
api_v1_router.include_router(debtors_router)
api_v1_router.include_router(ocr_router)

__all__ = ["api_v1_router"]
