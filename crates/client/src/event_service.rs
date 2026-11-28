use std::{sync::Arc, time::Duration};

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

    pub async fn upload_loop(&self, shutdown: watch::Receiver<bool>) -> Result<(), ClientError> {
        self.upload_loop_with_auth(shutdown, None).await
    }

    pub async fn upload_loop_authenticated(
        &self,
        shutdown: watch::Receiver<bool>,
        auth: &crate::registration::ClientAuthService,
    ) -> Result<(), ClientError> {
        self.upload_loop_with_auth(shutdown, Some(auth)).await
    }

    async fn upload_loop_with_auth(
        &self,
        mut shutdown: watch::Receiver<bool>,
        auth: Option<&crate::registration::ClientAuthService>,
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
            let result = match auth {
                Some(auth) => self.upload_client.send_authenticated(auth, &events).await,
                None => self.upload_client.send(self.device_id, &events).await,
            };
            match result {
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
