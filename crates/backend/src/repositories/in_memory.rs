use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use time::OffsetDateTime;
use timetracker_model::{DeviceId, Event, EventId, EventKind, ObservationKind, UserId};

use crate::{
    BackendError,
    dashboard::{PRECEDING_ACTIONS_LIMIT, TimeRange, UserAction, UserActionKind, UserSummary},
    repositories::{DeviceRepository, EventRepository, UserActionRepository, UserRepository},
};

#[derive(Default)]
struct State {
    devices: HashMap<DeviceId, UserId>,
    users: HashMap<UserId, UserSummary>,
    events: Vec<(UserId, Event)>,
}

#[derive(Default)]
pub struct InMemoryRepository {
    state: Mutex<State>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, BackendError> {
        self.state.lock().map_err(|error| {
            BackendError::Internal(format!("in-memory store lock poisoned: {error}"))
        })
    }
}

#[async_trait]
impl DeviceRepository for InMemoryRepository {
    async fn owner_of(&self, device_id: DeviceId) -> Result<Option<UserId>, BackendError> {
        let mut state = self.lock()?;
        let user_id = *state
            .devices
            .entry(device_id)
            .or_insert(UserId(device_id.0));
        state.users.entry(user_id).or_insert_with(|| UserSummary {
            id: user_id,
            name: format!("Device {}", &device_id.0.to_string()[..8]),
        });
        Ok(Some(user_id))
    }
}

#[async_trait]
impl EventRepository for InMemoryRepository {
    async fn insert_batch(
        &self,
        user_id: UserId,
        events: &[Event],
    ) -> Result<Vec<EventId>, BackendError> {
        let mut state = self.lock()?;
        for event in events {
            if !state.events.iter().any(|(_, saved)| saved.id == event.id) {
                state.events.push((user_id, event.clone()));
            }
        }
        Ok(events.iter().map(|event| event.id).collect())
    }
}

#[async_trait]
impl UserRepository for InMemoryRepository {
    async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> {
        let mut users: Vec<_> = self.lock()?.users.values().cloned().collect();
        users.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.0.cmp(&right.id.0)));
        Ok(users)
    }

    async fn find_user(&self, user_id: UserId) -> Result<Option<UserSummary>, BackendError> {
        Ok(self.lock()?.users.get(&user_id).cloned())
    }
}

#[async_trait]
impl UserActionRepository for InMemoryRepository {
    async fn user_actions(
        &self,
        user_id: UserId,
        range: TimeRange,
    ) -> Result<Vec<UserAction>, BackendError> {
        Ok(self
            .lock()?
            .events
            .iter()
            .filter(|(owner, event)| {
                *owner == user_id
                    && event.occurred_at >= range.start
                    && event.occurred_at <= range.end
            })
            .map(|(_, event)| to_user_action(event))
            .collect())
    }

    async fn preceding_actions(
        &self,
        user_id: UserId,
        before: OffsetDateTime,
    ) -> Result<Vec<UserAction>, BackendError> {
        let mut events: Vec<_> = self
            .lock()?
            .events
            .iter()
            .filter(|(owner, event)| *owner == user_id && event.occurred_at < before)
            .map(|(_, event)| to_user_action(event))
            .collect();
        events.sort_by(|left, right| right.occurred_at.cmp(&left.occurred_at));
        events.truncate(PRECEDING_ACTIONS_LIMIT as usize);
        Ok(events)
    }
}

fn to_user_action(event: &Event) -> UserAction {
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
        EventKind::Observation(ObservationKind::KeyboardButtonPressed { key }) => {
            UserActionKind::KeyboardButtonPressed { key: key.clone() }
        }
        EventKind::Observation(ObservationKind::MouseClicked { button, x, y }) => {
            UserActionKind::MouseClicked {
                button: button.clone(),
                x: *x,
                y: *y,
            }
        }
        EventKind::Observation(ObservationKind::MouseMoved { x, y }) => {
            UserActionKind::MouseMoved { x: *x, y: *y }
        }
        EventKind::Observation(ObservationKind::SystemSleepChanged { sleeping }) => {
            UserActionKind::SystemSleepChanged {
                sleeping: *sleeping,
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
    };

    UserAction {
        id: event.id,
        device_id: event.device_id,
        occurred_at: event.occurred_at,
        kind,
    }
}
