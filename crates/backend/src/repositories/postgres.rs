use std::io;

use async_trait::async_trait;
use sqlx::PgPool;
use timetracker_model::{DeviceId, Event, EventId, UserId};

use crate::{
    BackendError,
    dashboard::{TimeRange, UserAction, UserSummary},
    repositories::{DeviceRepository, EventRepository, UserActionRepository, UserRepository},
};

fn not_implemented(operation: &str) -> BackendError {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("PostgreSQL repository operation '{operation}' is not implemented"),
    )
    .into()
}

pub struct PostgresDeviceRepository {
    _pool: PgPool,
}

impl PostgresDeviceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { _pool: pool }
    }
}

#[async_trait]
impl DeviceRepository for PostgresDeviceRepository {
    async fn owner_of(&self, _device_id: DeviceId) -> Result<Option<UserId>, BackendError> {
        Err(not_implemented("device owner lookup"))
    }
}

pub struct PostgresEventRepository {
    _pool: PgPool,
}

impl PostgresEventRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { _pool: pool }
    }
}

#[async_trait]
impl EventRepository for PostgresEventRepository {
    async fn insert_batch(
        &self,
        _user_id: UserId,
        _events: &[Event],
    ) -> Result<Vec<EventId>, BackendError> {
        Err(not_implemented("event batch insertion"))
    }
}

pub struct PostgresUserRepository {
    _pool: PgPool,
}

impl PostgresUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { _pool: pool }
    }
}

#[async_trait]
impl UserRepository for PostgresUserRepository {
    async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> {
        Err(not_implemented("user listing"))
    }

    async fn find_user(&self, _user_id: UserId) -> Result<Option<UserSummary>, BackendError> {
        Err(not_implemented("user lookup"))
    }
}

pub struct PostgresUserActionRepository {
    _pool: PgPool,
}

impl PostgresUserActionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { _pool: pool }
    }
}

#[async_trait]
impl UserActionRepository for PostgresUserActionRepository {
    async fn user_actions(
        &self,
        _user_id: UserId,
        _range: TimeRange,
    ) -> Result<Vec<UserAction>, BackendError> {
        Err(not_implemented("user action listing"))
    }
}
