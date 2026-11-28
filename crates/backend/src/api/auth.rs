use super::ApiError;
use crate::{
    BackendError,
    auth::{AuthError, AuthService, hash_password, password_job, validate_name},
    dashboard::UserSummary,
    repositories::UserRepository,
};
use axum::{
    Extension, Json, Router,
    extract::{ConnectInfo, Path, Request, State, rejection::JsonRejection},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use timetracker_model::{DeviceId, auth::*};
use uuid::Uuid;

type JsonBody<T> = Result<Json<T>, JsonRejection>;
fn body<T>(value: JsonBody<T>) -> Result<T, ApiError> {
    value
        .map(|Json(value)| value)
        .map_err(|error| ApiError::BadRequest(error.body_text()))
}
fn id(value: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value).map_err(|_| ApiError::BadRequest("Invalid UUID".into()))
}
fn summary(record: crate::auth::DeviceRecord) -> DeviceSummary {
    DeviceSummary {
        device_id: record.device_id,
        user_id: record.user_id,
    }
}

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
            .route("/api/auth/me", get(|State(api): State<Arc<Self>>, Extension(claims): Extension<JwtClaims>| async move { api.current_admin(claims).await.map(Json) }))
            .route("/api/admins", get(|State(api): State<Arc<Self>>| async move { api.list_admins().await.map(Json) })
                .post(|State(api): State<Arc<Self>>, data: JsonBody<AdminLoginRequest>| async move { Ok::<_, ApiError>((StatusCode::CREATED, Json(api.create_admin(body(data)?).await?))) }))
            .route("/api/admins/{id}", patch(|State(api): State<Arc<Self>>, Path(value): Path<String>, data: JsonBody<ChangePasswordRequest>| async move { api.change_password(id(&value)?, body(data)?).await?; Ok::<_, ApiError>(StatusCode::NO_CONTENT) }))
            .route("/api/devices", get(|State(api): State<Arc<Self>>| async move { api.list_devices().await.map(Json) })
                .post(|State(api): State<Arc<Self>>, data: JsonBody<DeviceApprovalRequest>| async move { api.approve_device(body(data)?).await.map(Json) }))
            .route("/api/devices/{id}", get(|State(api): State<Arc<Self>>, Path(value): Path<String>| async move { api.find_device(DeviceId(id(&value)?)).await.map(Json) }))
            .route("/users", post(|State(api): State<Arc<Self>>, data: JsonBody<CreateUserRequest>| async move { Ok::<_, ApiError>((StatusCode::CREATED, Json(api.create_user(body(data)?).await?))) }))
            .route_layer(middleware::from_fn_with_state(self.auth.clone(), authenticate_http));
        Router::new()
            .route(
                "/api/auth/login",
                post(
                    |State(api): State<Arc<Self>>, data: JsonBody<AdminLoginRequest>| async move {
                        api.login_admin(body(data)?).await.map(Json)
                    },
                ),
            )
            .route(
                "/api/devices/login",
                post(
                    |State(api): State<Arc<Self>>, data: JsonBody<DeviceCredentials>| async move {
                        api.login_device(body(data)?).await.map(Json)
                    },
                ),
            )
            .route_layer(middleware::from_fn_with_state(
                Arc::new(Mutex::new(HashMap::new())),
                limit_login,
            ))
            .route(
                "/api/devices/register",
                post(
                    |State(api): State<Arc<Self>>, data: JsonBody<DeviceCredentials>| async move {
                        api.check_registration(body(data)?).await.map(Json)
                    },
                ),
            )
            .merge(private)
            .with_state(Arc::new(self.clone()))
    }
    pub async fn login_admin(&self, request: AdminLoginRequest) -> Result<TokenResponse, ApiError> {
        self.auth.login_admin(request).await.map_err(Into::into)
    }
    pub async fn login_device(
        &self,
        request: DeviceCredentials,
    ) -> Result<TokenResponse, ApiError> {
        self.auth.login_device(request).await.map_err(Into::into)
    }
    pub async fn check_registration(
        &self,
        request: DeviceCredentials,
    ) -> Result<RegistrationResponse, ApiError> {
        self.auth
            .check_registration(request)
            .await
            .map_err(Into::into)
    }
    pub async fn current_admin(&self, claims: JwtClaims) -> Result<AdminAccount, ApiError> {
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
            .map(|admin| AdminAccount {
                id: admin.id,
                username: admin.username,
            })
            .collect())
    }
    pub async fn create_admin(&self, request: AdminLoginRequest) -> Result<AdminAccount, ApiError> {
        let name = validate_name(&request.username)?;
        let hash = password_job(move || hash_password(&request.password)).await?;
        let admin = self.auth.admins.create(name, &hash).await?;
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
        let hash = password_job(move || hash_password(&request.password)).await?;
        self.auth.admins.change_password(id, &hash).await?;
        Ok(())
    }
    pub async fn list_devices(&self) -> Result<Vec<DeviceSummary>, ApiError> {
        Ok(self
            .auth
            .devices
            .list()
            .await?
            .into_iter()
            .map(summary)
            .collect())
    }
    pub async fn find_device(&self, id: DeviceId) -> Result<DeviceSummary, ApiError> {
        self.auth
            .devices
            .find(id)
            .await?
            .map(summary)
            .ok_or_else(|| BackendError::NotFound("Device not found".into()).into())
    }
    pub async fn approve_device(
        &self,
        request: DeviceApprovalRequest,
    ) -> Result<DeviceSummary, ApiError> {
        if request.credential_hash.len() != 64
            || !request
                .credential_hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ApiError::BadRequest(
                "Credential hash must contain 64 lowercase hex characters".into(),
            ));
        }
        if self.users.find_user(request.user_id).await?.is_none() {
            return Err(BackendError::NotFound("User not found".into()).into());
        }
        Ok(summary(self.auth.devices.create_approved(request).await?))
    }
    pub async fn create_user(&self, request: CreateUserRequest) -> Result<UserSummary, ApiError> {
        Ok(self
            .users
            .create_user(validate_name(&request.name)?)
            .await?)
    }
}

pub async fn authenticate_http(
    State(auth): State<Arc<AuthService>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;
    let claims = auth.validate_token(token)?;
    if claims.kind != PrincipalKind::Admin {
        return Err(ApiError::Forbidden);
    }
    auth.authorize(&claims).await?;
    request.extensions_mut().insert(claims);
    Ok(next.run(request).await)
}

#[allow(
    clippy::result_large_err,
    reason = "gRPC transport errors use tonic::Status"
)]
pub async fn authenticate_grpc(
    auth: &AuthService,
    metadata: &tonic::metadata::MetadataMap,
    device_id: DeviceId,
) -> Result<JwtClaims, tonic::Status> {
    fn failure(error: AuthError) -> tonic::Status {
        match error {
            AuthError::Forbidden => tonic::Status::permission_denied("Device token required"),
            AuthError::Backend(_) => tonic::Status::internal("Authentication unavailable"),
            _ => tonic::Status::unauthenticated("Invalid credentials or token"),
        }
    }
    let token = metadata
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| tonic::Status::unauthenticated("Device token required"))?;
    let claims = auth.validate_token(token).map_err(failure)?;
    if claims.kind != PrincipalKind::Device {
        return Err(tonic::Status::permission_denied("Device token required"));
    }
    auth.authorize(&claims).await.map_err(failure)?;
    if claims.sub != device_id.0 {
        return Err(tonic::Status::invalid_argument(
            "Device does not match token subject",
        ));
    }
    Ok(claims)
}

type LoginLimits = Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>;
async fn limit_login(
    State(limits): State<LoginLimits>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0.ip())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]));
    let limited = {
        let mut limits = limits
            .lock()
            .map_err(|_| BackendError::Internal("Login limiter unavailable".into()))?;
        limits.retain(|_, (start, _)| start.elapsed() < Duration::from_secs(60));
        if !limits.contains_key(&peer) && limits.len() >= 4096 {
            true
        } else {
            let (_, count) = limits.entry(peer).or_insert((Instant::now(), 0));
            *count = count.saturating_add(1);
            *count > 30
        }
    };
    if limited {
        return Ok((StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({"code":"too_many_requests","message":"Too many login attempts; retry in one minute"}))).into_response());
    }
    Ok(next.run(request).await)
}
