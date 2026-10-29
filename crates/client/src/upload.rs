use timetracker_model::{DeviceId, Event, EventId};

use crate::ClientError;

pub struct GrpcUploadClient;

impl GrpcUploadClient {
    pub fn new() -> GrpcUploadClient {
        todo!()
    }

    pub async fn send(
        &self,
        _device_id: DeviceId,
        _events: &[Event],
    ) -> Result<Vec<EventId>, ClientError> {
        todo!()
    }
}
