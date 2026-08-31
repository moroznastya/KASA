//! Сервіси application-шару (етап 6 — auth: авторизація, користувачі, RBAC,
//! налаштування).
//!
//! [`AuthServiceFacade`] — тонкий фасад над портом [`torgashka_domain::AuthService`].
//! Валідація вхідних даних і генерація JWT — на рівні API (torgashka-api);
//! тут лише делегування.

use torgashka_domain::{
    AuthError, AuthService as AuthPort, LoginPinRequest, LoginRequest, LoginResult, PublicUserDto,
    SettingDto, SettingsModulesDto, UserCreateInput, UserDto, UserListDto, UserUpdateInput,
};
use uuid::Uuid;

/// Фасад auth-операцій. Параметризується реалізацією [`AuthPort`].
pub struct AuthServiceFacade<R> {
    repo: R,
}

impl<R: AuthPort> AuthServiceFacade<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn login(&self, input: &LoginRequest) -> Result<LoginResult, AuthError> {
        self.repo.login(input).await
    }
    pub async fn login_pin(&self, input: &LoginPinRequest) -> Result<LoginResult, AuthError> {
        self.repo.login_pin(input).await
    }
    pub async fn refresh(&self, user_id: Uuid) -> Result<LoginResult, AuthError> {
        self.repo.refresh(user_id).await
    }
    pub async fn logout(&self, user_id: Uuid) -> Result<(), AuthError> {
        self.repo.logout(user_id).await
    }
    pub async fn get_user_by_id(&self, user_id: Uuid) -> Result<UserDto, AuthError> {
        self.repo.get_user_by_id(user_id).await
    }
    pub async fn users_list_public(&self) -> Result<Vec<PublicUserDto>, AuthError> {
        self.repo.users_list_public().await
    }
    pub async fn list_users(&self, page: i64, size: i64) -> Result<UserListDto, AuthError> {
        self.repo.list_users(page, size).await
    }
    pub async fn create_user(&self, input: &UserCreateInput) -> Result<UserDto, AuthError> {
        self.repo.create_user(input).await
    }
    pub async fn update_user(
        &self,
        user_id: Uuid,
        input: &UserUpdateInput,
    ) -> Result<UserDto, AuthError> {
        self.repo.update_user(user_id, input).await
    }
    pub async fn update_permissions(
        &self,
        user_id: Uuid,
        permissions: &[String],
    ) -> Result<UserDto, AuthError> {
        self.repo.update_permissions(user_id, permissions).await
    }
    pub async fn update_hourly_rate(
        &self,
        user_id: Uuid,
        hourly_rate: f64,
    ) -> Result<serde_json::Value, AuthError> {
        self.repo.update_hourly_rate(user_id, hourly_rate).await
    }
    pub async fn delete_user(&self, user_id: Uuid, current_user_id: Uuid) -> Result<(), AuthError> {
        self.repo.delete_user(user_id, current_user_id).await
    }
    pub async fn settings_all(&self) -> Result<SettingsModulesDto, AuthError> {
        self.repo.settings_all().await
    }
    pub async fn settings_by_module(&self, module: &str) -> Result<Vec<SettingDto>, AuthError> {
        self.repo.settings_by_module(module).await
    }
    pub async fn settings_batch_update(
        &self,
        settings: &[(String, Option<String>)],
    ) -> Result<SettingsModulesDto, AuthError> {
        self.repo.settings_batch_update(settings).await
    }
    pub async fn settings_update_key(
        &self,
        key: &str,
        value: Option<String>,
    ) -> Result<SettingDto, AuthError> {
        self.repo.settings_update_key(key, value).await
    }
}
