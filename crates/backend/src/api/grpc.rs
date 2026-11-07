#![allow(
    clippy::result_large_err,
    reason = "This gRPC module uses tonic::Status for transport errors"
)]

use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use time::OffsetDateTime;
use timetracker_model::{
    AdapterStatus, DeviceId, Event, EventId, EventKind, ObservationKind, ObservationType,
};
use tokio::sync::watch;
use tonic::{Request, Response, Status, transport::Server};
use uuid::Uuid;

use crate::{
    BackendError,
    api::ApiServer,
    collection::{CollectionError, MetricsCollectionService},
};
pub mod proto {
    tonic::include_proto!("timetracker.metrics");
}

use proto::{
    AdapterStatusMessage, EventMessage, ObservationTypeProto, UploadEventsRequest,
    UploadEventsResponse, adapter_status_message,
    client_metrics_uploader_server::{ClientMetricsUploader, ClientMetricsUploaderServer},
    event_message,
};

#[derive(Clone)]
pub struct GrpcMetricsServer {
    pub(crate) collection: Arc<MetricsCollectionService>,
}

impl GrpcMetricsServer {
    pub fn new(collection: Arc<MetricsCollectionService>) -> Self {
        Self { collection }
    }
}

fn parse_uuid(raw: &str, field_name: &str) -> Result<Uuid, Status> {
    if raw.trim().is_empty() {
        return Err(Status::invalid_argument(format!(
            "Field '{field_name}' cannot be empty"
        )));
    }
    Uuid::parse_str(raw)
        .map_err(|e| Status::invalid_argument(format!("Invalid UUID in '{field_name}': {e}")))
}

fn convert_proto_event(
    proto_event: EventMessage,
    batch_device_id: DeviceId,
) -> Result<Event, Status> {
    let event_id = EventId(parse_uuid(&proto_event.id, "events[].id")?);
    let event_device_id = DeviceId(parse_uuid(&proto_event.device_id, "events[].device_id")?);

    if event_device_id != batch_device_id {
        return Err(Status::invalid_argument(format!(
            "Event {} has device_id {} which does not match request device_id {}",
            event_id.0, event_device_id.0, batch_device_id.0
        )));
    }

    let occurred_at =
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(proto_event.occurred_at_unix_nanos))
            .map_err(|e| {
            Status::invalid_argument(format!("Invalid timestamp in event {}: {e}", event_id.0))
        })?;

    let proto_kind = proto_event.kind.ok_or_else(|| {
        Status::invalid_argument(format!("Missing event kind in event {}", event_id.0))
    })?;

    let kind = match proto_kind {
        event_message::Kind::CollectorStarted(_) => EventKind::CollectorStarted,
        event_message::Kind::CollectorStopped(_) => EventKind::CollectorStopped,
        event_message::Kind::Heartbeat(_) => EventKind::Heartbeat,
        event_message::Kind::IdleChanged(idle) => {
            EventKind::Observation(ObservationKind::IdleChanged { idle })
        }
        event_message::Kind::FocusedApplicationChanged(app) => {
            if app.app_id.trim().is_empty() {
                return Err(Status::invalid_argument(
                    "FocusedApplicationChanged.app_id cannot be empty",
                ));
            }
            EventKind::Observation(ObservationKind::FocusedApplicationChanged {
                app_id: app.app_id,
                title: app.title,
            })
        }
        event_message::Kind::KeyboardButtonPressed(_) => {
            EventKind::Observation(ObservationKind::KeyboardButtonPressed {})
        }
        event_message::Kind::MouseClicked(_) => {
            EventKind::Observation(ObservationKind::MouseClicked {})
        }
        event_message::Kind::MouseMoved(_) => {
            EventKind::Observation(ObservationKind::MouseMoved {})
        }
        event_message::Kind::AdapterStatus(status_msg) => convert_adapter_status(status_msg)?,
    };

    Ok(Event {
        id: event_id,
        device_id: event_device_id,
        occurred_at,
        kind,
    })
}

fn convert_adapter_status(msg: AdapterStatusMessage) -> Result<EventKind, Status> {
    if msg.adapter_id.trim().is_empty() {
        return Err(Status::invalid_argument("adapter_id cannot be empty"));
    }

    let obs_type_proto = ObservationTypeProto::try_from(msg.observation_type).map_err(|_| {
        Status::invalid_argument(format!(
            "Unknown observation_type value: {}",
            msg.observation_type
        ))
    })?;

    let observation_type = match obs_type_proto {
        ObservationTypeProto::ObservationTypeUnspecified => {
            return Err(Status::invalid_argument(
                "observation_type must be specified",
            ));
        }
        ObservationTypeProto::Idle => ObservationType::Idle,
        ObservationTypeProto::FocusedApplication => ObservationType::FocusedApplication,
        ObservationTypeProto::Keyboard => ObservationType::Keyboard,
        ObservationTypeProto::MouseClick => ObservationType::MouseClick,
        ObservationTypeProto::MouseMove => ObservationType::MouseMove,
    };

    let status = match msg.status {
        Some(adapter_status_message::Status::Available(true)) => AdapterStatus::Available,
        Some(adapter_status_message::Status::Available(false)) => {
            return Err(Status::invalid_argument(
                "Available flag cannot be false without a reason",
            ));
        }
        Some(adapter_status_message::Status::UnavailableReason(reason)) => {
            AdapterStatus::Unavailable { reason }
        }
        None => {
            return Err(Status::invalid_argument(
                "AdapterStatusMessage.status must be set",
            ));
        }
    };

    Ok(EventKind::AdapterStatus {
        adapter_id: msg.adapter_id,
        observation_type,
        status,
    })
}

#[tonic::async_trait]
impl ClientMetricsUploader for GrpcMetricsServer {
    async fn upload_events(
        &self,
        request: Request<UploadEventsRequest>,
    ) -> Result<Response<UploadEventsResponse>, Status> {
        let payload = request.into_inner();

        let batch_device_id = DeviceId(parse_uuid(&payload.device_id, "device_id")?);

        let domain_events: Vec<Event> = payload
            .events
            .into_iter()
            .map(|raw_event| convert_proto_event(raw_event, batch_device_id))
            .collect::<Result<Vec<Event>, Status>>()?;

        let receipt = self
            .collection
            .collect(batch_device_id, &domain_events)
            .await
            .map_err(|err| {
                if let Some(collection_err) = err.downcast_ref::<CollectionError>() {
                    match collection_err {
                        CollectionError::UnknownDevice(_) => {
                            Status::not_found(collection_err.to_string())
                        }
                        CollectionError::DeviceMismatch { .. } => {
                            Status::invalid_argument(collection_err.to_string())
                        }
                    }
                } else {
                    Status::internal(format!("Failed to collect events: {err}"))
                }
            })?;

        let accepted_event_ids = receipt
            .accepted
            .into_iter()
            .map(|id| id.0.to_string())
            .collect();

        Ok(Response::new(UploadEventsResponse { accepted_event_ids }))
    }
}

#[async_trait]
impl ApiServer for GrpcMetricsServer {
    async fn serve(
        &self,
        address: SocketAddr,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), BackendError> {
        let svc = ClientMetricsUploaderServer::new(self.clone());

        Server::builder()
            .add_service(svc)
            .serve_with_shutdown(address, async move {
                while !*shutdown.borrow_and_update() {
                    if shutdown.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::{DeviceRepository, EventRepository};
    use std::collections::HashMap;
    use timetracker_model::UserId;
    use tonic::Code;

    struct TestDeviceRepo(HashMap<DeviceId, UserId>);
    #[async_trait]
    impl DeviceRepository for TestDeviceRepo {
        async fn owner_of(&self, device_id: DeviceId) -> Result<Option<UserId>, BackendError> {
            Ok(self.0.get(&device_id).copied())
        }
    }

    struct TestEventRepo;
    #[async_trait]
    impl EventRepository for TestEventRepo {
        async fn insert_batch(
            &self,
            _user_id: UserId,
            events: &[Event],
        ) -> Result<Vec<EventId>, BackendError> {
            Ok(events.iter().map(|e| e.id).collect())
        }
    }

    fn setup_grpc_server(known_device: Option<DeviceId>) -> GrpcMetricsServer {
        let mut map = HashMap::new();
        if let Some(dev) = known_device {
            map.insert(dev, UserId(Uuid::new_v4()));
        }
        let service = Arc::new(MetricsCollectionService::new(
            Arc::new(TestDeviceRepo(map)),
            Arc::new(TestEventRepo),
        ));
        GrpcMetricsServer::new(service)
    }

    fn valid_proto_event(event_id: Uuid, device_id: Uuid) -> EventMessage {
        EventMessage {
            id: event_id.to_string(),
            device_id: device_id.to_string(),
            occurred_at_unix_nanos: 1_700_000_000_000_000_000,
            kind: Some(event_message::Kind::Heartbeat(true)),
        }
    }

    #[tokio::test]
    async fn upload_events_succeeds_for_valid_batch() {
        let device_uuid = Uuid::new_v4();
        let event_uuid = Uuid::new_v4();
        let server = setup_grpc_server(Some(DeviceId(device_uuid)));

        let req = Request::new(UploadEventsRequest {
            device_id: device_uuid.to_string(),
            events: vec![valid_proto_event(event_uuid, device_uuid)],
        });

        let response = server.upload_events(req).await.unwrap().into_inner();
        assert_eq!(response.accepted_event_ids, vec![event_uuid.to_string()]);
    }

    #[tokio::test]
    async fn upload_events_rejects_malformed_uuid_with_invalid_argument() {
        let device_uuid = Uuid::new_v4();
        let server = setup_grpc_server(Some(DeviceId(device_uuid)));

        let mut bad_event = valid_proto_event(Uuid::new_v4(), device_uuid);
        bad_event.id = "not-a-valid-uuid".to_string();

        let req = Request::new(UploadEventsRequest {
            device_id: device_uuid.to_string(),
            events: vec![valid_proto_event(Uuid::new_v4(), device_uuid), bad_event],
        });

        let err = server.upload_events(req).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn upload_events_rejects_unknown_device_with_not_found() {
        let server = setup_grpc_server(None);
        let unknown_device = Uuid::new_v4();

        let req = Request::new(UploadEventsRequest {
            device_id: unknown_device.to_string(),
            events: vec![valid_proto_event(Uuid::new_v4(), unknown_device)],
        });

        let err = server.upload_events(req).await.unwrap_err();
        assert_eq!(err.code(), Code::NotFound);
    }
}
