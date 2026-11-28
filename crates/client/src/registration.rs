use std::{
    env,
    fmt::Write as _,
    io,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use timetracker_model::{
    DeviceId,
    auth::{DeviceCredentials, RegistrationResponse, TokenResponse},
};
use tokio::sync::Mutex;
use tonic::metadata::MetadataValue;

use crate::{ClientError, identity::InstallationIdentity};

pub const DEFAULT_HTTP_BASE_URL: &str = "http://localhost:8239";
const KEYRING_SERVICE: &str = "timetracker-client";
const SECRET_LEN_BYTES: usize = 32;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_EXPIRY_LEEWAY: Duration = Duration::from_secs(30);

pub fn http_base_url() -> String {
    env::var("TIMETRACKER_HTTP_URL")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_HTTP_BASE_URL.to_owned())
}

trait SecretStore {
    fn get_secret(&self, account: &str) -> Result<Option<String>, ClientError>;
    fn set_secret(&self, account: &str, secret: &str) -> Result<(), ClientError>;
}

struct KeyringSecretStore {
    service: &'static str,
}

impl SecretStore for KeyringSecretStore {
    fn get_secret(&self, account: &str) -> Result<Option<String>, ClientError> {
        let entry = keyring::Entry::new(self.service, account)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn set_secret(&self, account: &str, secret: &str) -> Result<(), ClientError> {
        let entry = keyring::Entry::new(self.service, account)?;
        entry.set_password(secret)?;
        Ok(())
    }
}

pub struct DeviceCredentialStore;

impl DeviceCredentialStore {
    pub fn load_or_create(&self) -> Result<DeviceCredentials, ClientError> {
        let device_id = InstallationIdentity.load_or_create()?;
        let store = KeyringSecretStore {
            service: KEYRING_SERVICE,
        };
        load_or_create_credentials(device_id, &store)
    }
}

fn load_or_create_credentials(
    device_id: DeviceId,
    store: &dyn SecretStore,
) -> Result<DeviceCredentials, ClientError> {
    let account = device_id.0.as_hyphenated().to_string();

    if let Some(existing) = store.get_secret(&account)? {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            decode_secret(trimmed)?;
            return Ok(DeviceCredentials {
                device_id,
                secret: trimmed.to_owned(),
            });
        }
    }

    let mut raw_secret = [0u8; SECRET_LEN_BYTES];
    OsRng.fill_bytes(&mut raw_secret);
    let encoded = URL_SAFE_NO_PAD.encode(raw_secret);

    store.set_secret(&account, &encoded)?;

    Ok(DeviceCredentials {
        device_id,
        secret: encoded,
    })
}

fn decode_secret(secret: &str) -> Result<Vec<u8>, ClientError> {
    let decoded = URL_SAFE_NO_PAD.decode(secret).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid base64url device secret: {error}"),
        )
    })?;

    if decoded.len() != SECRET_LEN_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "expected {SECRET_LEN_BYTES}-byte device secret, got {} bytes",
                decoded.len()
            ),
        )
        .into());
    }

    Ok(decoded)
}

pub struct HttpAuthClient {
    pub base_url: String,
    client: reqwest::Client,
}

impl HttpAuthClient {
    pub fn new(base_url: String) -> Self {
        let normalized = base_url.trim_end_matches('/').to_owned();
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("failed to build HTTP client with TLS certificate validation");

        Self {
            base_url: normalized,
            client,
        }
    }

    pub fn from_env() -> Self {
        Self::new(http_base_url())
    }

    pub async fn check_registration(
        &self,
        credentials: &DeviceCredentials,
    ) -> Result<RegistrationResponse, ClientError> {
        let url = format!("{}/api/devices/register", self.base_url);
        let response = self
            .client
            .post(url)
            .json(credentials)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json::<RegistrationResponse>().await?)
    }

    pub async fn login(
        &self,
        credentials: &DeviceCredentials,
    ) -> Result<TokenResponse, ClientError> {
        let url = format!("{}/api/devices/login", self.base_url);
        let response = self
            .client
            .post(url)
            .json(credentials)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json::<TokenResponse>().await?)
    }
}

struct CachedToken {
    access_token: String,
    refresh_at: Instant,
}

pub struct ClientAuthService {
    pub http: HttpAuthClient,
    pub credentials: DeviceCredentials,
    token_cache: Mutex<Option<CachedToken>>,
}

impl ClientAuthService {
    pub fn new(http: HttpAuthClient, credentials: DeviceCredentials) -> Self {
        Self {
            http,
            credentials,
            token_cache: Mutex::new(None),
        }
    }

    pub fn device_id(&self) -> DeviceId {
        self.credentials.device_id
    }

    pub fn credential_hash(&self) -> Result<String, ClientError> {
        let secret_bytes = decode_secret(&self.credentials.secret)?;
        let digest = Sha256::digest(&secret_bytes);

        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            let _ = write!(&mut hex, "{byte:02x}");
        }
        Ok(hex)
    }

    pub async fn check_registration(&self) -> Result<RegistrationResponse, ClientError> {
        self.http.check_registration(&self.credentials).await
    }

    pub async fn access_token(&self) -> Result<String, ClientError> {
        let mut cache = self.token_cache.lock().await;
        let now = Instant::now();

        if let Some(cached) = cache.as_ref()
            && now < cached.refresh_at
        {
            return Ok(cached.access_token.clone());
        }

        let response = self.http.login(&self.credentials).await?;
        let ttl = Duration::from_secs(response.expires_in);
        let leeway = MAX_EXPIRY_LEEWAY.min(ttl / 4);
        let refresh_at = Instant::now() + ttl.saturating_sub(leeway);

        let token = response.access_token.clone();
        *cache = Some(CachedToken {
            access_token: response.access_token,
            refresh_at,
        });

        Ok(token)
    }

    pub async fn invalidate_token(&self) -> Result<(), ClientError> {
        let mut cache = self.token_cache.lock().await;
        *cache = None;
        Ok(())
    }
}

pub struct GrpcAuthInterceptor {
    pub token: String,
}

impl tonic::service::Interceptor for GrpcAuthInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        let header_value = format!("Bearer {}", self.token);
        let metadata_value = MetadataValue::try_from(header_value).map_err(|error| {
            tonic::Status::invalid_argument(format!("invalid bearer token metadata: {error}"))
        })?;
        request
            .metadata_mut()
            .insert("authorization", metadata_value);
        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
    use tokio::net::TcpListener;
    use tonic::service::Interceptor;
    use uuid::Uuid;

    use super::*;

    #[derive(Default)]
    struct InMemorySecretStore {
        entries: StdMutex<HashMap<String, String>>,
    }

    impl SecretStore for InMemorySecretStore {
        fn get_secret(&self, account: &str) -> Result<Option<String>, ClientError> {
            Ok(self.entries.lock().unwrap().get(account).cloned())
        }

        fn set_secret(&self, account: &str, secret: &str) -> Result<(), ClientError> {
            self.entries
                .lock()
                .unwrap()
                .insert(account.to_owned(), secret.to_owned());
            Ok(())
        }
    }

    #[test]
    fn credential_persistence_reuses_secret_and_uses_unpadded_base64url() {
        let device_id = DeviceId(Uuid::new_v4());
        let store = InMemorySecretStore::default();

        let first = load_or_create_credentials(device_id, &store).unwrap();
        let second = load_or_create_credentials(device_id, &store).unwrap();

        assert_eq!(first.device_id, device_id);
        assert_eq!(second.device_id, device_id);
        assert_eq!(first.secret, second.secret);
        assert!(!first.secret.contains('='));
        assert!(!first.secret.contains('+'));
        assert!(!first.secret.contains('/'));
        assert_eq!(URL_SAFE_NO_PAD.decode(&first.secret).unwrap().len(), 32);
    }

    #[test]
    fn credential_hash_is_lowercase_sha256_hex_of_decoded_secret() {
        let raw_secret = [0xABu8; 32];
        let credentials = DeviceCredentials {
            device_id: DeviceId(Uuid::new_v4()),
            secret: URL_SAFE_NO_PAD.encode(raw_secret),
        };
        let service = ClientAuthService::new(
            HttpAuthClient::new(DEFAULT_HTTP_BASE_URL.to_owned()),
            credentials,
        );

        let hash = service.credential_hash().unwrap();
        let expected = {
            let digest = Sha256::digest(raw_secret);
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };

        assert_eq!(hash.len(), 64);
        assert_eq!(hash, expected);
        assert_eq!(hash, hash.to_lowercase());
    }

    #[test]
    fn grpc_auth_interceptor_attaches_bearer_metadata() {
        let mut interceptor = GrpcAuthInterceptor {
            token: "test-jwt-token".to_owned(),
        };

        let request = interceptor.call(tonic::Request::new(())).unwrap();
        let auth = request
            .metadata()
            .get("authorization")
            .expect("authorization metadata missing")
            .to_str()
            .unwrap();

        assert_eq!(auth, "Bearer test-jwt-token");
    }

    #[derive(Clone)]
    struct MockAuthState {
        register_calls: Arc<AtomicUsize>,
        login_calls: Arc<AtomicUsize>,
        expires_in: u64,
        login_delay: Duration,
    }

    async fn spawn_mock_auth_server(state: MockAuthState) -> String {
        let app =
            Router::new()
                .route(
                    "/api/devices/register",
                    post(
                        |State(state): State<MockAuthState>,
                         Json(creds): Json<DeviceCredentials>| async move {
                            assert!(!creds.secret.contains('='));
                            state.register_calls.fetch_add(1, Ordering::SeqCst);
                            (
                                StatusCode::OK,
                                Json(RegistrationResponse { registered: true }),
                            )
                        },
                    ),
                )
                .route(
                    "/api/devices/login",
                    post(
                        |State(state): State<MockAuthState>,
                         Json(creds): Json<DeviceCredentials>| async move {
                            assert!(!creds.secret.contains('='));
                            if !state.login_delay.is_zero() {
                                tokio::time::sleep(state.login_delay).await;
                            }
                            let n = state.login_calls.fetch_add(1, Ordering::SeqCst) + 1;
                            (
                                StatusCode::OK,
                                Json(TokenResponse {
                                    access_token: format!("jwt-{n}"),
                                    token_type: "Bearer".to_owned(),
                                    expires_in: state.expires_in,
                                }),
                            )
                        },
                    ),
                )
                .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    fn sample_credentials() -> DeviceCredentials {
        DeviceCredentials {
            device_id: DeviceId(Uuid::new_v4()),
            secret: URL_SAFE_NO_PAD.encode([7u8; 32]),
        }
    }

    #[tokio::test]
    async fn http_auth_client_handles_register_and_login() {
        let state = MockAuthState {
            register_calls: Arc::new(AtomicUsize::new(0)),
            login_calls: Arc::new(AtomicUsize::new(0)),
            expires_in: 300,
            login_delay: Duration::ZERO,
        };
        let base_url = spawn_mock_auth_server(state.clone()).await;
        let client = HttpAuthClient::new(base_url);
        let creds = sample_credentials();

        let reg = client.check_registration(&creds).await.unwrap();
        assert!(reg.registered);
        assert_eq!(state.register_calls.load(Ordering::SeqCst), 1);

        let token = client.login(&creds).await.unwrap();
        assert_eq!(token.access_token, "jwt-1");
        assert_eq!(token.expires_in, 300);
        assert_eq!(state.login_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn client_auth_service_caches_invalidates_and_renews_on_expiry() {
        let state = MockAuthState {
            register_calls: Arc::new(AtomicUsize::new(0)),
            login_calls: Arc::new(AtomicUsize::new(0)),
            expires_in: 1,
            login_delay: Duration::ZERO,
        };
        let base_url = spawn_mock_auth_server(state.clone()).await;
        let service = ClientAuthService::new(HttpAuthClient::new(base_url), sample_credentials());

        let t1 = service.access_token().await.unwrap();
        let t2 = service.access_token().await.unwrap();
        assert_eq!(t1, "jwt-1");
        assert_eq!(t2, "jwt-1");
        assert_eq!(state.login_calls.load(Ordering::SeqCst), 1);

        // Wait for 1s TTL (minus 250ms leeway = 750ms) to expire
        tokio::time::sleep(Duration::from_millis(850)).await;
        let t3 = service.access_token().await.unwrap();
        assert_eq!(t3, "jwt-2");
        assert_eq!(state.login_calls.load(Ordering::SeqCst), 2);

        // Explicit invalidation forces a fresh login
        service.invalidate_token().await.unwrap();
        let t4 = service.access_token().await.unwrap();
        assert_eq!(t4, "jwt-3");
        assert_eq!(state.login_calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn client_auth_service_coordinates_concurrent_callers() {
        let state = MockAuthState {
            register_calls: Arc::new(AtomicUsize::new(0)),
            login_calls: Arc::new(AtomicUsize::new(0)),
            expires_in: 300,
            login_delay: Duration::from_millis(50),
        };
        let base_url = spawn_mock_auth_server(state.clone()).await;
        let service = Arc::new(ClientAuthService::new(
            HttpAuthClient::new(base_url),
            sample_credentials(),
        ));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let svc = Arc::clone(&service);
            handles.push(tokio::spawn(
                async move { svc.access_token().await.unwrap() },
            ));
        }

        for handle in handles {
            assert_eq!(handle.await.unwrap(), "jwt-1");
        }
        assert_eq!(state.login_calls.load(Ordering::SeqCst), 1);
    }
}
