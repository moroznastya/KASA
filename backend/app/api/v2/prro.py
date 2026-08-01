"""
PRRO API v2 — налаштування, зміни, фіскалізація, статус, черга.

Базовий шлях: /api/v2/prro
Теги: ["ПРРО"]
"""

from __future__ import annotations

from typing import Optional
from uuid import UUID

from fastapi import (
    APIRouter,
    Depends,
    File,
    Form,
    HTTPException,
    Query,
    Request,
    UploadFile,
    status,
)

from app.application.dto.prro_dto import (
    CloseShiftRequestDTO,
    FiscalizeRequestDTO,
    FiscalizeResponseDTO,
    OpenShiftRequestDTO,
    PrroSettingsDTO,
    PrroShiftDTO,
    PrroStatusDTO,
)
from app.application.use_cases.prro import (
    PrroFiscalizeError,
    PrroSettingsError,
    PrroShiftError,
    PrroUseCases,
)
from app.domain.services.cache_service import ICacheService
from .cache_utils import invalidate_receipt_cache
from .deps import get_cache_service, get_prro_use_cases

router = APIRouter(prefix="/prro", tags=["ПРРО"])


async def require_admin_role(request: Request) -> None:
    """
    Перевіряє, що автентифікований користувач має роль admin
    (адміністратор / старший касир).

    AuthMiddleware встановлює scope["user_role"] з JWT-токена.
    Чутливі операції ПРРО (налаштування ключа, Z-звіт, синхронізація)
    дозволені лише адміністратору.
    """
    role = request.scope.get("user_role")
    if role != "admin":
        raise HTTPException(
            status_code=403,
            detail="Потрібні права адміністратора (старшого касира)",
        )


# ─── Налаштування ───────────────────────────────────────────────────────────

@router.get("/settings", response_model=PrroSettingsDTO)
async def get_settings(
    prro: PrroUseCases = Depends(get_prro_use_cases),
):
    """Отримати налаштування ПРРО (пароль ключа — тільки маска)."""
    return await prro.get_settings()


@router.put("/settings", response_model=PrroSettingsDTO)
async def save_settings(
    key_file: Optional[UploadFile] = File(default=None),
    key_password: Optional[str] = Form(default=None),
    prro_fn: Optional[str] = Form(default=None),
    prro_tn: Optional[str] = Form(default=None),
    prro_zn: Optional[str] = Form(default=None),
    mode: Optional[str] = Form(default=None),
    key_file_path: Optional[str] = Form(default=None),
    auto_fiscalize: Optional[str] = Form(default=None),
    prro: PrroUseCases = Depends(get_prro_use_cases),
    _: None = Depends(require_admin_role),
):
    """
    Зберегти налаштування ПРРО.

    - key_file — завантажений файл ключа КЕП (multipart);
    - key_file_path — або шлях до існуючого файлу ключа;
    - key_password — пароль ключа (шифрується Fernet);
    - prro_fn / prro_tn / prro_zn — реквізити ПРРО;
    - mode — "test" / "prod";
    - auto_fiscalize — "true"/"false": авто-фіскалізація чеків після продажу.
    """
    try:
        content = None
        name = None
        if key_file is not None:
            content = await key_file.read()
            name = key_file.filename
        return await prro.save_settings(
            key_file_path=key_file_path,
            key_file_content=content,
            key_file_name=name,
            key_password=key_password,
            prro_fn=prro_fn,
            prro_tn=prro_tn,
            prro_zn=prro_zn,
            mode=mode,
            auto_fiscalize=_parse_bool_form(auto_fiscalize),
        )
    except PrroSettingsError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.post("/test-connection")
async def test_connection(
    prro: PrroUseCases = Depends(get_prro_use_cases),
    _: None = Depends(require_admin_role),
):
    """Перевірити зв'язок з фіскальним сервером (ping)."""
    return await prro.test_connection()


# ─── Статус ─────────────────────────────────────────────────────────────────

@router.get("/status", response_model=PrroStatusDTO)
async def get_status(
    prro: PrroUseCases = Depends(get_prro_use_cases),
):
    """Отримати статус ПРРО (statusRro/infoRro + локальний стан)."""
    return await prro.get_status()


# ─── Зміни ──────────────────────────────────────────────────────────────────

@router.post("/shift/open", response_model=PrroShiftDTO)
async def open_shift(
    data: Optional[OpenShiftRequestDTO] = None,
    prro: PrroUseCases = Depends(get_prro_use_cases),
):
    """Відкрити зміну ПРРО (службовий чек T=108)."""
    try:
        return await prro.open_shift(data)
    except PrroShiftError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.post("/shift/close", response_model=PrroShiftDTO)
async def close_shift(
    data: Optional[CloseShiftRequestDTO] = None,
    prro: PrroUseCases = Depends(get_prro_use_cases),
    _: None = Depends(require_admin_role),
):
    """Закрити зміну ПРРО (Z-звіт)."""
    try:
        return await prro.close_shift(data)
    except PrroShiftError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.get("/shifts", response_model=dict)
async def list_shifts(
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    prro: PrroUseCases = Depends(get_prro_use_cases),
):
    """Список змін ПРРО з пагінацією."""
    shifts, total = await prro.list_shifts(page=page, size=size)
    return {"items": shifts, "total": total, "page": page, "size": size}


# ─── Фіскалізація ───────────────────────────────────────────────────────────

@router.post(
    "/receipts/{receipt_id}/fiscalize",
    response_model=FiscalizeResponseDTO,
)
async def fiscalize_receipt(
    receipt_id: UUID,
    data: Optional[FiscalizeRequestDTO] = None,
    prro: PrroUseCases = Depends(get_prro_use_cases),
    cache: ICacheService = Depends(get_cache_service),
):
    """Фіскалізувати чек (ручна фіскалізація)."""
    try:
        # Якщо тіло не передано — вважаємо ручною фіскалізацією (юзер натиснув кнопку)
        result = await prro.fiscalize_receipt(
            receipt_id, manual=(data.manual if data else True)
        )
        # Інвалідуємо кеш чеку/списків, щоб GET показував актуальний
        # фіскальний стан одразу після фіскалізації
        await invalidate_receipt_cache(cache)
        return result
    except PrroFiscalizeError as e:
        await invalidate_receipt_cache(cache)
        raise HTTPException(status_code=400, detail=str(e))


# ─── Синхронізація офлайн-черги ─────────────────────────────────────────────

@router.post("/sync")
async def sync_offline_queue(
    limit: int = Query(100, ge=1, le=500),
    prro: PrroUseCases = Depends(get_prro_use_cases),
    _: None = Depends(require_admin_role),
):
    """Синхронізувати офлайн-чергу ПРРО (повторна передача)."""
    return await prro.sync_offline_queue(limit=limit)


@router.get("/queue")
async def get_queue(
    page: int = Query(1, ge=1),
    size: int = Query(20, ge=1, le=100),
    status_filter: Optional[str] = Query(default=None, alias="status"),
    prro: PrroUseCases = Depends(get_prro_use_cases),
):
    """Журнал офлайн-черги ПРРО."""
    return await prro.get_queue(page=page, size=size, status_filter=status_filter)


def _parse_bool_form(value: str | None) -> bool | None:
    """Перетворює текстове значення form-поля у bool | None."""
    if value is None:
        return None
    return value.strip().lower() in ("1", "true", "yes", "on")
