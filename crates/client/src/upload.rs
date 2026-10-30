use protobuf::proto;

mod protos {
    include!("../generated/generated.rs");
}

use std::sync::Arc;
use uuid::Uuid;
use grpc::client::Channel;
use grpc::client::ChannelOptions;
use grpc::credentials::LocalChannelCredentials;
use grpc_protobuf::UnaryCallBuilder;

use timetracker_model::{DeviceId, Event, EventId};

use crate::ClientError;

pub struct GrpcUploadClient {
    channel: Channel
}

impl GrpcUploadClient {
    pub fn new(address: String) -> GrpcUploadClient {
        GrpcUploadClient {
            channel:
                Channel::new(
                    address,
                    Arc::new(LocalChannelCredentials::new()),
                    ChannelOptions::default()
                )
        }
    }

    pub async fn send(
        &self,
        device_id: DeviceId,
        _events: &[Event],
    ) -> Result<Vec<EventId>, ClientError> {
    let mut messages = Vec::new();        
        for event in _events {
            let id = event.id.0.as_hyphenated().to_string();
            let event_device_id = event.device_id.0.as_hyphenated().to_string();
            let occurred_at: i64 = event.occurred_at.unix_timestamp_nanos().try_into().unwrap();

            let event_message = match event.kind {
                timetracker_model::EventKind::CollectorStarted => proto!(protos::EventMessage {
                    id: id,
                    device_id: event_device_id,
                    occurred_at_unix_nanos: occurred_at,
                    collector_started: true,
                }),
                timetracker_model::EventKind::CollectorStopped => proto!(protos::EventMessage {
                    id: id,
                    device_id: event_device_id,
                    occurred_at_unix_nanos: occurred_at,
                    collector_stopped: true,
                }),
                timetracker_model::EventKind::Heartbeat => proto!(protos::EventMessage {
                    id: id,
                    device_id: event_device_id,
                    occurred_at_unix_nanos: occurred_at,
                    heartbeat: true,
                }),
                timetracker_model::EventKind::Observation(ref kind) => 
                    match kind {
                        timetracker_model::ObservationKind::IdleChanged { idle } => proto!(protos::EventMessage {
                            id: id,
                            device_id: event_device_id,
                            occurred_at_unix_nanos: occurred_at,
                            idle_changed: *idle,
                        }),
                        timetracker_model::ObservationKind::FocusedApplicationChanged { app_id, title } => proto!(protos::EventMessage {
                            id: id,
                            device_id: event_device_id,
                            occurred_at_unix_nanos: occurred_at,
                            focused_application_changed: protos::FocusedApplicationChangedMessage {
                                app_id: app_id,
                                title: title.clone().unwrap_or("".to_string()),
                            }       
                        }),
                        timetracker_model::ObservationKind::KeyboardButtonPressed {} => proto!(protos::EventMessage {
                            id: id,
                            device_id: event_device_id,
                            occurred_at_unix_nanos: occurred_at,
                            keyboard_button_pressed: true,
                        }),
                        timetracker_model::ObservationKind::MouseClicked {} => proto!(protos::EventMessage {
                            id: id,
                            device_id: event_device_id,
                            occurred_at_unix_nanos: occurred_at,
                            mouse_clicked: true,
                        }),
                        timetracker_model::ObservationKind::MouseMoved {} => proto!(protos::EventMessage {
                            id: id,
                            device_id: event_device_id,
                            occurred_at_unix_nanos: occurred_at,
                            mouse_moved: true,
                        }),
                    },
                timetracker_model::EventKind::AdapterStatus {
                    ref adapter_id,
                    observation_type,
                    ref status
                } => {
                    let observation_type = match observation_type {
                        timetracker_model::ObservationType::Idle => protos::ObservationTypeProto::Idle,
                        timetracker_model::ObservationType::FocusedApplication => protos::ObservationTypeProto::FocusedApplication,
                        timetracker_model::ObservationType::Keyboard => protos::ObservationTypeProto::Keyboard,
                        timetracker_model::ObservationType::MouseClick => protos::ObservationTypeProto::MouseClick,
                        timetracker_model::ObservationType::MouseMove => protos::ObservationTypeProto::MouseMove
                    };
                    let adapter_status = match status {
                        timetracker_model::AdapterStatus::Available => proto!(protos::AdapterStatusMessage {
                            adapter_id: adapter_id,
                            observation_type: observation_type,
                            available: true,
                        }),
                        timetracker_model::AdapterStatus::Unavailable { reason } => proto!(protos::AdapterStatusMessage {
                            adapter_id: adapter_id,
                            observation_type: observation_type,
                            unavailable_reason: reason,
                        }),
                    };
                    proto!(protos::EventMessage {
                        id: id,
                        device_id: event_device_id,
                        occurred_at_unix_nanos: occurred_at,
                        adapter_status: adapter_status,                        
                    })
                },
            };
            messages.push(event_message);
        };
        let request = proto!(protos::UploadEventsRequest {
            device_id: device_id.0.as_hyphenated().to_string(),
            events: messages.into_iter(),
        });
        let response: Result<protos::UploadEventsResponse, _> = UnaryCallBuilder::new(&self.channel, "/timetracker.metrics.ClientMetricsUploader/UploadEvents", request.as_view()).await;
        match response {
            Ok(response) => Ok(
                response.accepted_event_ids()
                .iter()
                .map(|proto_str| Uuid::parse_str(proto_str.to_str().ok()?).ok())
                .filter(|uuid_opt| uuid_opt.is_some())
                .map(|uuid_opt| EventId(uuid_opt.unwrap()))
                .collect()
                ),
            Err(e) => Err(e.message().into()),
        }
    }
}
