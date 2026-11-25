use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{DeviceId, UserId};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalKind {
    Admin,
    Device,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: Uuid,
    pub kind: PrincipalKind,
    pub iss: String,
    pub aud: String,
    pub iat: u64,
    pub exp: u64,
}

#[derive(Serialize, Deserialize)]
pub struct AdminLoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize)]
pub struct DeviceCredentials {
    pub device_id: DeviceId,
    pub secret: String,
}

#[derive(Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdminAccount {
    pub id: Uuid,
    pub username: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub device_id: DeviceId,
    pub user_id: UserId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceApprovalRequest {
    pub device_id: DeviceId,
    pub user_id: UserId,
    pub credential_hash: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RegistrationResponse {
    pub registered: bool,
}

#[derive(Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub password: String,
}

#[derive(Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
}
