use std::sync::Arc;
use std::time::Duration;
use timetracker_model::{
    AdapterStatus, DeviceId, Event, EventId, EventKind, ObservationKind, ObservationType,
};
use tokio::sync::Mutex;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use crate::ClientError;

pub use timetracker_proto::metrics as proto;

use proto::client_metrics_uploader_client::ClientMetricsUploaderClient;
use proto::*;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct GrpcUploadClient {
    client: Arc<Mutex<ClientMetricsUploaderClient<Channel>>>,
}

impl GrpcUploadClient {
    pub fn new(address: String) -> Self {
        let address = address
            .strip_prefix("dns:///")
            .map_or(address.clone(), |authority| format!("http://{authority}"));
        let channel = Endpoint::from_shared(address)
            .expect("gRPC server address must be a valid URI")
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .connect_lazy();

        Self {
            client: Arc::new(Mutex::new(ClientMetricsUploaderClient::new(channel))),
        }
    }

    pub async fn send(
        &self,
        device_id: DeviceId,
        events: &[Event],
    ) -> Result<Vec<EventId>, ClientError> {
        let mut messages = Vec::new();
        for event in events {
            messages.push(Self::convert_event(event));
        }
        let request = UploadEventsRequest {
            device_id: device_id.0.as_hyphenated().to_string(),
            events: messages,
        };
        let response = self.client.lock().await.upload_events(request).await?;
        Ok(response
            .into_inner()
            .accepted_event_ids
            .iter()
            .map(|uuid_str| Uuid::parse_str(uuid_str).ok())
            .filter(|uuid_opt| uuid_opt.is_some())
            .map(|uuid_opt| EventId(uuid_opt.unwrap()))
            .collect())
    }

    fn convert_event(event: &Event) -> EventMessage {
        let id = event.id.0.as_hyphenated().to_string();
        let event_device_id = event.device_id.0.as_hyphenated().to_string();
        let occurred_at: i64 = event.occurred_at.unix_timestamp_nanos().try_into().unwrap();
        let kind = match event.kind {
            EventKind::CollectorStarted => Some(event_message::Kind::CollectorStarted(true)),
            EventKind::CollectorStopped => Some(event_message::Kind::CollectorStopped(true)),
            EventKind::Heartbeat => Some(event_message::Kind::Heartbeat(true)),
            EventKind::Observation(ref kind) => match kind {
                ObservationKind::IdleChanged { idle } => {
                    Some(event_message::Kind::IdleChanged(*idle))
                }
                ObservationKind::FocusedApplicationChanged { app_id, title } => {
                    Some(event_message::Kind::FocusedApplicationChanged(
                        FocusedApplicationChangedMessage {
                            app_id: app_id.to_string(),
                            title: title.clone(),
                        },
                    ))
                }
                ObservationKind::KeyboardButtonPressed { key } => Some(
                    event_message::Kind::KeyboardButtonPressed(KeyboardButtonPressedMessage {
                        key: key.to_string(),
                    }),
                ),
                ObservationKind::MouseClicked { button, x, y } => {
                    Some(event_message::Kind::MouseClicked(MouseClickedMessage {
                        button: button.to_string(),
                        x: *x,
                        y: *y,
                    }))
                }
                ObservationKind::MouseMoved { x, y } => {
                    Some(event_message::Kind::MouseMoved(MouseMovedMessage {
                        x: *x,
                        y: *y,
                    }))
                }
                ObservationKind::SystemSleepChanged { sleeping } => {
                    Some(event_message::Kind::SystemSleepChanged(*sleeping))
                }
                ObservationKind::ActiveBrowserTabChanged {
                    source_id,
                    window_id,
                    tab_id,
                    title,
                } => Some(event_message::Kind::ActiveBrowserTabChanged(
                    ActiveBrowserTabChangedMessage {
                        source_id: source_id.to_string(),
                        window_id: *window_id,
                        tab_id: *tab_id,
                        title: title.clone(),
                    },
                )),
                ObservationKind::ActiveBrowserTabTitleChanged {
                    source_id,
                    window_id,
                    tab_id,
                    title,
                } => Some(event_message::Kind::ActiveBrowserTabTitleChanged(
                    ActiveBrowserTabTitleChangedMessage {
                        source_id: source_id.to_string(),
                        window_id: *window_id,
                        tab_id: *tab_id,
                        title: title.clone(),
                    },
                )),
                ObservationKind::BrowserWindowFocusChanged {
                    source_id,
                    window_id,
                    focused,
                } => Some(event_message::Kind::BrowserWindowFocusChanged(
                    BrowserWindowFocusChangedMessage {
                        source_id: source_id.to_string(),
                        window_id: *window_id,
                        focused: *focused,
                    },
                )),
            },
            EventKind::AdapterStatus {
                ref adapter_id,
                observation_type,
                ref status,
            } => Some(event_message::Kind::AdapterStatus(AdapterStatusMessage {
                adapter_id: adapter_id.to_string(),
                observation_type: match observation_type {
                    ObservationType::Idle => ObservationTypeProto::Idle,
                    ObservationType::FocusedApplication => ObservationTypeProto::FocusedApplication,
                    ObservationType::Keyboard => ObservationTypeProto::Keyboard,
                    ObservationType::MouseClick => ObservationTypeProto::MouseClick,
                    ObservationType::MouseMove => ObservationTypeProto::MouseMove,
                    ObservationType::BrowserActivity => ObservationTypeProto::BrowserActivity,
                }
                .into(),
                status: match status {
                    AdapterStatus::Available => {
                        Some(adapter_status_message::Status::Available(true))
                    }
                    AdapterStatus::Unavailable { reason } => Some(
                        adapter_status_message::Status::UnavailableReason(reason.to_string()),
                    ),
                },
            })),
        };
        EventMessage {
            id,
            device_id: event_device_id,
            occurred_at_unix_nanos: occurred_at,
            kind,
        }
    }
}
