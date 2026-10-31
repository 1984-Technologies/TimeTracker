use std::io;

use timetracker_model::{
    AdapterStatus, DeviceId, Event, EventId, EventKind, ObservationKind, ObservationType,
};
use tonic::transport::{Channel, Endpoint};

use crate::ClientError;

pub mod proto {
    tonic::include_proto!("timetracker.metrics");
}

use proto::{
    AdapterStatusMessage, EventMessage, FocusedApplicationChangedMessage, ObservationTypeProto,
    UploadEventsRequest, adapter_status_message,
    client_metrics_uploader_client::ClientMetricsUploaderClient, event_message,
};

pub struct GrpcUploadClient {
    client: ClientMetricsUploaderClient<Channel>,
}

impl GrpcUploadClient {
    pub async fn connect(address: impl Into<String>) -> Result<Self, ClientError> {
        let address = normalize_address(address.into());
        let channel = Endpoint::from_shared(address)?.connect_lazy();
        let client = ClientMetricsUploaderClient::new(channel);
        Ok(Self { client })
    }

    pub async fn send(
        &self,
        device_id: DeviceId,
        events: &[Event],
    ) -> Result<Vec<EventId>, ClientError> {
        let events = events
            .iter()
            .map(to_proto_event)
            .collect::<Result<Vec<_>, ClientError>>()?;
        let response = self
            .client
            .clone()
            .upload_events(UploadEventsRequest {
                device_id: device_id.0.to_string(),
                events,
            })
            .await?
            .into_inner();

        response
            .accepted_event_ids
            .into_iter()
            .map(|id| {
                uuid::Uuid::parse_str(&id)
                    .map(EventId)
                    .map_err(|error| Box::new(error) as ClientError)
            })
            .collect()
    }
}

fn normalize_address(address: String) -> String {
    if address.contains("://") {
        address
    } else {
        format!("http://{address}")
    }
}

fn to_proto_event(event: &Event) -> Result<EventMessage, ClientError> {
    let occurred_at_unix_nanos =
        i64::try_from(event.occurred_at.unix_timestamp_nanos()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "event timestamp is outside i64 range",
            )
        })?;

    let kind = match &event.kind {
        EventKind::CollectorStarted => event_message::Kind::CollectorStarted(true),
        EventKind::CollectorStopped => event_message::Kind::CollectorStopped(true),
        EventKind::Heartbeat => event_message::Kind::Heartbeat(true),
        EventKind::Observation(ObservationKind::IdleChanged { idle }) => {
            event_message::Kind::IdleChanged(*idle)
        }
        EventKind::Observation(ObservationKind::FocusedApplicationChanged { app_id, title }) => {
            event_message::Kind::FocusedApplicationChanged(FocusedApplicationChangedMessage {
                app_id: app_id.clone(),
                title: title.clone(),
            })
        }
        EventKind::Observation(ObservationKind::KeyboardButtonPressed { .. }) => {
            event_message::Kind::KeyboardButtonPressed(true)
        }
        EventKind::Observation(ObservationKind::MouseClicked { .. }) => {
            event_message::Kind::MouseClicked(true)
        }
        EventKind::Observation(ObservationKind::MouseMoved { .. }) => {
            event_message::Kind::MouseMoved(true)
        }
        EventKind::Observation(ObservationKind::SystemSleepChanged { sleeping }) => {
            event_message::Kind::SystemSleepChanged(*sleeping)
        }
        EventKind::AdapterStatus {
            adapter_id,
            observation_type,
            status,
        } => {
            let observation_type = match observation_type {
                ObservationType::Idle => ObservationTypeProto::Idle,
                ObservationType::FocusedApplication => ObservationTypeProto::FocusedApplication,
                ObservationType::Keyboard => ObservationTypeProto::Keyboard,
                ObservationType::MouseClick => ObservationTypeProto::MouseClick,
                ObservationType::MouseMove => ObservationTypeProto::MouseMove,
            } as i32;
            let status = match status {
                AdapterStatus::Available => adapter_status_message::Status::Available(true),
                AdapterStatus::Unavailable { reason } => {
                    adapter_status_message::Status::UnavailableReason(reason.clone())
                }
            };
            event_message::Kind::AdapterStatus(AdapterStatusMessage {
                adapter_id: adapter_id.clone(),
                observation_type,
                status: Some(status),
            })
        }
    };

    Ok(EventMessage {
        id: event.id.0.to_string(),
        device_id: event.device_id.0.to_string(),
        occurred_at_unix_nanos,
        kind: Some(kind),
    })
}
