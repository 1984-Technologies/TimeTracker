use super::ApiError;
use std::sync::Arc;

use axum::{
    Router,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use timetracker_model::{DeviceId, auth::*};
use uuid::Uuid;

use crate::{auth::AuthService, dashboard::UserSummary, repositories::UserRepository};

pub struct HttpAuthApi {
    pub auth: Arc<AuthService>,
    pub users: Arc<dyn UserRepository>,
}

impl HttpAuthApi {
    pub fn new(_auth: Arc<AuthService>, _users: Arc<dyn UserRepository>) -> Self {
        todo!()
    }
    pub fn router(&self) -> Router {
        todo!("Mount auth and management routes, with admin middleware on private endpoints.")
    }
    pub async fn login_admin(
        &self,
        _request: AdminLoginRequest,
    ) -> Result<TokenResponse, ApiError> {
        todo!("Pass admin credentials to the auth service and return the JWT response.")
    }
    pub async fn login_device(
        &self,
        _request: DeviceCredentials,
    ) -> Result<TokenResponse, ApiError> {
        todo!("Pass device credentials to the auth service and return the JWT response.")
    }
    pub async fn check_registration(
        &self,
        _request: DeviceCredentials,
    ) -> Result<RegistrationResponse, ApiError> {
        todo!("Check device approval and credentials without creating a registration record.")
    }
    pub async fn current_admin(&self, _claims: JwtClaims) -> Result<AdminAccount, ApiError> {
        todo!("Load the authenticated admin by subject ID and omit the password hash.")
    }
    pub async fn list_admins(&self) -> Result<Vec<AdminAccount>, ApiError> {
        todo!("List admin IDs and usernames without exposing password hashes.")
    }
    pub async fn create_admin(
        &self,
        _request: AdminLoginRequest,
    ) -> Result<AdminAccount, ApiError> {
        todo!("Hash the supplied password with Argon2id and reject duplicate usernames.")
    }
    pub async fn change_password(
        &self,
        _id: Uuid,
        _request: ChangePasswordRequest,
    ) -> Result<(), ApiError> {
        todo!("Store an Argon2id hash of the new password or report an unknown admin.")
    }
    pub async fn list_devices(&self) -> Result<Vec<DeviceSummary>, ApiError> {
        todo!("List device IDs and user assignments without exposing credential hashes.")
    }
    pub async fn find_device(&self, _id: DeviceId) -> Result<DeviceSummary, ApiError> {
        todo!("Return the device ID and user assignment or report a missing device.")
    }
    pub async fn approve_device(
        &self,
        _request: DeviceApprovalRequest,
    ) -> Result<DeviceSummary, ApiError> {
        todo!("Validate the user and credential hash, then create or return an identical approval.")
    }
    pub async fn create_user(&self, _request: CreateUserRequest) -> Result<UserSummary, ApiError> {
        todo!("Validate the name and create a tracked user for device assignment.")
    }
}

pub async fn authenticate_http(
    State(_auth): State<Arc<AuthService>>,
    _request: Request,
    _next: Next,
) -> Result<Response, ApiError> {
    todo!(
        "Validate the bearer token, require an existing admin, and attach claims before forwarding."
    )
}

pub async fn authenticate_grpc(
    _auth: &AuthService,
    _metadata: &tonic::metadata::MetadataMap,
    _device_id: DeviceId,
) -> Result<JwtClaims, tonic::Status> {
    todo!("Validate the bearer token, require an existing device, and match its ID to the request.")
}
