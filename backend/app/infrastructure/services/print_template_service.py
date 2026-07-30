"""
Сервіс для роботи з шаблонами друку чеків (PrintTemplate).

Містить бізнес-логіку:
  - Рендер шаблону: заміна {{variable}} на передані значення
  - Встановлення шаблону за замовчуванням для типу
"""

import logging
from typing import Optional
from uuid import UUID

from sqlalchemy import select, update
from sqlalchemy.ext.asyncio import AsyncSession

from app.infrastructure.persistence.models.print_template import PrintTemplate

logger = logging.getLogger(__name__)


class PrintTemplateService:
    """Сервіс для управління та рендеру шаблонів друку чеків."""

    def __init__(self, session: AsyncSession):
        self.session = session

    # ─── Рендер шаблону ────────────────────────────────────────────────────

    @staticmethod
    def render_template(content: str, data: dict[str, str]) -> str:
        """
        Замінює всі {{variable}} у вмісті шаблону на передані значення.

        Args:
            content: HTML-вміст шаблону з {{змінними}}
            data: словник зі значеннями змінних

        Returns:
            HTML-рядок з підставленими значеннями
        """
        result = content
        for key, value in data.items():
            result = result.replace("{{" + key + "}}", str(value))
        return result

    # ─── Встановлення шаблону за замовчуванням ─────────────────────────────

    async def set_as_default(self, template_id: UUID) -> Optional[PrintTemplate]:
        """
        Встановлює шаблон як основний для свого типу.

        Знімає прапорець is_default з усіх інших шаблонів того самого типу,
        потім встановлює його для вказаного.

        Args:
            template_id: ID шаблону

        Returns:
            Оновлений шаблон або None, якщо не знайдено
        """
        # Отримуємо шаблон
        result = await self.session.execute(
            select(PrintTemplate).where(PrintTemplate.id == template_id)
        )
        template = result.scalar_one_or_none()

        if not template:
            return None

        # Знімаємо is_default з усіх шаблонів того самого типу
        await self.session.execute(
            update(PrintTemplate)
            .where(PrintTemplate.type == template.type)
            .where(PrintTemplate.is_default == True)
            .values(is_default=False)
        )

        # Встановлюємо is_default для вказаного шаблону
        template.is_default = True
        await self.session.flush()

        return template

    # ─── Отримання шаблону за замовчуванням для типу ───────────────────────

    async def get_default_for_type(self, template_type: str) -> Optional[PrintTemplate]:
        """
        Повертає шаблон за замовчуванням для вказаного типу.

        Args:
            template_type: тип шаблону (receipt_58mm, receipt_80mm, fiscal, custom)

        Returns:
            Шаблон за замовчуванням або перший активний шаблон цього типу,
            або None якщо жодного шаблону не знайдено
        """
        # Спочатку шукаємо шаблон з is_default=True
        result = await self.session.execute(
            select(PrintTemplate)
            .where(PrintTemplate.type == template_type)
            .where(PrintTemplate.is_default == True)
            .where(PrintTemplate.is_active == True)
        )
        template = result.scalar_one_or_none()

        if template:
            return template

        # Якщо немає шаблону за замовчуванням — повертаємо перший активний
        result = await self.session.execute(
            select(PrintTemplate)
            .where(PrintTemplate.type == template_type)
            .where(PrintTemplate.is_active == True)
            .order_by(PrintTemplate.created_at.desc())
            .limit(1)
        )
        return result.scalar_one_or_none()
