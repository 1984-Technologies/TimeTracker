//! Entry points for fuzzing the production parsers without a database or server.
//! Available only with the explicit `fuzzing` feature. Signature verification,
//! time validation, and all other security checks remain enabled.
use crate::{
    BackendError,
    auth::{AdminAccountRecord, AuthError, AuthService, DeviceRecord},
    repositories::{AdminAccountRepository, DeviceRepository},
};
use async_trait::async_trait;
use std::sync::Arc;
use timetracker_model::{DeviceId, UserId, auth::DeviceApprovalRequest};
use uuid::Uuid;

pub fn auth(secret: String) -> Result<AuthService, BackendError> {
    let repositories = Arc::new(NoRepositories);
    AuthService::new(repositories.clone(), repositories, secret)
}

pub fn credential_digest(secret: &str) -> Result<[u8; 32], BackendError> {
    crate::auth::credential_digest(secret)
}
pub fn decode_credential_hash(hash: &str) -> Result<[u8; 32], BackendError> {
    crate::auth::decode_credential_hash(hash)
}
pub fn verify_device_secret(digest: &[u8; 32], hash: &str) -> Result<(), AuthError> {
    crate::auth::verify_device_secret(digest, hash)
}
pub fn bearer(value: &str) -> Result<&str, AuthError> {
    crate::api::auth::bearer(value)
}
pub fn http_bearer(headers: &axum::http::HeaderMap) -> Result<&str, AuthError> {
    crate::api::auth::http_bearer(headers)
}
pub fn grpc_bearer(metadata: &tonic::metadata::MetadataMap) -> Result<&str, AuthError> {
    crate::api::auth::grpc_bearer(metadata)
}

struct NoRepositories;

#[async_trait]
impl AdminAccountRepository for NoRepositories {
    async fn find_by_id(&self, _id: Uuid) -> Result<Option<AdminAccountRecord>, BackendError> {
        unreachable!("parsing must not access repositories")
    }
    async fn find_by_username(
        &self,
        _username: &str,
    ) -> Result<Option<AdminAccountRecord>, BackendError> {
        unreachable!("parsing must not access repositories")
    }
    async fn list(&self) -> Result<Vec<AdminAccountRecord>, BackendError> {
        unreachable!("parsing must not access repositories")
    }
    async fn create(
        &self,
        _username: &str,
        _hash: &str,
    ) -> Result<AdminAccountRecord, BackendError> {
        unreachable!("parsing must not access repositories")
    }
    async fn change_password(&self, _id: Uuid, _hash: &str) -> Result<(), BackendError> {
        unreachable!("parsing must not access repositories")
    }
    async fn ensure_initial_account(
        &self,
        _username: &str,
        _hash: &str,
    ) -> Result<(), BackendError> {
        unreachable!("parsing must not access repositories")
    }
}

#[async_trait]
impl DeviceRepository for NoRepositories {
    async fn find(&self, _id: DeviceId) -> Result<Option<DeviceRecord>, BackendError> {
        unreachable!("parsing must not access repositories")
    }
    async fn list(&self) -> Result<Vec<DeviceRecord>, BackendError> {
        unreachable!("parsing must not access repositories")
    }
    async fn create_approved(
        &self,
        _request: DeviceApprovalRequest,
    ) -> Result<DeviceRecord, BackendError> {
        unreachable!("parsing must not access repositories")
    }
    async fn owner_of(&self, _id: DeviceId) -> Result<Option<UserId>, BackendError> {
        unreachable!("parsing must not access repositories")
    }
}
