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
                BackendError::Internal(
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
        _user_id: UserId,
        _before: OffsetDateTime,
    ) -> Result<Vec<UserAction>, BackendError> {
        todo!()
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
