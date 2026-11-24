use std::sync::Arc;

use timetracker_model::auth::{
    AdminLoginRequest, DeviceCredentials, JwtClaims, RegistrationResponse, TokenResponse,
};

use crate::{
    BackendError,
    repositories::{AdminAccountRepository, DeviceRepository},
};

pub struct AdminAccountRecord {
    pub id: uuid::Uuid,
    pub username: String,
    pub password_hash: String,
}

pub struct DeviceRecord {
    pub device_id: timetracker_model::DeviceId,
    pub user_id: timetracker_model::UserId,
    pub credential_hash: String,
}

#[derive(Debug)]
pub enum AuthError {
    InvalidCredentials,
    InvalidToken,
    Forbidden,
    Backend(BackendError),
}

pub struct AuthService {
    pub admins: Arc<dyn AdminAccountRepository>,
    pub devices: Arc<dyn DeviceRepository>,
    pub jwt_secret: String,
}

impl AuthService {
    pub fn new(
        _admins: Arc<dyn AdminAccountRepository>,
        _devices: Arc<dyn DeviceRepository>,
        _jwt_secret: String,
    ) -> Self {
        todo!()
    }

    pub async fn login_admin(
        &self,
        _request: AdminLoginRequest,
    ) -> Result<TokenResponse, AuthError> {
        todo!("Verify the Argon2id password and issue a 15-minute admin JWT.")
    }

    pub async fn login_device(
        &self,
        _credentials: DeviceCredentials,
    ) -> Result<TokenResponse, AuthError> {
        todo!("Verify the approved device secret and issue a 15-minute device JWT.")
    }

    pub async fn check_registration(
        &self,
        _credentials: DeviceCredentials,
    ) -> Result<RegistrationResponse, AuthError> {
        todo!(
            "Return false for unknown devices and reject mismatched secrets without writing records."
        )
    }

    pub fn validate_token(&self, _token: &str) -> Result<JwtClaims, AuthError> {
        todo!("Validate HS256, issuer, audience, expiry, subject UUID, and principal kind.")
    }

    pub async fn authorize(&self, _claims: &JwtClaims) -> Result<(), AuthError> {
        todo!(
            "Confirm that the token subject still exists in the matching admin or device repository."
        )
    }
}
