use std::sync::Arc;

use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, DeviceId, EventId, ObservationType, UserId};

use crate::{
    BackendError,
    repositories::{UserActionRepository, UserRepository},
};

pub struct TimeRange {
    pub start: OffsetDateTime,
    pub end: OffsetDateTime,
}

pub struct UserSummary {
    pub id: UserId,
    pub name: String,
}

pub enum UserActionKind {
    CollectorStarted,
    CollectorStopped,
    Heartbeat,
    IdleChanged {
        idle: bool,
    },
    FocusedApplicationChanged {
        app_id: String,
        title: Option<String>,
    },
    AdapterStatus {
        adapter_id: String,
        observation_type: ObservationType,
        status: AdapterStatus,
    },
}

pub struct UserAction {
    pub id: EventId,
    pub device_id: DeviceId,
    pub occurred_at: OffsetDateTime,
    pub kind: UserActionKind,
}

pub struct DashboardService {
    pub(crate) users: Arc<dyn UserRepository>,
    pub(crate) actions: Arc<dyn UserActionRepository>,
}

impl DashboardService {
    pub fn new(users: Arc<dyn UserRepository>, actions: Arc<dyn UserActionRepository>) -> Self {
        Self { users, actions }
    }

    pub async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> {
        todo!()
    }

    pub async fn user_actions(
        &self,
        _user_id: UserId,
        _range: TimeRange,
    ) -> Result<Vec<UserAction>, BackendError> {
        todo!()
    }
}
