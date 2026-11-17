use std::{io, time::Duration};

use timetracker_model::{AdapterStatus, DeviceId, Event, EventId, EventKind, ObservationKind};
use timetracker_proto::metrics::{
    ActiveBrowserTabChangedMessage, ActiveBrowserTabTitleChangedMessage, AdapterStatusMessage,
    BrowserWindowFocusChangedMessage, EventMessage, FocusedApplicationChangedMessage,
    KeyboardButtonPressedMessage, MouseClickedMessage, MouseMovedMessage, ObservationTypeProto,
    UploadEventsRequest, adapter_status_message,
    client_metrics_uploader_client::ClientMetricsUploaderClient, event_message,
};
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use crate::ClientError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct GrpcUploadClient {
    client: ClientMetricsUploaderClient<Channel>,
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
            client: ClientMetricsUploaderClient::new(channel),
        }
    }

    pub async fn send(
        &self,
        device_id: DeviceId,
        events: &[Event],
    ) -> Result<Vec<EventId>, ClientError> {
        let request = UploadEventsRequest {
            device_id: device_id.0.as_hyphenated().to_string(),
            events: events
                .iter()
                .map(event_to_proto)
                .collect::<Result<_, _>>()?,
        };

        let response = self
            .client
            .clone()
            .upload_events(request)
            .await?
            .into_inner();

        response
            .accepted_event_ids
            .into_iter()
            .map(|raw_id| {
                Uuid::parse_str(&raw_id).map(EventId).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("server returned invalid accepted event id '{raw_id}': {error}"),
                    )
                    .into()
                })
            })
            .collect()
    }
}

fn event_to_proto(event: &Event) -> Result<EventMessage, ClientError> {
    let occurred_at_unix_nanos = event
        .occurred_at
        .unix_timestamp_nanos()
        .try_into()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "event {} timestamp does not fit into i64: {error}",
                    event.id.0
                ),
            )
        })?;

    let kind = match &event.kind {
        EventKind::CollectorStarted => event_message::Kind::CollectorStarted(true),
        EventKind::CollectorStopped => event_message::Kind::CollectorStopped(true),
        EventKind::Heartbeat => event_message::Kind::Heartbeat(true),
        EventKind::Observation(observation) => observation_to_proto(observation),
        EventKind::AdapterStatus {
            adapter_id,
            observation_type,
            status,
        } => {
            let observation_type = match observation_type {
                timetracker_model::ObservationType::Idle => ObservationTypeProto::Idle,
                timetracker_model::ObservationType::FocusedApplication => {
                    ObservationTypeProto::FocusedApplication
                }
                timetracker_model::ObservationType::Keyboard => ObservationTypeProto::Keyboard,
                timetracker_model::ObservationType::MouseClick => ObservationTypeProto::MouseClick,
                timetracker_model::ObservationType::MouseMove => ObservationTypeProto::MouseMove,
                timetracker_model::ObservationType::BrowserActivity => {
                    ObservationTypeProto::BrowserActivity
                }
            };
            let status = match status {
                AdapterStatus::Available => adapter_status_message::Status::Available(true),
                AdapterStatus::Unavailable { reason } => {
                    adapter_status_message::Status::UnavailableReason(reason.clone())
                }
            };

            event_message::Kind::AdapterStatus(AdapterStatusMessage {
                adapter_id: adapter_id.clone(),
                observation_type: observation_type.into(),
                status: Some(status),
            })
        }
    };

    Ok(EventMessage {
        id: event.id.0.as_hyphenated().to_string(),
        device_id: event.device_id.0.as_hyphenated().to_string(),
        occurred_at_unix_nanos,
        kind: Some(kind),
    })
}

fn observation_to_proto(observation: &ObservationKind) -> event_message::Kind {
    match observation {
        ObservationKind::IdleChanged { idle } => event_message::Kind::IdleChanged(*idle),
        ObservationKind::FocusedApplicationChanged { app_id, title } => {
            event_message::Kind::FocusedApplicationChanged(FocusedApplicationChangedMessage {
                app_id: app_id.clone(),
                title: title.clone(),
            })
        }
        ObservationKind::KeyboardButtonPressed { key } => {
            event_message::Kind::KeyboardButtonPressed(KeyboardButtonPressedMessage {
                key: key.clone(),
            })
        }
        ObservationKind::MouseClicked { button, x, y } => {
            event_message::Kind::MouseClicked(MouseClickedMessage {
                button: button.clone(),
                x: *x,
                y: *y,
            })
        }
        ObservationKind::MouseMoved { x, y } => {
            event_message::Kind::MouseMoved(MouseMovedMessage { x: *x, y: *y })
        }
        ObservationKind::SystemSleepChanged { sleeping } => {
            event_message::Kind::SystemSleepChanged(*sleeping)
        }
        ObservationKind::ActiveBrowserTabChanged {
            source_id,
            window_id,
            tab_id,
            title,
        } => event_message::Kind::ActiveBrowserTabChanged(ActiveBrowserTabChangedMessage {
            source_id: source_id.clone(),
            window_id: *window_id,
            tab_id: *tab_id,
            title: title.clone(),
        }),
        ObservationKind::ActiveBrowserTabTitleChanged {
            source_id,
            window_id,
            tab_id,
            title,
        } => {
            event_message::Kind::ActiveBrowserTabTitleChanged(ActiveBrowserTabTitleChangedMessage {
                source_id: source_id.clone(),
                window_id: *window_id,
                tab_id: *tab_id,
                title: title.clone(),
            })
        }
        ObservationKind::BrowserWindowFocusChanged {
            source_id,
            window_id,
            focused,
        } => event_message::Kind::BrowserWindowFocusChanged(BrowserWindowFocusChangedMessage {
            source_id: source_id.clone(),
            window_id: *window_id,
            focused: *focused,
        }),
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn preserves_absent_optional_title() {
        let event = Event {
            id: EventId(Uuid::new_v4()),
            device_id: DeviceId(Uuid::new_v4()),
            occurred_at: OffsetDateTime::UNIX_EPOCH,
            kind: EventKind::Observation(ObservationKind::FocusedApplicationChanged {
                app_id: "com.example.App".to_owned(),
                title: None,
            }),
        };

        let encoded = event_to_proto(&event).unwrap();
        let Some(event_message::Kind::FocusedApplicationChanged(app)) = encoded.kind else {
            panic!("expected a focused-application observation");
        };
        assert_eq!(app.app_id, "com.example.App");
        assert_eq!(app.title, None);
    }
}
