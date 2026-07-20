"""
Infrastructure Layer: Dependency Injection.

Містить DIContainer та Service Registry для керування залежностями.
"""

from .container import DIContainer
from .service_registry import register_all_services

__all__ = [
    "DIContainer",
    "register_all_services",
]
