use std::sync::Arc;

use serde::{Deserialize, Serialize};
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserSummary {
    pub id: UserId,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserAction {
    pub id: EventId,
    pub device_id: DeviceId,
    #[serde(with = "time::serde::rfc3339")]
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
        self.users.list_users().await
    }

    pub async fn user_actions(
        &self,
        user_id: UserId,
        range: TimeRange,
    ) -> Result<Vec<UserAction>, BackendError> {
        self.actions.user_actions(user_id, range).await
    }
}
