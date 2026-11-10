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
    BrowserActivity,
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
    KeyboardButtonPressed {
        key: String,
    },
    MouseClicked {
        button: String,
        x: Option<i32>,
        y: Option<i32>,
    },
    MouseMoved {
        x: i32,
        y: i32,
    },
    SystemSleepChanged {
        sleeping: bool,
    },
    // TODO(browser-protobuf): Add BrowserActivity and the browser observation variants below to
    // metrics.proto, then implement client encoding and backend decoding for them.
    ActiveBrowserTabChanged {
        source_id: String,
        window_id: i64,
        tab_id: i64,
        title: Option<String>,
    },
    ActiveBrowserTabTitleChanged {
        source_id: String,
        window_id: i64,
        tab_id: i64,
        title: Option<String>,
    },
    BrowserWindowFocusChanged {
        source_id: String,
        window_id: Option<i64>,
        focused: bool,
    },
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
