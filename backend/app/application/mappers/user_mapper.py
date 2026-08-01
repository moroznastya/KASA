"""
Mapper для User entity.

Конвертує між User (domain entity) та UserDTO / UserCreateDTO.
"""

from app.domain.entities.user import User, UserRole
from app.application.dto.user_dto import UserDTO, UserCreateDTO


class UserMapper:
    """Статичний mapper для конвертації User entity <-> DTO."""

    @staticmethod
    def entity_to_dto(entity: User) -> UserDTO:
        """
        Конвертує User entity в UserDTO.

        Args:
            entity: User entity.

        Returns:
            UserDTO.
        """
        role_value = entity.role.value if hasattr(entity.role, "value") else entity.role
        return UserDTO(
            id=entity.id,
            name=entity.name,
            login=entity.login,
            role=role_value,
            is_active=entity.is_active,
            email=getattr(entity, "email", "") or "",
            phone=getattr(entity, "phone", "") or "",
            created_at=entity.created_at,
            last_login_at=getattr(entity, "last_login_at", None),
        )

    @staticmethod
    def dto_to_entity(dto: UserDTO) -> User:
        """
        Конвертує UserDTO назад в User entity.

        Args:
            dto: UserDTO.

        Returns:
            User entity.
        """
        return User(
            id=dto.id,
            name=dto.name,
            login=dto.login,
            role=UserRole(dto.role),
            is_active=dto.is_active,
            email=dto.email,
            phone=dto.phone,
            created_at=dto.created_at,
            last_login_at=dto.last_login_at,
        )

    @staticmethod
    def create_dto_to_entity(dto: UserCreateDTO) -> User:
        """
        Конвертує UserCreateDTO в нову User entity.

        Args:
            dto: UserCreateDTO.

        Returns:
            Нова User entity.
        """
        return User(
            name=dto.name,
            login=dto.login,
            role=UserRole(dto.role),
            is_active=dto.is_active,
            email=dto.email,
            phone=dto.phone,
        )
