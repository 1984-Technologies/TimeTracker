use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, DeviceId, EventId, ObservationType, UserId};

use crate::{
    BackendError,
    repositories::{UserActionRepository, UserRepository},
};

pub const PRECEDING_ACTIONS_LIMIT: u32 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: OffsetDateTime,
    pub end: OffsetDateTime,
}

impl TimeRange {
    pub fn validate(&self) -> Result<(), BackendError> {
        if self.start > self.end {
            return Err(BackendError::Validation("start must be <= end".to_string()));
        }
        Ok(())
    }
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
        range.validate()?;

        let mut actions = self.actions.user_actions(user_id, range).await?;

        let preceding = self.actions.preceding_actions(user_id, range.start).await?;

        let mut existing_ids = HashSet::new();
        actions.retain(|action| existing_ids.insert(action.id));
        for action in preceding {
            if existing_ids.insert(action.id) {
                actions.push(action);
            }
        }

        actions.sort_by(|a, b| a.occurred_at.cmp(&b.occurred_at).then(a.id.0.cmp(&b.id.0)));

        Ok(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use timetracker_model::{DeviceId, EventId};
    use uuid::Uuid;

    fn create_test_action(id: Uuid, occurred_at: OffsetDateTime) -> UserAction {
        UserAction {
            id: EventId(id),
            device_id: DeviceId(Uuid::new_v4()),
            occurred_at,
            kind: UserActionKind::Heartbeat,
        }
    }

    #[test]
    fn time_range_validation_rejects_invalid_range() {
        let range = TimeRange {
            start: OffsetDateTime::from_unix_timestamp(100).unwrap(),
            end: OffsetDateTime::from_unix_timestamp(50).unwrap(),
        };
        assert!(range.validate().is_err());
    }

    #[test]
    fn time_range_validation_accepts_valid_range() {
        let range = TimeRange {
            start: OffsetDateTime::from_unix_timestamp(50).unwrap(),
            end: OffsetDateTime::from_unix_timestamp(100).unwrap(),
        };
        assert!(range.validate().is_ok());
    }

    #[test]
    fn time_range_validation_accepts_equal_range() {
        let ts = OffsetDateTime::from_unix_timestamp(50).unwrap();
        let range = TimeRange { start: ts, end: ts };
        assert!(range.validate().is_ok());
    }

    struct MockUserRepository;

    #[async_trait::async_trait]
    impl UserRepository for MockUserRepository {
        async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> {
            Ok(vec![])
        }

        async fn find_user(&self, _user_id: UserId) -> Result<Option<UserSummary>, BackendError> {
            Ok(None)
        }
    }

    struct MockUserActionRepository {
        actions: Vec<UserAction>,
        preceding: Vec<UserAction>,
    }

    #[async_trait::async_trait]
    impl UserActionRepository for MockUserActionRepository {
        async fn user_actions(
            &self,
            _user_id: UserId,
            _range: TimeRange,
        ) -> Result<Vec<UserAction>, BackendError> {
            Ok(self.actions.clone())
        }

        async fn preceding_actions(
            &self,
            _user_id: UserId,
            _before: OffsetDateTime,
        ) -> Result<Vec<UserAction>, BackendError> {
            Ok(self.preceding.clone())
        }
    }

    #[tokio::test]
    async fn user_actions_deduplicates_by_event_id() {
        let ts1 = OffsetDateTime::from_unix_timestamp(10).unwrap();
        let ts2 = OffsetDateTime::from_unix_timestamp(20).unwrap();
        let ts3 = OffsetDateTime::from_unix_timestamp(30).unwrap();

        let action1 = create_test_action(Uuid::new_v4(), ts1);
        let action2 = create_test_action(Uuid::new_v4(), ts2);
        let action3 = create_test_action(Uuid::new_v4(), ts3);

        let mut preceding = action2.clone();
        preceding.kind = UserActionKind::CollectorStarted;

        let repo = MockUserActionRepository {
            actions: vec![action1.clone(), action2.clone(), action2.clone()],
            preceding: vec![preceding, action3.clone(), action3.clone()],
        };

        let service = DashboardService::new(Arc::new(MockUserRepository), Arc::new(repo));

        let range = TimeRange {
            start: OffsetDateTime::from_unix_timestamp(15).unwrap(),
            end: OffsetDateTime::from_unix_timestamp(40).unwrap(),
        };

        let result = service
            .user_actions(UserId(Uuid::new_v4()), range)
            .await
            .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].id, action1.id);
        assert_eq!(result[1].id, action2.id);
        assert_eq!(result[2].id, action3.id);
    }

    #[tokio::test]
    async fn user_actions_orders_equal_timestamps_by_event_id() {
        let timestamp = OffsetDateTime::from_unix_timestamp(20).unwrap();
        let first = create_test_action(Uuid::from_u128(1), timestamp);
        let second = create_test_action(Uuid::from_u128(2), timestamp);
        let service = DashboardService::new(
            Arc::new(MockUserRepository),
            Arc::new(MockUserActionRepository {
                actions: vec![second.clone(), first.clone()],
                preceding: vec![],
            }),
        );
        let result = service
            .user_actions(
                UserId(Uuid::new_v4()),
                TimeRange {
                    start: timestamp,
                    end: timestamp,
                },
            )
            .await
            .unwrap();
        assert_eq!(result, vec![first, second]);
    }

    #[tokio::test]
    async fn user_actions_sorts_by_occurred_at() {
        let ts1 = OffsetDateTime::from_unix_timestamp(30).unwrap();
        let ts2 = OffsetDateTime::from_unix_timestamp(10).unwrap();
        let ts3 = OffsetDateTime::from_unix_timestamp(20).unwrap();

        let action1 = create_test_action(Uuid::new_v4(), ts1);
        let action2 = create_test_action(Uuid::new_v4(), ts2);
        let action3 = create_test_action(Uuid::new_v4(), ts3);

        let repo = MockUserActionRepository {
            actions: vec![action1.clone(), action3.clone()],
            preceding: vec![action2.clone()],
        };

        let service = DashboardService::new(Arc::new(MockUserRepository), Arc::new(repo));

        let range = TimeRange {
            start: OffsetDateTime::from_unix_timestamp(5).unwrap(),
            end: OffsetDateTime::from_unix_timestamp(40).unwrap(),
        };

        let result = service
            .user_actions(UserId(Uuid::new_v4()), range)
            .await
            .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].occurred_at, ts2);
        assert_eq!(result[1].occurred_at, ts3);
        assert_eq!(result[2].occurred_at, ts1);
    }

    #[tokio::test]
    async fn user_actions_rejects_invalid_range() {
        let service = DashboardService::new(
            Arc::new(MockUserRepository),
            Arc::new(MockUserActionRepository {
                actions: vec![],
                preceding: vec![],
            }),
        );

        let range = TimeRange {
            start: OffsetDateTime::from_unix_timestamp(100).unwrap(),
            end: OffsetDateTime::from_unix_timestamp(50).unwrap(),
        };

        let result = service.user_actions(UserId(Uuid::new_v4()), range).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn user_actions_with_preceding_actions_includes_them() {
        let ts1 = OffsetDateTime::from_unix_timestamp(10).unwrap();
        let ts2 = OffsetDateTime::from_unix_timestamp(20).unwrap();
        let ts3 = OffsetDateTime::from_unix_timestamp(30).unwrap();

        let action1 = create_test_action(Uuid::new_v4(), ts1);
        let action2 = create_test_action(Uuid::new_v4(), ts2);
        let action3 = create_test_action(Uuid::new_v4(), ts3);

        let repo = MockUserActionRepository {
            actions: vec![action2.clone(), action3.clone()],
            preceding: vec![action1.clone()],
        };

        let service = DashboardService::new(Arc::new(MockUserRepository), Arc::new(repo));

        let range = TimeRange {
            start: OffsetDateTime::from_unix_timestamp(15).unwrap(),
            end: OffsetDateTime::from_unix_timestamp(40).unwrap(),
        };

        let result = service
            .user_actions(UserId(Uuid::new_v4()), range)
            .await
            .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].id, action1.id);
        assert_eq!(result[1].id, action2.id);
        assert_eq!(result[2].id, action3.id);
    }

    #[test]
    fn test_preceding_actions_limit_constant() {
        assert_eq!(PRECEDING_ACTIONS_LIMIT, 10);
    }
}
