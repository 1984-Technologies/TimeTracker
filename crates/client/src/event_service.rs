use std::{sync::Arc, time::Duration};

use time::OffsetDateTime;
use timetracker_model::{DeviceId, Event, EventId, EventKind};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::{
    ClientError, adapters::AdapterOutput, queue::EventQueue, registration::ClientAuthService,
    upload::GrpcUploadClient,
};

pub struct ClientEventService {
    pub(crate) device_id: DeviceId,
    pub(crate) queue: Arc<EventQueue>,
    pub(crate) upload_client: GrpcUploadClient,
    pub(crate) auth: Option<Arc<ClientAuthService>>,
}

impl ClientEventService {
    // max number of events processed in 1 iteration of the upload loop
    const LIMIT: usize = 100;
    const RETRY_DELAY: Duration = Duration::from_secs(1);

    pub fn new(
        device_id: DeviceId,
        queue: Arc<EventQueue>,
        upload_client: GrpcUploadClient,
    ) -> Self {
        Self {
            device_id,
            queue,
            upload_client,
            auth: None,
        }
    }

    pub fn with_auth(
        device_id: DeviceId,
        queue: Arc<EventQueue>,
        upload_client: GrpcUploadClient,
        auth: Arc<ClientAuthService>,
    ) -> Self {
        Self {
            device_id,
            queue,
            upload_client,
            auth: Some(auth),
        }
    }

    pub async fn record_outputs(
        &self,
        mut outputs: mpsc::Receiver<AdapterOutput>,
    ) -> Result<(), ClientError> {
        while let Some(output) = outputs.recv().await {
            match output {
                AdapterOutput::Observation(observation) => {
                    let event_id = EventId(Uuid::new_v4());
                    self.queue
                        .push(Event {
                            id: event_id,
                            device_id: self.device_id,
                            occurred_at: observation.occurred_at,
                            kind: EventKind::Observation(observation.kind),
                        })
                        .await?;
                }
                AdapterOutput::Status {
                    adapter_id,
                    observation_type,
                    status,
                } => {
                    let event_id = EventId(Uuid::new_v4());
                    let timestamp = OffsetDateTime::now_utc();
                    self.queue
                        .push(Event {
                            id: event_id,
                            device_id: self.device_id,
                            occurred_at: timestamp,
                            kind: EventKind::AdapterStatus {
                                adapter_id: adapter_id.to_string(),
                                observation_type,
                                status,
                            },
                        })
                        .await?;
                }
            };
        }
        Ok(())
    }

    pub async fn enqueue_kind(&self, kind: EventKind) -> Result<(), ClientError> {
        self.queue
            .push(Event {
                id: EventId(Uuid::new_v4()),
                device_id: self.device_id,
                occurred_at: OffsetDateTime::now_utc(),
                kind,
            })
            .await
    }

    pub async fn upload_loop(
        &self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let mut shutting_down = false;

        loop {
            if !shutting_down {
                if *shutdown.borrow() {
                    shutting_down = true;
                } else {
                    tokio::select! {
                        biased;
                        changed = shutdown.changed() => {
                            shutting_down = match changed {
                                Ok(()) => *shutdown.borrow(),
                                Err(_) => true,
                            };
                        }
                        result = self.queue.wait_for_events() => result?,
                    }
                }
            }

            let events = self.queue.pending(Self::LIMIT).await?;
            if events.is_empty() {
                if shutting_down {
                    break;
                }
                continue;
            }

            // Once a request starts, let its timeout determine when it ends. Dropping the
            // request on shutdown can lose this in-memory batch before the server accepts it.
            let send_result = match &self.auth {
                Some(auth) => self.upload_client.send_authenticated(auth, &events).await,
                None => self.upload_client.send(self.device_id, &events).await,
            };

            match send_result {
                Ok(event_ids) => self.queue.acknowledge(&event_ids).await?,
                Err(error) => {
                    eprintln!("Failed to upload events: {error}");
                    // Keep unacknowledged events for a later attempt, but do not retry
                    // forever when the client is already stopping.
                    if shutting_down || *shutdown.borrow() || shutdown.has_changed().is_err() {
                        return Err(error);
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Self::RETRY_DELAY) => {}
                        _ = shutdown.wait_for(|stopping| *stopping) => return Err(error),
                    }
                }
            }
        }
        Ok(())
    }
}
