mod postgres;

pub use postgres::{
    PostgresAdminAccountRepository, PostgresDeviceRepository, PostgresEventRepository,
    PostgresUserActionRepository, PostgresUserRepository,
};

use async_trait::async_trait;
use time::OffsetDateTime;
use timetracker_model::{DeviceId, Event, EventId, UserId};

use crate::{
    BackendError,
    dashboard::{TimeRange, UserAction, UserSummary},
};

#[async_trait]
pub trait DeviceRepository: Send + Sync {
    async fn find(
        &self,
        device_id: DeviceId,
    ) -> Result<Option<crate::auth::DeviceRecord>, BackendError>;
    async fn list(&self) -> Result<Vec<crate::auth::DeviceRecord>, BackendError>;
    async fn create_approved(
        &self,
        request: timetracker_model::auth::DeviceApprovalRequest,
    ) -> Result<crate::auth::DeviceRecord, BackendError>;

    async fn owner_of(&self, device_id: DeviceId) -> Result<Option<UserId>, BackendError>;
}

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn insert_batch(
        &self,
        user_id: UserId,
        events: &[Event],
    ) -> Result<Vec<EventId>, BackendError>;
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create_user(&self, name: &str) -> Result<UserSummary, BackendError>;

    async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError>;

    async fn find_user(&self, user_id: UserId) -> Result<Option<UserSummary>, BackendError>;
}

#[async_trait]
pub trait UserActionRepository: Send + Sync {
    async fn user_actions(
        &self,
        user_id: UserId,
        range: TimeRange,
    ) -> Result<Vec<UserAction>, BackendError>;

    /// Returns actions that occurred strictly before `before`.
    /// These are needed to determine the state at the start of a time range.
    /// The contract is: return at most `PRECEDING_ACTIONS_LIMIT` most recent actions before `before`,
    /// ordered by occurred_at descending.
    async fn preceding_actions(
        &self,
        user_id: UserId,
        before: OffsetDateTime,
    ) -> Result<Vec<UserAction>, BackendError>;
}

#[async_trait]
pub trait AdminAccountRepository: Send + Sync {
    async fn find_by_id(
        &self,
        id: uuid::Uuid,
    ) -> Result<Option<crate::auth::AdminAccountRecord>, BackendError>;
    async fn find_by_username(
        &self,
        username: &str,
    ) -> Result<Option<crate::auth::AdminAccountRecord>, BackendError>;
    async fn list(&self) -> Result<Vec<crate::auth::AdminAccountRecord>, BackendError>;
    async fn create(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<crate::auth::AdminAccountRecord, BackendError>;
    async fn change_password(
        &self,
        id: uuid::Uuid,
        password_hash: &str,
    ) -> Result<(), BackendError>;
    async fn ensure_initial_account(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<(), BackendError>;
}
