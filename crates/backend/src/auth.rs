use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use timetracker_model::{
    DeviceId,
    auth::{
        AdminLoginRequest, DeviceCredentials, JwtClaims, PrincipalKind, RegistrationResponse,
        TokenResponse,
    },
};

use crate::{
    BackendError,
    repositories::{AdminAccountRepository, DeviceRepository},
};

pub const TOKEN_LIFETIME_SECONDS: u64 = 900;
pub const JWT_ISSUER: &str = "timetracker";
pub const JWT_AUDIENCE: &str = "timetracker-api";
const LOGIN_WINDOW: Duration = Duration::from_secs(60);
const LOGIN_LIMIT: u32 = 20;

#[derive(Clone)]
pub struct AdminAccountRecord {
    pub id: uuid::Uuid,
    pub username: String,
    pub password_hash: String,
}

#[derive(Clone)]
pub struct DeviceRecord {
    pub device_id: DeviceId,
    pub user_id: timetracker_model::UserId,
    pub credential_hash: String,
}

#[derive(Debug)]
pub enum AuthError {
    InvalidCredentials,
    InvalidToken,
    Forbidden,
    RateLimited,
    Backend(BackendError),
}

impl From<BackendError> for AuthError {
    fn from(error: BackendError) -> Self {
        Self::Backend(error)
    }
}

struct LoginBudget {
    start: Instant,
    attempts: u32,
}

impl LoginBudget {
    fn acquire(&mut self, now: Instant) -> Result<(), AuthError> {
        if now.duration_since(self.start) >= LOGIN_WINDOW {
            self.start = now;
            self.attempts = 0;
        }
        if self.attempts >= LOGIN_LIMIT {
            return Err(AuthError::RateLimited);
        }
        self.attempts += 1;
        Ok(())
    }
}

pub struct AuthService {
    pub admins: Arc<dyn AdminAccountRepository>,
    pub devices: Arc<dyn DeviceRepository>,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    validation: Validation,
    login_budget: Mutex<LoginBudget>,
    password_workers: Arc<tokio::sync::Semaphore>,
}

impl AuthService {
    pub fn new(
        admins: Arc<dyn AdminAccountRepository>,
        devices: Arc<dyn DeviceRepository>,
        jwt_secret: String,
    ) -> Result<Self, BackendError> {
        let secret = URL_SAFE_NO_PAD.decode(jwt_secret).map_err(|_| {
            BackendError::Validation(
                "JWT_SECRET must be unpadded base64url encoding of at least 32 random bytes".into(),
            )
        })?;
        if secret.len() < 32 {
            return Err(BackendError::Validation(
                "JWT_SECRET must contain at least 32 random bytes".into(),
            ));
        }
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 0;
        validation.validate_nbf = true;
        validation.set_issuer(&[JWT_ISSUER]);
        validation.set_audience(&[JWT_AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        Ok(Self {
            admins,
            devices,
            encoding_key: EncodingKey::from_secret(&secret),
            decoding_key: DecodingKey::from_secret(&secret),
            validation,
            login_budget: Mutex::new(LoginBudget {
                start: Instant::now(),
                attempts: 0,
            }),
            password_workers: Arc::new(tokio::sync::Semaphore::new(4)),
        })
    }

    fn limit_login(&self) -> Result<(), AuthError> {
        self.login_budget
            .lock()
            .map_err(|_| BackendError::Internal("Login limiter unavailable".into()))?
            .acquire(Instant::now())
    }

    pub async fn ensure_initial_admin(
        &self,
        request: AdminLoginRequest,
    ) -> Result<(), BackendError> {
        validate_username(&request.username)?;
        let hash = hash_password(request.password).await?;
        self.admins
            .ensure_initial_account(&request.username, &hash)
            .await
    }

    pub async fn login_admin(
        &self,
        request: AdminLoginRequest,
    ) -> Result<TokenResponse, AuthError> {
        self.limit_login()?;
        validate_username(&request.username)?;
        validate_password(&request.password)?;
        let admin = self
            .admins
            .find_by_username(&request.username)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        let id = admin.id;
        let permit = self
            .password_workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| BackendError::Internal("Password verifier unavailable".into()))?;
        let valid = tokio::task::spawn_blocking(move || {
            // Keep the permit until the blocking verifier finishes, including
            // when the HTTP request is cancelled while verification is running.
            let _permit = permit;
            PasswordHash::new(&admin.password_hash).is_ok_and(|hash| {
                hash.algorithm.as_str() == "argon2id"
                    && Argon2::default()
                        .verify_password(request.password.as_bytes(), &hash)
                        .is_ok()
            })
        })
        .await
        .map_err(|_| BackendError::Internal("Password verification failed".into()))?;
        if !valid {
            return Err(AuthError::InvalidCredentials);
        }
        self.issue_token(id, PrincipalKind::Admin)
    }

    pub async fn login_device(
        &self,
        credentials: DeviceCredentials,
    ) -> Result<TokenResponse, AuthError> {
        self.limit_login()?;
        let digest = credential_digest(&credentials.secret)?;
        let device = self
            .devices
            .find(credentials.device_id)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        verify_device_secret(&digest, &device.credential_hash)?;
        self.issue_token(device.device_id.0, PrincipalKind::Device)
    }

    pub async fn check_registration(
        &self,
        credentials: DeviceCredentials,
    ) -> Result<RegistrationResponse, AuthError> {
        let digest = credential_digest(&credentials.secret)?;
        let device = self.devices.find(credentials.device_id).await?;
        if let Some(device) = device {
            verify_device_secret(&digest, &device.credential_hash)?;
            Ok(RegistrationResponse { registered: true })
        } else {
            Ok(RegistrationResponse { registered: false })
        }
    }

    fn issue_token(
        &self,
        sub: uuid::Uuid,
        kind: PrincipalKind,
    ) -> Result<TokenResponse, AuthError> {
        let iat = jsonwebtoken::get_current_timestamp();
        let claims = JwtClaims {
            sub,
            kind,
            iss: JWT_ISSUER.into(),
            aud: JWT_AUDIENCE.into(),
            iat,
            exp: iat + TOKEN_LIFETIME_SECONDS,
        };
        let access_token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding_key)
            .map_err(|_| BackendError::Internal("Token issuance failed".into()))?;
        Ok(TokenResponse {
            access_token,
            token_type: "Bearer".into(),
            expires_in: TOKEN_LIFETIME_SECONDS,
        })
    }

    pub fn validate_token(&self, token: &str) -> Result<JwtClaims, AuthError> {
        let claims = decode::<JwtClaims>(token, &self.decoding_key, &self.validation)
            .map_err(|_| AuthError::InvalidToken)?
            .claims;
        let now = jsonwebtoken::get_current_timestamp();
        if claims.exp <= now || claims.iat > now || claims.exp <= claims.iat {
            return Err(AuthError::InvalidToken);
        }
        Ok(claims)
    }

    pub async fn authorize(&self, claims: &JwtClaims) -> Result<(), AuthError> {
        let exists = match claims.kind {
            PrincipalKind::Admin => self.admins.find_by_id(claims.sub).await?.is_some(),
            PrincipalKind::Device => self.devices.find(DeviceId(claims.sub)).await?.is_some(),
        };
        if exists {
            Ok(())
        } else {
            Err(AuthError::InvalidToken)
        }
    }
}

pub(crate) fn validate_username(username: &str) -> Result<(), BackendError> {
    if username.trim().is_empty() || username.len() > 255 || username.chars().any(char::is_control)
    {
        return Err(BackendError::Validation(
            "Username must contain 1–255 bytes without control characters".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_password(password: &str) -> Result<(), BackendError> {
    if password.is_empty() || password.len() > 1024 {
        return Err(BackendError::Validation(
            "Password must contain 1–1024 bytes".into(),
        ));
    }
    Ok(())
}

pub async fn hash_password(password: String) -> Result<String, BackendError> {
    validate_password(&password)?;
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| BackendError::Internal("Password hashing failed".into()))
    })
    .await
    .map_err(|_| BackendError::Internal("Password hashing failed".into()))?
}

pub(crate) fn credential_digest(secret: &str) -> Result<[u8; 32], BackendError> {
    let bytes = URL_SAFE_NO_PAD.decode(secret).map_err(|_| {
        BackendError::Validation("Device secret must use unpadded base64url".into())
    })?;
    if bytes.len() != 32 {
        return Err(BackendError::Validation(
            "Device secret must contain 32 bytes".into(),
        ));
    }
    Ok(Sha256::digest(bytes).into())
}

pub(crate) fn decode_credential_hash(hash: &str) -> Result<[u8; 32], BackendError> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(BackendError::Validation(
            "credential_hash must contain 64 lowercase hex digits".into(),
        ));
    }
    let mut result = [0u8; 32];
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hash[2 * i..2 * i + 2], 16)
            .map_err(|_| BackendError::Validation("Invalid credential hash".into()))?;
    }
    Ok(result)
}

pub(crate) fn verify_device_secret(digest: &[u8; 32], hash: &str) -> Result<(), AuthError> {
    let stored = decode_credential_hash(hash).map_err(|_| AuthError::InvalidCredentials)?;
    if bool::from(digest.ct_eq(&stored)) {
        Ok(())
    } else {
        Err(AuthError::InvalidCredentials)
    }
}

#[cfg(test)]
mod tests;
