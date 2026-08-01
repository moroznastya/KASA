"""API v2 — Use Cases based API для Kasa POS."""

from fastapi import APIRouter

from . import products, invoices, receipts, auth, ledger, categories, prro

router = APIRouter(prefix="/api/v2")
router.include_router(products.router)
router.include_router(invoices.router)
router.include_router(receipts.router)
router.include_router(auth.router)
router.include_router(ledger.router)
router.include_router(categories.router)
router.include_router(prro.router)

__all__ = ["router"]
