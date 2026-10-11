use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct DeviceId(pub Uuid);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ObservationType {
    Idle,
    FocusedApplication,
    Keyboard,
    MouseClick,
    MouseMove,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AdapterStatus {
    Available,
    Unavailable { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ObservationKind {
    IdleChanged {
        idle: bool,
    },
    FocusedApplicationChanged {
        app_id: String,
        title: Option<String>,
    },
    KeyboardButtonPressed {/* TODO */},
    MouseClicked {/* TODO */},
    MouseMoved {/* TODO */},
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub occurred_at: OffsetDateTime,
    pub kind: ObservationKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    CollectorStarted,
    CollectorStopped,
    Heartbeat,
    Observation(ObservationKind),
    AdapterStatus {
        adapter_id: String,
        observation_type: ObservationType,
        status: AdapterStatus,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub device_id: DeviceId,
    pub occurred_at: OffsetDateTime,
    pub kind: EventKind,
}
