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
        _device_id: DeviceId,
        _events: &[Event],
    ) -> Result<Vec<EventId>, ClientError> {
    let mut res = Vec::new();        
        for event in _events {
            let (event_id_hi, event_id_lo) = event.id.0.as_u64_pair();
            let id = proto!(protos::Uuid {
                hi: event_id_hi,
                lo: event_id_lo
            });
            let (device_id_hi, device_id_lo) = event.device_id.0.as_u64_pair();
            let device_id = proto!(protos::Uuid {
                hi: device_id_hi,
                lo: device_id_lo
            });
            let occurred_at = event.occurred_at.unix_timestamp();

            let _event = match event.kind {
                timetracker_model::EventKind::CollectorStarted => proto!(protos::Event {
                    id: id,
                    device_id: device_id,
                    occurred_at: occurred_at,
                    collectorStarted: protos::CollectorStarted {},
                }),
                timetracker_model::EventKind::CollectorStopped => proto!(protos::Event {
                    id: id,
                    device_id: device_id,
                    occurred_at: occurred_at,
                    collectorStopped: protos::CollectorStopped {},
                }),
                timetracker_model::EventKind::Heartbeat => proto!(protos::Event {
                    id: id,
                    device_id: device_id,
                    occurred_at: occurred_at,
                    heartbeat: protos::Heartbeat {},
                }),
                timetracker_model::EventKind::Observation(ref kind) => {
                    let observation = match kind {
                        timetracker_model::ObservationKind::IdleChanged { idle } => proto!(protos::Observation {
                            idleChanged: protos::IdleChanged {
                                idle: *idle
                            }
                        }),
                        timetracker_model::ObservationKind::FocusedApplicationChanged { app_id, title } =>
                        match title {
                            Some(title) =>
                                proto!(protos::Observation {
                                    focusedApplicationChanged: protos::FocusedApplicationChanged {
                                        app_id: app_id,
                                        title: title
                                    }
                                }),
                            None =>
                                proto!(protos::Observation {
                                    focusedApplicationChanged: protos::FocusedApplicationChanged {
                                        app_id: app_id
                                    }
                                }),
                        },
                        timetracker_model::ObservationKind::KeyboardButtonPressed {} => proto!(protos::Observation {
                            keyboardButtonPressed: protos::KeyboardButtonPressed {}
                        }),
                        timetracker_model::ObservationKind::MouseClicked {} => proto!(protos::Observation {
                            mouseClicked: protos::MouseClicked {}
                        }),
                        timetracker_model::ObservationKind::MouseMoved {} => proto!(protos::Observation {
                            mouseMoved: protos::MouseMoved {}
                        }),
                    };
                    proto!(protos::Event {
                        id: id,
                        device_id: device_id,
                        occurred_at: occurred_at,
                        observation: observation
                    })
                },
                timetracker_model::EventKind::AdapterStatus {
                    ref adapter_id,
                    observation_type,
                    ref status
                } => {
                    let observation_type = match observation_type {
                        timetracker_model::ObservationType::Idle => protos::ObservationType::Idle,
                        timetracker_model::ObservationType::FocusedApplication => protos::ObservationType::FocusedApplication,
                        timetracker_model::ObservationType::Keyboard => protos::ObservationType::Keyboard,
                        timetracker_model::ObservationType::MouseClick => protos::ObservationType::MouseClick,
                        timetracker_model::ObservationType::MouseMove => protos::ObservationType::MouseMove
                    };
                    let adapter_status = match status {
                        timetracker_model::AdapterStatus::Available => proto!(protos::AdapterStatusEvent {
                            adapter_id: adapter_id,
                            observationType: observation_type,
                            status: protos::AdapterStatus {
                                available: protos::Available {}
                            }
                        }),
                        timetracker_model::AdapterStatus::Unavailable { reason } => proto!(protos::AdapterStatusEvent {
                            adapter_id: adapter_id,
                            observationType: observation_type,
                            status: protos::AdapterStatus {
                                unavailable: protos::Unavailable {
                                    reason: reason
                                }
                            }
                        })
                    };
                    proto!(protos::Event {
                        id: id,
                        device_id: device_id,
                        occurred_at: occurred_at,
                        adapterStatus: adapter_status
                    })
                }
            };

            let _res: Result<protos::Uuid, _> = UnaryCallBuilder::new(&self.channel, "/upload_client.Upload/Send", _event.as_view()).await;
            match _res {
                Ok(uuid) => res.push(EventId(Uuid::from_u64_pair(uuid.hi(), uuid.lo()))),
                Err(_) => ()
            }
        }
        Ok(res)
    }
}
