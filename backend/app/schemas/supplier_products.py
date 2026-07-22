"""
Pydantic схеми для перегляду товарів постачальника, їх залишків та руху.

Ендпоінт: GET /suppliers/{supplier_id}/products
"""

from decimal import Decimal
from datetime import datetime
from typing import Optional
from uuid import UUID

from pydantic import BaseModel, Field, ConfigDict


class SupplierProductItem(BaseModel):
    """Товар постачальника з поточним залишком."""
    id: UUID
    barcode: Optional[str] = None
    sku: Optional[str] = None
    title: str
    price: Optional[Decimal] = None
    cost_price: Optional[Decimal] = None
    stock: Optional[Decimal] = None
    unit: Optional[str] = None
    category_name: Optional[str] = None

    model_config = ConfigDict(from_attributes=True)


class SupplierProductMovement(BaseModel):
    """Один запис руху товару."""
    id: UUID
    date: datetime
    document_type: str = Field(..., description="Тип документа: invoice, return_invoice, transfer, write_off, receipt, purchase_order")
    document_number: str
    document_id: UUID
    quantity: Decimal = Field(..., description="Кількість (додатна — прихід, від'ємна — витрата)")
    price: Optional[Decimal] = None
    total: Optional[Decimal] = None
    notes: Optional[str] = None


class SupplierProductDetail(BaseModel):
    """Детальна інформація про товар постачальника з рухом."""
    product: SupplierProductItem
    movements: list[SupplierProductMovement] = []


class SupplierProductsResponse(BaseModel):
    """Відповідь зі списком товарів постачальника."""
    supplier_id: UUID
    supplier_name: str
    total_products: int = Field(..., description="Загальна кількість товарів цього постачальника")
    total_stock_value: Decimal = Field(Decimal("0.00"), description="Загальна вартість залишків (за собівартістю)")
    products: list[SupplierProductItem] = []


class SupplierProductMovementsResponse(BaseModel):
    """Відповідь з рухом конкретного товару постачальника."""
    product: SupplierProductItem
    movements: list[SupplierProductMovement] = []
    total_movements: int = 0
