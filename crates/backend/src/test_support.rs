//! Shared in-memory repositories for service and API boundary tests.
use crate::{
    BackendError,
    auth::{AdminAccountRecord, AuthService, DeviceRecord, hash_password},
    dashboard::{TimeRange, UserAction, UserSummary},
    repositories::*,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use time::OffsetDateTime;
use timetracker_model::{auth::*, *};
use uuid::Uuid;

pub const ADMIN_ID: Uuid = Uuid::from_u128(1);
pub const DEVICE_ID: DeviceId = DeviceId(Uuid::from_u128(2));
pub const USER_ID: UserId = UserId(Uuid::from_u128(3));
pub const JWT_KEY: [u8; 32] = [7; 32];

#[derive(Default)]
pub struct Store {
    pub admins: Mutex<HashMap<Uuid, AdminAccountRecord>>,
    pub devices: Mutex<HashMap<DeviceId, DeviceRecord>>,
    pub users: Mutex<HashMap<UserId, UserSummary>>,
    pub events: Mutex<HashMap<(DeviceId, EventId), Event>>,
    pub writes: AtomicUsize,
    pub fail: AtomicBool,
}

impl Store {
    fn check(&self) -> Result<(), BackendError> {
        if self.fail.load(Ordering::SeqCst) {
            Err(BackendError::Internal("sensitive repository error".into()))
        } else {
            Ok(())
        }
    }
}

pub fn auth(store: Arc<Store>) -> Arc<AuthService> {
    Arc::new(AuthService::new(store.clone(), store, URL_SAFE_NO_PAD.encode(JWT_KEY)).unwrap())
}

pub fn token(sub: Uuid, kind: PrincipalKind) -> String {
    let now = jsonwebtoken::get_current_timestamp();
    sign(
        serde_json::json!({"sub":sub, "kind":kind, "iss":"timetracker", "aud":"timetracker-api", "iat":now, "exp":now+900}),
        Algorithm::HS256,
        &JWT_KEY,
    )
}

pub fn sign(claims: serde_json::Value, algorithm: Algorithm, key: &[u8]) -> String {
    jsonwebtoken::encode(
        &Header::new(algorithm),
        &claims,
        &EncodingKey::from_secret(key),
    )
    .unwrap()
}

pub async fn fixture() -> (Arc<Store>, Arc<AuthService>) {
    static HASH: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
    let hash = HASH
        .get_or_init(|| async { hash_password("correct-password".into()).await.unwrap() })
        .await;
    let store = Arc::new(Store::default());
    store.admins.lock().unwrap().insert(
        ADMIN_ID,
        AdminAccountRecord {
            id: ADMIN_ID,
            username: "admin".into(),
            password_hash: hash.clone(),
        },
    );
    store.users.lock().unwrap().insert(
        USER_ID,
        UserSummary {
            id: USER_ID,
            name: "Tracked User".into(),
        },
    );
    use sha2::{Digest, Sha256};
    store.devices.lock().unwrap().insert(
        DEVICE_ID,
        DeviceRecord {
            device_id: DEVICE_ID,
            user_id: USER_ID,
            credential_hash: format!("{:x}", Sha256::digest([3; 32])),
        },
    );
    let service = auth(store.clone());
    (store, service)
}

#[async_trait]
impl AdminAccountRepository for Store {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<AdminAccountRecord>, BackendError> {
        self.check()?;
        Ok(self.admins.lock().unwrap().get(&id).cloned())
    }
    async fn find_by_username(
        &self,
        username: &str,
    ) -> Result<Option<AdminAccountRecord>, BackendError> {
        self.check()?;
        Ok(self
            .admins
            .lock()
            .unwrap()
            .values()
            .find(|a| a.username == username)
            .cloned())
    }
    async fn list(&self) -> Result<Vec<AdminAccountRecord>, BackendError> {
        self.check()?;
        Ok(self.admins.lock().unwrap().values().cloned().collect())
    }
    async fn create(&self, username: &str, hash: &str) -> Result<AdminAccountRecord, BackendError> {
        self.check()?;
        let mut admins = self.admins.lock().unwrap();
        if admins.values().any(|a| a.username == username) {
            return Err(BackendError::Conflict("Username exists".into()));
        }
        let admin = AdminAccountRecord {
            id: Uuid::new_v4(),
            username: username.into(),
            password_hash: hash.into(),
        };
        admins.insert(admin.id, admin.clone());
        Ok(admin)
    }
    async fn change_password(&self, id: Uuid, hash: &str) -> Result<(), BackendError> {
        self.check()?;
        let mut admins = self.admins.lock().unwrap();
        admins
            .get_mut(&id)
            .ok_or_else(|| BackendError::NotFound("Admin not found".into()))?
            .password_hash = hash.into();
        Ok(())
    }
    async fn ensure_initial_account(&self, username: &str, hash: &str) -> Result<(), BackendError> {
        if self.admins.lock().unwrap().is_empty() {
            self.create(username, hash).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl DeviceRepository for Store {
    async fn find(&self, id: DeviceId) -> Result<Option<DeviceRecord>, BackendError> {
        self.check()?;
        Ok(self.devices.lock().unwrap().get(&id).cloned())
    }
    async fn list(&self) -> Result<Vec<DeviceRecord>, BackendError> {
        self.check()?;
        Ok(self.devices.lock().unwrap().values().cloned().collect())
    }
    async fn create_approved(
        &self,
        request: DeviceApprovalRequest,
    ) -> Result<DeviceRecord, BackendError> {
        self.check()?;
        let mut devices = self.devices.lock().unwrap();
        if let Some(existing) = devices.get(&request.device_id) {
            if existing.user_id != request.user_id
                || existing.credential_hash != request.credential_hash
            {
                return Err(BackendError::Conflict("Device approval conflicts".into()));
            }
            return Ok(existing.clone());
        }
        self.writes.fetch_add(1, Ordering::SeqCst);
        let device = DeviceRecord {
            device_id: request.device_id,
            user_id: request.user_id,
            credential_hash: request.credential_hash,
        };
        devices.insert(device.device_id, device.clone());
        Ok(device)
    }
    async fn owner_of(&self, id: DeviceId) -> Result<Option<UserId>, BackendError> {
        self.check()?;
        Ok(self.devices.lock().unwrap().get(&id).map(|d| d.user_id))
    }
}

#[async_trait]
impl UserRepository for Store {
    async fn create_user(&self, name: &str) -> Result<UserSummary, BackendError> {
        self.check()?;
        let user = UserSummary {
            id: UserId(Uuid::new_v4()),
            name: name.into(),
        };
        self.users.lock().unwrap().insert(user.id, user.clone());
        Ok(user)
    }
    async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> {
        self.check()?;
        Ok(self.users.lock().unwrap().values().cloned().collect())
    }
    async fn find_user(&self, id: UserId) -> Result<Option<UserSummary>, BackendError> {
        self.check()?;
        Ok(self.users.lock().unwrap().get(&id).cloned())
    }
}

#[async_trait]
impl EventRepository for Store {
    async fn insert_batch(
        &self,
        _user: UserId,
        batch: &[Event],
    ) -> Result<Vec<EventId>, BackendError> {
        self.check()?;
        let mut events = self.events.lock().unwrap();
        for event in batch {
            if let Some(existing) = events.get(&(event.device_id, event.id))
                && (existing.kind != event.kind || existing.occurred_at != event.occurred_at)
            {
                return Err(BackendError::Conflict("Event conflicts".into()));
            }
        }
        self.writes.fetch_add(1, Ordering::SeqCst);
        for event in batch {
            events.insert((event.device_id, event.id), event.clone());
        }
        Ok(batch.iter().map(|e| e.id).collect())
    }
}

#[async_trait]
impl UserActionRepository for Store {
    async fn user_actions(
        &self,
        _id: UserId,
        _range: TimeRange,
    ) -> Result<Vec<UserAction>, BackendError> {
        Ok(vec![])
    }
    async fn preceding_actions(
        &self,
        _id: UserId,
        _before: OffsetDateTime,
    ) -> Result<Vec<UserAction>, BackendError> {
        Ok(vec![])
    }
}
