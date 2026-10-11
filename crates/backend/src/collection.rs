use std::sync::Arc;

use timetracker_model::{DeviceId, Event, EventId};

use crate::{
    BackendError,
    repositories::{DeviceRepository, EventRepository},
};

pub struct CollectionReceipt {
    pub accepted: Vec<EventId>,
}

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
        _device_id: DeviceId,
        _events: &[Event],
    ) -> Result<CollectionReceipt, BackendError> {
        todo!()
    }
}
