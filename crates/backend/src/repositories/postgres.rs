use async_trait::async_trait;
use sqlx::{PgPool, types::Json};
use time::OffsetDateTime;
use timetracker_model::{DeviceId, Event, EventId, EventKind, ObservationKind, UserId};

use crate::{
    BackendError,
    dashboard::{TimeRange, UserAction, UserActionKind, UserSummary},
    repositories::{DeviceRepository, EventRepository, UserActionRepository, UserRepository},
};

pub struct PostgresDeviceRepository(pub PgPool);
pub struct PostgresEventRepository(pub PgPool);
pub struct PostgresUserRepository(pub PgPool);
pub struct PostgresUserActionRepository(pub PgPool);

impl PostgresDeviceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self(pool)
    }
}

impl PostgresEventRepository {
    pub fn new(pool: PgPool) -> Self {
        Self(pool)
    }
}

impl PostgresUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self(pool)
    }
}

impl PostgresUserActionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self(pool)
    }
}

#[async_trait]
impl DeviceRepository for PostgresDeviceRepository {
    async fn find(
        &self,
        device_id: timetracker_model::DeviceId,
    ) -> Result<Option<crate::auth::DeviceRecord>, BackendError> {
        Ok(sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid, String)>(
            "SELECT id, user_id, credential_hash FROM devices WHERE id = $1 AND credential_hash IS NOT NULL")
            .bind(device_id.0).fetch_optional(&self.0).await?.map(device_record))
    }
    async fn list(&self) -> Result<Vec<crate::auth::DeviceRecord>, BackendError> {
        Ok(sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid, String)>(
            "SELECT id, user_id, credential_hash FROM devices WHERE credential_hash IS NOT NULL ORDER BY id")
            .fetch_all(&self.0).await?.into_iter().map(device_record).collect())
    }
    async fn create_approved(
        &self,
        request: timetracker_model::auth::DeviceApprovalRequest,
    ) -> Result<crate::auth::DeviceRecord, BackendError> {
        crate::auth::decode_credential_hash(&request.credential_hash)?;
        let record = sqlx::query_as::<_, (uuid::Uuid, uuid::Uuid, String)>(
            "INSERT INTO devices (id, user_id, credential_hash) VALUES ($1, $2, $3)
             ON CONFLICT (id) DO UPDATE SET credential_hash = EXCLUDED.credential_hash
             WHERE devices.user_id = EXCLUDED.user_id
               AND (devices.credential_hash IS NULL OR devices.credential_hash = EXCLUDED.credential_hash)
             RETURNING id, user_id, credential_hash")
            .bind(request.device_id.0).bind(request.user_id.0).bind(request.credential_hash)
            .fetch_optional(&self.0).await.map_err(|error| {
                if error.as_database_error().is_some_and(|e| e.is_foreign_key_violation()) {
                    BackendError::NotFound("User not found".into())
                } else { error.into() }
            })?;
        record.map(device_record).ok_or_else(|| {
            BackendError::Conflict("Device already has different ownership or credentials".into())
        })
    }

    async fn owner_of(&self, device_id: DeviceId) -> Result<Option<UserId>, BackendError> {
        Ok(
            sqlx::query_scalar::<_, uuid::Uuid>("SELECT user_id FROM devices WHERE id = $1")
                .bind(device_id.0)
                .fetch_optional(&self.0)
                .await?
                .map(UserId),
        )
    }
}

#[async_trait]
impl EventRepository for PostgresEventRepository {
    async fn insert_batch(
        &self,
        user_id: UserId,
        events: &[Event],
    ) -> Result<Vec<EventId>, BackendError> {
        let mut transaction = self.0.begin().await?;
        let mut accepted = Vec::with_capacity(events.len());
        for event in events {
            let id: Option<uuid::Uuid> = sqlx::query_scalar("INSERT INTO events (id, user_id, device_id, occurred_at, payload, action) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (device_id, id) DO UPDATE SET id = EXCLUDED.id WHERE events.user_id = EXCLUDED.user_id AND events.payload = EXCLUDED.payload RETURNING id")
                .bind(event.id.0).bind(user_id.0).bind(event.device_id.0).bind(event.occurred_at)
                .bind(Json(event)).bind(action(event).map(Json)).fetch_optional(&mut *transaction).await?;
            accepted.push(EventId(id.ok_or_else(|| {
                BackendError::Conflict(
                    "event ID is already associated with different event data".into(),
                )
            })?));
        }
        transaction.commit().await?;
        Ok(accepted)
    }
}

#[async_trait]
impl UserRepository for PostgresUserRepository {
    async fn create_user(&self, name: &str) -> Result<UserSummary, BackendError> {
        let id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, name) VALUES ($1, $2)")
            .bind(id)
            .bind(name)
            .execute(&self.0)
            .await?;
        Ok(UserSummary {
            id: UserId(id),
            name: name.into(),
        })
    }

    async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> {
        Ok(sqlx::query_as::<_, (uuid::Uuid, String)>(
            "SELECT id, name FROM users ORDER BY name, id",
        )
        .fetch_all(&self.0)
        .await?
        .into_iter()
        .map(|(id, name)| UserSummary {
            id: UserId(id),
            name,
        })
        .collect())
    }

    async fn find_user(&self, user_id: UserId) -> Result<Option<UserSummary>, BackendError> {
        Ok(
            sqlx::query_as::<_, (uuid::Uuid, String)>("SELECT id, name FROM users WHERE id = $1")
                .bind(user_id.0)
                .fetch_optional(&self.0)
                .await?
                .map(|(id, name)| UserSummary {
                    id: UserId(id),
                    name,
                }),
        )
    }
}

#[async_trait]
impl UserActionRepository for PostgresUserActionRepository {
    async fn user_actions(
        &self,
        user_id: UserId,
        range: TimeRange,
    ) -> Result<Vec<UserAction>, BackendError> {
        Ok(sqlx::query_scalar::<_, Json<UserAction>>("SELECT action FROM events WHERE user_id = $1 AND occurred_at >= $2 AND occurred_at <= $3 AND action IS NOT NULL ORDER BY occurred_at, id")
            .bind(user_id.0).bind(range.start).bind(range.end).fetch_all(&self.0).await?.into_iter().map(|Json(action)| action).collect())
    }

    async fn preceding_actions(
        &self,
        user_id: UserId,
        before: OffsetDateTime,
    ) -> Result<Vec<UserAction>, BackendError> {
        Ok(sqlx::query_scalar::<_, Json<UserAction>>(
            "SELECT action FROM events WHERE user_id = $1 AND occurred_at < $2 AND action IS NOT NULL ORDER BY occurred_at DESC, id DESC LIMIT $3")
            .bind(user_id.0).bind(before).bind(i64::from(crate::dashboard::PRECEDING_ACTIONS_LIMIT))
            .fetch_all(&self.0).await?.into_iter().map(|Json(action)| action).collect())
    }
}

fn action(event: &Event) -> Option<UserAction> {
    let kind = match &event.kind {
        EventKind::CollectorStarted => UserActionKind::CollectorStarted,
        EventKind::CollectorStopped => UserActionKind::CollectorStopped,
        EventKind::Heartbeat => UserActionKind::Heartbeat,
        EventKind::Observation(ObservationKind::IdleChanged { idle }) => {
            UserActionKind::IdleChanged { idle: *idle }
        }
        EventKind::Observation(ObservationKind::FocusedApplicationChanged { app_id, title }) => {
            UserActionKind::FocusedApplicationChanged {
                app_id: app_id.clone(),
                title: title.clone(),
            }
        }
        EventKind::AdapterStatus {
            adapter_id,
            observation_type,
            status,
        } => UserActionKind::AdapterStatus {
            adapter_id: adapter_id.clone(),
            observation_type: *observation_type,
            status: status.clone(),
        },
        // The dashboard's action model does not include individual input or sleep events.
        EventKind::Observation(_) => return None,
    };
    Some(UserAction {
        id: event.id,
        device_id: event.device_id,
        occurred_at: event.occurred_at,
        kind,
    })
}

pub struct PostgresAdminAccountRepository(pub PgPool);

impl PostgresAdminAccountRepository {
    pub fn new(pool: PgPool) -> Self {
        Self(pool)
    }
}

#[async_trait]
impl super::AdminAccountRepository for PostgresAdminAccountRepository {
    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<Option<crate::auth::AdminAccountRecord>, BackendError> {
        Ok(sqlx::query_as::<_, (uuid::Uuid, String, String)>(
            "SELECT id, username, password_hash FROM admin_accounts WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.0)
        .await?
        .map(admin_record))
    }
    async fn find_by_username(
        &self,
        username: &str,
    ) -> Result<Option<crate::auth::AdminAccountRecord>, BackendError> {
        Ok(sqlx::query_as::<_, (uuid::Uuid, String, String)>(
            "SELECT id, username, password_hash FROM admin_accounts WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.0)
        .await?
        .map(admin_record))
    }
    async fn list(&self) -> Result<Vec<crate::auth::AdminAccountRecord>, BackendError> {
        Ok(sqlx::query_as::<_, (uuid::Uuid, String, String)>(
            "SELECT id, username, password_hash FROM admin_accounts ORDER BY username, id",
        )
        .fetch_all(&self.0)
        .await?
        .into_iter()
        .map(admin_record)
        .collect())
    }
    async fn create(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<crate::auth::AdminAccountRecord, BackendError> {
        Ok(admin_record(sqlx::query_as::<_, (uuid::Uuid, String, String)>(
            "INSERT INTO admin_accounts (id, username, password_hash) VALUES ($1, $2, $3) RETURNING id, username, password_hash")
            .bind(uuid::Uuid::new_v4()).bind(username).bind(password_hash).fetch_one(&self.0).await?))
    }
    async fn change_password(
        &self,
        id: uuid::Uuid,
        password_hash: &str,
    ) -> Result<(), BackendError> {
        let result = sqlx::query("UPDATE admin_accounts SET password_hash = $1 WHERE id = $2")
            .bind(password_hash)
            .bind(id)
            .execute(&self.0)
            .await?;
        if result.rows_affected() == 0 {
            return Err(BackendError::NotFound("Admin not found".into()));
        }
        Ok(())
    }
    async fn ensure_initial_account(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<(), BackendError> {
        let mut transaction = self.0.begin().await?;
        sqlx::query("LOCK TABLE admin_accounts IN EXCLUSIVE MODE")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO admin_accounts (id, username, password_hash) SELECT $1, $2, $3 WHERE NOT EXISTS (SELECT 1 FROM admin_accounts)")
            .bind(uuid::Uuid::new_v4()).bind(username).bind(password_hash).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn device_record((id, user, hash): (uuid::Uuid, uuid::Uuid, String)) -> crate::auth::DeviceRecord {
    crate::auth::DeviceRecord {
        device_id: DeviceId(id),
        user_id: UserId(user),
        credential_hash: hash,
    }
}

fn admin_record(
    (id, username, password_hash): (uuid::Uuid, String, String),
) -> crate::auth::AdminAccountRecord {
    crate::auth::AdminAccountRecord {
        id,
        username,
        password_hash,
    }
}
