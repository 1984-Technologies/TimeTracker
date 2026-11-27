use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::Response,
    routing::{get, patch, post},
};
use timetracker_model::{DeviceId, auth::*};
use uuid::Uuid;

use super::{
    ApiError,
    error::{ApiJson, ApiPath},
};
use crate::{
    BackendError,
    auth::{AuthError, AuthService, decode_credential_hash, hash_password, validate_username},
    dashboard::UserSummary,
    repositories::UserRepository,
};

#[derive(Clone)]
pub struct HttpAuthApi {
    pub auth: Arc<AuthService>,
    pub users: Arc<dyn UserRepository>,
}

impl HttpAuthApi {
    pub fn new(auth: Arc<AuthService>, users: Arc<dyn UserRepository>) -> Self {
        Self { auth, users }
    }

    pub fn router(&self) -> Router {
        let private = Router::new()
            .route("/api/auth/me", get(me))
            .route("/api/admins", get(admins).post(create_admin))
            .route("/api/admins/{id}", patch(change_password))
            .route("/api/devices", get(devices).post(approve_device))
            .route("/api/devices/{id}", get(device))
            .route("/users", post(create_user))
            .route_layer(middleware::from_fn_with_state(
                self.auth.clone(),
                authenticate_http,
            ));
        Router::new()
            .merge(private)
            .route("/api/auth/login", post(login_admin))
            .route("/api/devices/login", post(login_device))
            .route("/api/devices/register", post(check_registration))
            .with_state(Arc::new(self.clone()))
    }

    pub async fn login_admin(&self, request: AdminLoginRequest) -> Result<TokenResponse, ApiError> {
        Ok(self.auth.login_admin(request).await?)
    }
    pub async fn login_device(
        &self,
        request: DeviceCredentials,
    ) -> Result<TokenResponse, ApiError> {
        Ok(self.auth.login_device(request).await?)
    }
    pub async fn check_registration(
        &self,
        request: DeviceCredentials,
    ) -> Result<RegistrationResponse, ApiError> {
        Ok(self.auth.check_registration(request).await?)
    }
    pub async fn current_admin(&self, claims: JwtClaims) -> Result<AdminAccount, ApiError> {
        if claims.kind != PrincipalKind::Admin {
            return Err(ApiError::Forbidden);
        }
        let admin = self
            .auth
            .admins
            .find_by_id(claims.sub)
            .await?
            .ok_or(ApiError::Unauthorized)?;
        Ok(AdminAccount {
            id: admin.id,
            username: admin.username,
        })
    }
    pub async fn list_admins(&self) -> Result<Vec<AdminAccount>, ApiError> {
        Ok(self
            .auth
            .admins
            .list()
            .await?
            .into_iter()
            .map(|a| AdminAccount {
                id: a.id,
                username: a.username,
            })
            .collect())
    }
    pub async fn create_admin(&self, request: AdminLoginRequest) -> Result<AdminAccount, ApiError> {
        validate_username(&request.username)?;
        let password_hash = hash_password(request.password).await?;
        let admin = self
            .auth
            .admins
            .create(&request.username, &password_hash)
            .await?;
        Ok(AdminAccount {
            id: admin.id,
            username: admin.username,
        })
    }
    pub async fn change_password(
        &self,
        id: Uuid,
        request: ChangePasswordRequest,
    ) -> Result<(), ApiError> {
        let password_hash = hash_password(request.password).await?;
        self.auth.admins.change_password(id, &password_hash).await?;
        Ok(())
    }
    pub async fn list_devices(&self) -> Result<Vec<DeviceSummary>, ApiError> {
        Ok(self
            .auth
            .devices
            .list()
            .await?
            .into_iter()
            .map(device_summary)
            .collect())
    }
    pub async fn find_device(&self, id: DeviceId) -> Result<DeviceSummary, ApiError> {
        self.auth
            .devices
            .find(id)
            .await?
            .map(device_summary)
            .ok_or_else(|| BackendError::NotFound("Device not found".into()).into())
    }
    pub async fn approve_device(
        &self,
        request: DeviceApprovalRequest,
    ) -> Result<DeviceSummary, ApiError> {
        decode_credential_hash(&request.credential_hash)?;
        self.users
            .find_user(request.user_id)
            .await?
            .ok_or_else(|| BackendError::NotFound("User not found".into()))?;
        Ok(device_summary(
            self.auth.devices.create_approved(request).await?,
        ))
    }
    pub async fn create_user(&self, request: CreateUserRequest) -> Result<UserSummary, ApiError> {
        let name = request.name.trim();
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
            return Err(ApiError::BadRequest(
                "Name must contain 1–255 bytes without control characters".into(),
            ));
        }
        Ok(self.users.create_user(name).await?)
    }
}

fn device_summary(device: crate::auth::DeviceRecord) -> DeviceSummary {
    DeviceSummary {
        device_id: device.device_id,
        user_id: device.user_id,
    }
}

async fn login_admin(
    State(api): State<Arc<HttpAuthApi>>,
    ApiJson(body): ApiJson<AdminLoginRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    Ok(Json(api.login_admin(body).await?))
}
async fn login_device(
    State(api): State<Arc<HttpAuthApi>>,
    ApiJson(body): ApiJson<DeviceCredentials>,
) -> Result<Json<TokenResponse>, ApiError> {
    Ok(Json(api.login_device(body).await?))
}
async fn check_registration(
    State(api): State<Arc<HttpAuthApi>>,
    ApiJson(body): ApiJson<DeviceCredentials>,
) -> Result<Json<RegistrationResponse>, ApiError> {
    Ok(Json(api.check_registration(body).await?))
}
async fn me(
    State(api): State<Arc<HttpAuthApi>>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Json<AdminAccount>, ApiError> {
    Ok(Json(api.current_admin(claims).await?))
}
async fn admins(State(api): State<Arc<HttpAuthApi>>) -> Result<Json<Vec<AdminAccount>>, ApiError> {
    Ok(Json(api.list_admins().await?))
}
async fn create_admin(
    State(api): State<Arc<HttpAuthApi>>,
    ApiJson(body): ApiJson<AdminLoginRequest>,
) -> Result<(StatusCode, Json<AdminAccount>), ApiError> {
    Ok((StatusCode::CREATED, Json(api.create_admin(body).await?)))
}
async fn change_password(
    State(api): State<Arc<HttpAuthApi>>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<ChangePasswordRequest>,
) -> Result<StatusCode, ApiError> {
    api.change_password(id, body).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn devices(
    State(api): State<Arc<HttpAuthApi>>,
) -> Result<Json<Vec<DeviceSummary>>, ApiError> {
    Ok(Json(api.list_devices().await?))
}
async fn device(
    State(api): State<Arc<HttpAuthApi>>,
    ApiPath(id): ApiPath<DeviceId>,
) -> Result<Json<DeviceSummary>, ApiError> {
    Ok(Json(api.find_device(id).await?))
}
async fn approve_device(
    State(api): State<Arc<HttpAuthApi>>,
    ApiJson(body): ApiJson<DeviceApprovalRequest>,
) -> Result<Json<DeviceSummary>, ApiError> {
    Ok(Json(api.approve_device(body).await?))
}
async fn create_user(
    State(api): State<Arc<HttpAuthApi>>,
    ApiJson(body): ApiJson<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserSummary>), ApiError> {
    Ok((StatusCode::CREATED, Json(api.create_user(body).await?)))
}

pub(crate) fn bearer(value: &str) -> Result<&str, AuthError> {
    let (scheme, token) = value.split_once(' ').ok_or(AuthError::InvalidToken)?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.bytes().any(|b| b.is_ascii_whitespace())
    {
        return Err(AuthError::InvalidToken);
    }
    Ok(token)
}

pub(crate) fn http_bearer(headers: &HeaderMap) -> Result<&str, AuthError> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let value = values.next().ok_or(AuthError::InvalidToken)?;
    if values.next().is_some() {
        return Err(AuthError::InvalidToken);
    }
    bearer(value.to_str().map_err(|_| AuthError::InvalidToken)?)
}

pub async fn authenticate_http(
    State(auth): State<Arc<AuthService>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let claims = auth.validate_token(http_bearer(request.headers())?)?;
    if claims.kind != PrincipalKind::Admin {
        return Err(ApiError::Forbidden);
    }
    auth.authorize(&claims).await?;
    request.extensions_mut().insert(claims);
    Ok(next.run(request).await)
}

pub(crate) fn grpc_auth_error(error: AuthError) -> tonic::Status {
    match error {
        AuthError::Forbidden => tonic::Status::permission_denied("Device token required"),
        AuthError::Backend(_) => tonic::Status::internal("Authentication unavailable"),
        _ => tonic::Status::unauthenticated("Invalid credentials or token"),
    }
}

#[allow(clippy::result_large_err, reason = "gRPC uses tonic::Status")]
pub(crate) async fn grpc_identity(
    auth: &AuthService,
    metadata: &tonic::metadata::MetadataMap,
) -> Result<JwtClaims, tonic::Status> {
    let token = grpc_bearer(metadata).map_err(grpc_auth_error)?;
    let claims = auth.validate_token(token).map_err(grpc_auth_error)?;
    if claims.kind != PrincipalKind::Device {
        return Err(grpc_auth_error(AuthError::Forbidden));
    }
    auth.authorize(&claims).await.map_err(grpc_auth_error)?;
    Ok(claims)
}

pub(crate) fn grpc_bearer(metadata: &tonic::metadata::MetadataMap) -> Result<&str, AuthError> {
    let mut values = metadata.get_all("authorization").iter();
    let value = values.next().ok_or(AuthError::InvalidToken)?;
    if values.next().is_some() {
        return Err(AuthError::InvalidToken);
    }
    bearer(value.to_str().map_err(|_| AuthError::InvalidToken)?)
}

#[allow(clippy::result_large_err, reason = "gRPC uses tonic::Status")]
pub async fn authenticate_grpc(
    auth: &AuthService,
    metadata: &tonic::metadata::MetadataMap,
    device_id: DeviceId,
) -> Result<JwtClaims, tonic::Status> {
    let claims = grpc_identity(auth, metadata).await?;
    if claims.sub != device_id.0 {
        return Err(tonic::Status::invalid_argument(
            "device_id must match token subject",
        ));
    }
    Ok(claims)
}

#[cfg(test)]
mod tests;
