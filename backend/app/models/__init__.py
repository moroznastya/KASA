"""
Зворотна сумісність: реекспорт усіх моделей з Infrastructure Layer.

Всі SQLAlchemy моделі тепер знаходяться в:
    app.infrastructure.persistence.models

Цей файл забезпечує зворотну сумісність для коду,
який ще імпортує з app.models.
"""

from app.infrastructure.persistence.models import *  # noqa: F403
