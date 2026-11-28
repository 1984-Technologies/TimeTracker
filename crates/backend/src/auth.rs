use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use timetracker_model::auth::{
    AdminLoginRequest, DeviceCredentials, JwtClaims, PrincipalKind, RegistrationResponse,
    TokenResponse,
};

use crate::{
    BackendError,
    repositories::{AdminAccountRepository, DeviceRepository},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminAccountRecord {
    pub id: uuid::Uuid,
    pub username: String,
    pub password_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

pub fn hash_password(password: &str) -> Result<String, BackendError> {
    if password.is_empty() || password.len() > 1024 {
        return Err(BackendError::Validation(
            "password must contain 1–1024 bytes".into(),
        ));
    }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| BackendError::Internal(format!("failed to hash password: {error}")))
}

impl From<BackendError> for AuthError {
    fn from(error: BackendError) -> Self {
        Self::Backend(error)
    }
}

pub fn jwt_key(secret: &str) -> Result<Vec<u8>, BackendError> {
    let key = URL_SAFE_NO_PAD
        .decode(secret)
        .map_err(|_| BackendError::Validation("JWT_SECRET must be unpadded base64url".into()))?;
    if key.len() < 32 {
        return Err(BackendError::Validation(
            "JWT_SECRET must contain at least 32 random bytes".into(),
        ));
    }
    Ok(key)
}

pub fn validate_name(name: &str) -> Result<&str, BackendError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 200 || name.chars().any(char::is_control) {
        return Err(BackendError::Validation(
            "Name must contain 1–200 bytes without control characters".into(),
        ));
    }
    Ok(name)
}

static PASSWORD_JOBS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
static DUMMY_HASH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| hash_password("unknown account").expect("fixed password"));

pub(crate) async fn password_job<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, BackendError> + Send + 'static,
) -> Result<T, BackendError> {
    let permit = PASSWORD_JOBS
        .acquire()
        .await
        .map_err(|_| BackendError::Internal("Password worker unavailable".into()))?;
    // Keep the permit until the blocking work ends, even if its caller is cancelled.
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|_| BackendError::Internal("Password worker failed".into()))?
}

impl AuthService {
    pub fn new(
        admins: Arc<dyn AdminAccountRepository>,
        devices: Arc<dyn DeviceRepository>,
        jwt_secret: String,
    ) -> Self {
        Self {
            admins,
            devices,
            jwt_secret,
        }
    }

    pub async fn login_admin(
        &self,
        request: AdminLoginRequest,
    ) -> Result<TokenResponse, AuthError> {
        let username = validate_name(&request.username)?;
        if request.password.is_empty() || request.password.len() > 1024 {
            return Err(AuthError::InvalidCredentials);
        }
        let admin = self.admins.find_by_username(username).await?;
        let hash = admin.as_ref().map(|admin| admin.password_hash.clone());
        let valid = password_job(move || {
            let hash = hash.as_deref().unwrap_or(&DUMMY_HASH);
            let hash = PasswordHash::new(hash)
                .map_err(|_| BackendError::Internal("Invalid stored password hash".into()))?;
            Ok(Argon2::default()
                .verify_password(request.password.as_bytes(), &hash)
                .is_ok())
        })
        .await?;
        match admin {
            Some(admin) if valid => self.issue(admin.id, PrincipalKind::Admin),
            _ => Err(AuthError::InvalidCredentials),
        }
    }

    pub async fn login_device(
        &self,
        credentials: DeviceCredentials,
    ) -> Result<TokenResponse, AuthError> {
        let id = credentials.device_id;
        if !self.check_registration(credentials).await?.registered {
            return Err(AuthError::InvalidCredentials);
        }
        self.issue(id.0, PrincipalKind::Device)
    }

    // Registration checks and subject lookup come from the local auth-contract-review worktree.
    pub async fn check_registration(
        &self,
        credentials: DeviceCredentials,
    ) -> Result<RegistrationResponse, AuthError> {
        let raw = URL_SAFE_NO_PAD
            .decode(&credentials.secret)
            .map_err(|_| BackendError::Validation("Secret must be unpadded base64url".into()))?;
        if raw.len() != 32 {
            return Err(BackendError::Validation("Secret must contain 32 bytes".into()).into());
        }
        let Some(device) = self.devices.find(credentials.device_id).await? else {
            return Ok(RegistrationResponse { registered: false });
        };
        let hash: String = Sha256::digest(raw)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if !bool::from(hash.as_bytes().ct_eq(device.credential_hash.as_bytes())) {
            return Err(AuthError::InvalidCredentials);
        }
        Ok(RegistrationResponse { registered: true })
    }

    fn issue(&self, sub: uuid::Uuid, kind: PrincipalKind) -> Result<TokenResponse, AuthError> {
        let iat = time::OffsetDateTime::now_utc().unix_timestamp() as u64;
        let claims = JwtClaims {
            sub,
            kind,
            iss: "timetracker".into(),
            aud: "timetracker-api".into(),
            iat,
            exp: iat + 900,
        };
        let access_token = jsonwebtoken::encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&jwt_key(&self.jwt_secret)?),
        )
        .map_err(|_| BackendError::Internal("Token signing failed".into()))?;
        Ok(TokenResponse {
            access_token,
            token_type: "Bearer".into(),
            expires_in: 900,
        })
    }

    pub fn validate_token(&self, token: &str) -> Result<JwtClaims, AuthError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&["timetracker"]);
        validation.set_audience(&["timetracker-api"]);
        validation.leeway = 0;
        let claims = jsonwebtoken::decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(&jwt_key(&self.jwt_secret)?),
            &validation,
        )
        .map_err(|_| AuthError::InvalidToken)?
        .claims;
        let now = time::OffsetDateTime::now_utc().unix_timestamp() as u64;
        if claims.iat > now || claims.exp <= now || claims.iat.checked_add(900) != Some(claims.exp)
        {
            return Err(AuthError::InvalidToken);
        }
        Ok(claims)
    }

    pub async fn authorize(&self, claims: &JwtClaims) -> Result<(), AuthError> {
        let exists = match claims.kind {
            PrincipalKind::Admin => self.admins.find_by_id(claims.sub).await?.is_some(),
            PrincipalKind::Device => self
                .devices
                .find(timetracker_model::DeviceId(claims.sub))
                .await?
                .is_some(),
        };
        if exists {
            Ok(())
        } else {
            Err(AuthError::InvalidToken)
        }
    }
}
