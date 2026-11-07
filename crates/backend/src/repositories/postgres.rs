use async_trait::async_trait;
use time::OffsetDateTime;
use timetracker_model::{DeviceId, Event, EventId, UserId};

use crate::{
    BackendError,
    dashboard::{TimeRange, UserAction, UserSummary},
    repositories::{DeviceRepository, EventRepository, UserActionRepository, UserRepository},
};

pub struct PostgresDeviceRepository;

#[async_trait]
impl DeviceRepository for PostgresDeviceRepository {
    async fn owner_of(&self, _device_id: DeviceId) -> Result<Option<UserId>, BackendError> {
        todo!()
    }
}

pub struct PostgresEventRepository;

#[async_trait]
impl EventRepository for PostgresEventRepository {
    async fn insert_batch(
        &self,
        _user_id: UserId,
        _events: &[Event],
    ) -> Result<Vec<EventId>, BackendError> {
        todo!()
    }
}

pub struct PostgresUserRepository;

#[async_trait]
impl UserRepository for PostgresUserRepository {
    async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> {
        todo!()
    }

    async fn find_user(&self, _user_id: UserId) -> Result<Option<UserSummary>, BackendError> {
        todo!()
    }
}

pub struct PostgresUserActionRepository;

#[async_trait]
impl UserActionRepository for PostgresUserActionRepository {
    async fn user_actions(
        &self,
        _user_id: UserId,
        _range: TimeRange,
    ) -> Result<Vec<UserAction>, BackendError> {
        todo!()
    }

    async fn preceding_actions(
        &self,
        _user_id: UserId,
        _before: OffsetDateTime,
    ) -> Result<Vec<UserAction>, BackendError> {
        todo!()
    }
}
