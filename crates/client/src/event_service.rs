use std::sync::Arc;

use timetracker_model::{DeviceId, EventKind};
use tokio::sync::{mpsc, watch};

use crate::{ClientError, adapters::AdapterOutput, queue::EventQueue, upload::GrpcUploadClient};

pub struct ClientEventService {
    pub(crate) device_id: DeviceId,
    pub(crate) queue: Arc<EventQueue>,
    pub(crate) upload_client: GrpcUploadClient,
}

impl ClientEventService {
    pub fn new(
        device_id: DeviceId,
        queue: Arc<EventQueue>,
        upload_client: GrpcUploadClient,
    ) -> Self {
        Self {
            device_id,
            queue,
            upload_client,
        }
    }

    pub async fn record_outputs(
        &self,
        _outputs: mpsc::Receiver<AdapterOutput>,
    ) -> Result<(), ClientError> {
        todo!()
    }

    pub async fn enqueue_kind(&self, _kind: EventKind) -> Result<(), ClientError> {
        todo!()
    }

    pub async fn upload_loop(&self, _shutdown: watch::Receiver<bool>) -> Result<(), ClientError> {
        todo!()
    }
}
