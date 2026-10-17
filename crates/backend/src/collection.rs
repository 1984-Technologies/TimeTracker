use std::fmt;
use std::sync::Arc;

use timetracker_model::{DeviceId, Event, EventId};

use crate::{
    BackendError,
    repositories::{DeviceRepository, EventRepository},
};

pub struct CollectionReceipt {
    pub accepted: Vec<EventId>,
}

#[derive(Debug)]
pub enum CollectionError {
    UnknownDevice(DeviceId),
    DeviceMismatch {
        batch_device_id: DeviceId,
        event_device_id: DeviceId,
    },
}

impl fmt::Display for CollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDevice(id) => write!(f, "Unknown device_id: {}", id.0),
            Self::DeviceMismatch {
                batch_device_id,
                event_device_id,
            } => write!(
                f,
                "Event device_id ({}) does not match batch device_id ({})",
                event_device_id.0, batch_device_id.0
            ),
        }
    }
}

impl std::error::Error for CollectionError {}

pub struct MetricsCollectionService {
    pub(crate) devices: Arc<dyn DeviceRepository>,
    pub(crate) events: Arc<dyn EventRepository>,
}

impl MetricsCollectionService {
    pub fn new(devices: Arc<dyn DeviceRepository>, events: Arc<dyn EventRepository>) -> Self {
        Self { devices, events }
    }

    pub async fn collect(
        &self,
        device_id: DeviceId,
        events: &[Event],
    ) -> Result<CollectionReceipt, BackendError> {
        for event in events {
            if event.device_id != device_id {
                return Err(Box::new(CollectionError::DeviceMismatch {
                    batch_device_id: device_id,
                    event_device_id: event.device_id,
                }));
            }
        }

        let user_id =
            self.devices.owner_of(device_id).await?.ok_or_else(|| {
                Box::new(CollectionError::UnknownDevice(device_id)) as BackendError
            })?;

        if events.is_empty() {
            return Ok(CollectionReceipt {
                accepted: Vec::new(),
            });
        }

        let accepted = self.events.insert_batch(user_id, events).await?;

        Ok(CollectionReceipt { accepted })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use time::OffsetDateTime;
    use timetracker_model::{EventKind, UserId};
    use uuid::Uuid;

    struct FakeDeviceRepo {
        owners: HashMap<DeviceId, UserId>,
    }

    #[async_trait]
    impl DeviceRepository for FakeDeviceRepo {
        async fn owner_of(&self, device_id: DeviceId) -> Result<Option<UserId>, BackendError> {
            Ok(self.owners.get(&device_id).copied())
        }
    }

    #[derive(Default)]
    struct FakeEventRepo {
        saved_events: Mutex<Vec<(UserId, Event)>>,
    }

    #[async_trait]
    impl EventRepository for FakeEventRepo {
        async fn insert_batch(
            &self,
            user_id: UserId,
            events: &[Event],
        ) -> Result<Vec<EventId>, BackendError> {
            let mut guard = self.saved_events.lock().unwrap();
            for event in events {
                if !guard.iter().any(|(_, existing)| existing.id == event.id) {
                    guard.push((user_id, event.clone()));
                }
            }
            Ok(events.iter().map(|e| e.id).collect())
        }
    }

    fn make_event(device_id: DeviceId) -> Event {
        Event {
            id: EventId(Uuid::new_v4()),
            device_id,
            occurred_at: OffsetDateTime::now_utc(),
            kind: EventKind::CollectorStarted,
        }
    }

    #[tokio::test]
    async fn collect_accepts_valid_batch_and_includes_already_saved_ids() {
        let device_id = DeviceId(Uuid::new_v4());
        let user_id = UserId(Uuid::new_v4());

        let mut owners = HashMap::new();
        owners.insert(device_id, user_id);

        let event_repo = Arc::new(FakeEventRepo::default());
        let service = MetricsCollectionService::new(
            Arc::new(FakeDeviceRepo { owners }),
            Arc::clone(&event_repo) as Arc<dyn EventRepository>,
        );

        let e1 = make_event(device_id);
        let e2 = make_event(device_id);

        let receipt1 = service.collect(device_id, &[e1.clone()]).await.unwrap();
        assert_eq!(receipt1.accepted, vec![e1.id]);

        let receipt2 = service
            .collect(device_id, &[e1.clone(), e2.clone()])
            .await
            .unwrap();
        assert_eq!(receipt2.accepted, vec![e1.id, e2.id]);

        assert_eq!(event_repo.saved_events.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn collect_rejects_unknown_device_without_saving() {
        let unknown_device = DeviceId(Uuid::new_v4());
        let event_repo = Arc::new(FakeEventRepo::default());

        let service = MetricsCollectionService::new(
            Arc::new(FakeDeviceRepo {
                owners: HashMap::new(),
            }),
            Arc::clone(&event_repo) as Arc<dyn EventRepository>,
        );

        let event = make_event(unknown_device);
        let result = service.collect(unknown_device, &[event]).await;

        assert!(result.is_err());
        assert!(event_repo.saved_events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn collect_rejects_entire_batch_if_one_event_has_mismatched_device_id() {
        let valid_device = DeviceId(Uuid::new_v4());
        let foreign_device = DeviceId(Uuid::new_v4());
        let user_id = UserId(Uuid::new_v4());

        let mut owners = HashMap::new();
        owners.insert(valid_device, user_id);

        let event_repo = Arc::new(FakeEventRepo::default());
        let service = MetricsCollectionService::new(
            Arc::new(FakeDeviceRepo { owners }),
            Arc::clone(&event_repo) as Arc<dyn EventRepository>,
        );

        let good_event = make_event(valid_device);
        let bad_event = make_event(foreign_device);

        let result = service
            .collect(valid_device, &[good_event, bad_event])
            .await;

        assert!(result.is_err());
        assert!(event_repo.saved_events.lock().unwrap().is_empty());
    }
}
