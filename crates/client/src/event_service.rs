use std::sync::Arc;

use time::OffsetDateTime;
use timetracker_model::{DeviceId, Event, EventId, EventKind};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::{ClientError, adapters::AdapterOutput, queue::EventQueue, upload::GrpcUploadClient};

pub struct ClientEventService {
    pub(crate) device_id: DeviceId,
    pub(crate) queue: Arc<EventQueue>,
    pub(crate) upload_client: GrpcUploadClient,
}

impl ClientEventService {
    // max number of events processed in 1 iteration of the upload loop
    const LIMIT: usize = 100;

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
        mut outputs: mpsc::Receiver<AdapterOutput>,
    ) -> Result<(), ClientError> {
        while let Some(adapter_output) = outputs.recv().await {
            let (occurred_at, kind) = match adapter_output {
                AdapterOutput::Observation(observation) => (
                    observation.occurred_at,
                    EventKind::Observation(observation.kind),
                ),
                AdapterOutput::Status {
                    adapter_id,
                    observation_type,
                    status,
                } => (
                    OffsetDateTime::now_utc(),
                    EventKind::AdapterStatus {
                        adapter_id: adapter_id.to_owned(),
                        observation_type,
                        status,
                    },
                ),
            };
            self.enqueue(occurred_at, kind).await?;
        }
        Ok(())
    }

    pub async fn enqueue_kind(&self, kind: EventKind) -> Result<(), ClientError> {
        self.enqueue(OffsetDateTime::now_utc(), kind).await
    }

    async fn enqueue(
        &self,
        occurred_at: OffsetDateTime,
        kind: EventKind,
    ) -> Result<(), ClientError> {
        self.queue
            .push(Event {
                id: EventId(Uuid::new_v4()),
                device_id: self.device_id,
                occurred_at,
                kind,
            })
            .await
    }

    pub async fn upload_loop(
        &self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        loop {
            if *shutdown.borrow() {
                return Ok(());
            }
            tokio::select! {
                result = self.queue.wait_for_events() => result?,
                result = shutdown.changed() => {
                    if result.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                    continue;
                }
            }
            let events = self.queue.pending(Self::LIMIT).await?;
            match self.upload_client.send(self.device_id, &events).await {
                Ok(event_ids) => {
                    self.queue.acknowledge(&event_ids).await?;
                    if event_ids.is_empty() {
                        wait_before_retry(&mut shutdown).await;
                    }
                }
                Err(error) => {
                    eprintln!("event upload failed: {error}; retrying in 3 seconds");
                    wait_before_retry(&mut shutdown).await;
                }
            }
        }
    }
}

async fn wait_before_retry(shutdown: &mut watch::Receiver<bool>) {
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {}
        _ = shutdown.changed() => {}
    }
}
