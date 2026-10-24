use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use active_win_pos_rs::get_active_window;
use async_trait::async_trait;
use rdev::{EventType, listen};
use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, Observation, ObservationKind, ObservationType};
use tokio::sync::{broadcast, mpsc, watch};

use super::{AdapterOutput, DesktopAdapter};
use crate::ClientError;

const WINDOW_POLL_INTERVAL: Duration = Duration::from_millis(500);
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_IDLE_THRESHOLD: Duration = Duration::from_secs(60);
const INPUT_CHANNEL_CAPACITY: usize = 4096;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
}

fn unavailable(reason: impl Into<String>) -> AdapterStatus {
    AdapterStatus::Unavailable {
        reason: reason.into(),
    }
}

async fn send_status(
    output: &mpsc::Sender<AdapterOutput>,
    adapter_id: &'static str,
    observation_type: ObservationType,
    status: AdapterStatus,
) -> bool {
    output
        .send(AdapterOutput::Status {
            adapter_id,
            observation_type,
            status,
        })
        .await
        .is_ok()
}

async fn send_observation(
    output: &mpsc::Sender<AdapterOutput>,
    occurred_at: OffsetDateTime,
    kind: ObservationKind,
) -> bool {
    output
        .send(AdapterOutput::Observation(Observation {
            occurred_at,
            kind,
        }))
        .await
        .is_ok()
}

fn shutdown_requested(shutdown: &watch::Receiver<bool>) -> bool {
    *shutdown.borrow()
}

fn input_permission_available() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

fn screen_capture_permission_available() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

fn idle_seconds() -> Option<f64> {
    let seconds = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
}

#[derive(Debug, Default)]
pub struct FocusedApplicationAdapter {
    previous: Option<(String, Option<String>)>,
}

impl FocusedApplicationAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    fn current_window() -> Result<(String, Option<String>), String> {
        if !screen_capture_permission_available() {
            return Err(
                "Screen Recording permission is required to read the active window title. Enable it in System Settings → Privacy & Security → Screen Recording.".to_owned(),
            );
        }

        let window =
            get_active_window().map_err(|()| "macOS did not return an active window".to_owned())?;
        let process_path = window.process_path.to_string_lossy().into_owned();
        let app_id = if process_path.is_empty() {
            window.app_name
        } else {
            process_path
        };
        if app_id.trim().is_empty() {
            return Err("the active window did not expose an application identifier".to_owned());
        }

        let title = (!window.title.trim().is_empty()).then_some(window.title);
        Ok((app_id, title))
    }
}

#[async_trait]
impl DesktopAdapter for FocusedApplicationAdapter {
    fn id(&self) -> &'static str {
        "macos.focused-application"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::FocusedApplication
    }

    async fn probe(&self) -> AdapterStatus {
        match Self::current_window() {
            Ok(_) => AdapterStatus::Available,
            Err(reason) => unavailable(reason),
        }
    }

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        if shutdown_requested(&shutdown) {
            return Ok(());
        }

        let mut ticker = tokio::time::interval(WINDOW_POLL_INTERVAL);
        let mut last_error: Option<String> = None;

        loop {
            if shutdown_requested(&shutdown) {
                return Ok(());
            }
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || shutdown_requested(&shutdown) {
                        return Ok(());
                    }
                }
                _ = ticker.tick() => {
                    match Self::current_window() {
                        Ok((app_id, title)) => {
                            if last_error.take().is_some()
                                && !send_status(&output, self.id(), self.observation_type(), AdapterStatus::Available).await
                            {
                                return Ok(());
                            }

                            let next = (app_id.clone(), title.clone());
                            if self.previous.as_ref() != Some(&next) {
                                if !send_observation(
                                    &output,
                                    OffsetDateTime::now_utc(),
                                    ObservationKind::FocusedApplicationChanged { app_id, title },
                                ).await {
                                    return Ok(());
                                }
                                self.previous = Some(next);
                            }
                        }
                        Err(reason) => {
                            if last_error.as_ref() != Some(&reason) {
                                if !send_status(&output, self.id(), self.observation_type(), unavailable(reason.clone())).await {
                                    return Ok(());
                                }
                                last_error = Some(reason);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct IdleAdapter {
    idle_threshold: Duration,
    previous_idle: Option<bool>,
}

impl Default for IdleAdapter {
    fn default() -> Self {
        Self::new(DEFAULT_IDLE_THRESHOLD)
    }
}

impl IdleAdapter {
    pub fn new(idle_threshold: Duration) -> Self {
        Self {
            idle_threshold,
            previous_idle: None,
        }
    }
}

#[async_trait]
impl DesktopAdapter for IdleAdapter {
    fn id(&self) -> &'static str {
        "macos.idle-sleep"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::Idle
    }

    async fn probe(&self) -> AdapterStatus {
        if idle_seconds().is_none() {
            return unavailable("macOS could not read the time since the last user input");
        }

        match powerwatch::PowerWatch::start() {
            Ok((watcher, _events)) => {
                drop(watcher);
                AdapterStatus::Available
            }
            Err(error) => unavailable(format!(
                "macOS sleep/wake notifications could not be registered: {error}"
            )),
        }
    }

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        if shutdown_requested(&shutdown) {
            return Ok(());
        }

        let mut sleep_watch = powerwatch::PowerWatch::start().ok();
        if sleep_watch.is_none()
            && !send_status(
                &output,
                self.id(),
                self.observation_type(),
                unavailable("macOS sleep/wake notifications could not be registered"),
            )
            .await
        {
            return Ok(());
        }

        let mut ticker = tokio::time::interval(IDLE_POLL_INTERVAL);
        let mut idle_error_reported = false;

        loop {
            if shutdown_requested(&shutdown) {
                return Ok(());
            }
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || shutdown_requested(&shutdown) {
                        return Ok(());
                    }
                }
                _ = ticker.tick() => {
                    match idle_seconds() {
                        Some(seconds) => {
                            if idle_error_reported {
                                if !send_status(&output, self.id(), self.observation_type(), AdapterStatus::Available).await {
                                    return Ok(());
                                }
                                idle_error_reported = false;
                            }
                            let idle = seconds >= self.idle_threshold.as_secs_f64();
                            if self.previous_idle != Some(idle) {
                                if !send_observation(
                                    &output,
                                    OffsetDateTime::now_utc(),
                                    ObservationKind::IdleChanged { idle },
                                ).await {
                                    return Ok(());
                                }
                                self.previous_idle = Some(idle);
                            }
                        }
                        None if !idle_error_reported => {
                            if !send_status(
                                &output,
                                self.id(),
                                self.observation_type(),
                                unavailable("macOS failed to read the idle timer"),
                            ).await {
                                return Ok(());
                            }
                            idle_error_reported = true;
                        }
                        None => {}
                    }
                }
                power_event = async {
                    match sleep_watch.as_mut() {
                        Some((_watch, events)) => events.recv_async().await.map(Some),
                        None => std::future::pending().await,
                    }
                } => {
                    match power_event {
                        Ok(Some(event)) => {
                            match format!("{event:?}").as_str() {
                                "Suspend" => {
                                    if !send_observation(
                                        &output,
                                        OffsetDateTime::now_utc(),
                                        ObservationKind::SystemSleepChanged { sleeping: true },
                                    ).await {
                                        return Ok(());
                                    }
                                }
                                "Resume" => {
                                    if !send_observation(
                                        &output,
                                        OffsetDateTime::now_utc(),
                                        ObservationKind::SystemSleepChanged { sleeping: false },
                                    ).await {
                                        return Ok(());
                                    }
                                }
                                _ => {}
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            if !send_status(
                                &output,
                                self.id(),
                                self.observation_type(),
                                unavailable(format!("macOS sleep/wake event handler failed: {error}")),
                            ).await {
                                return Ok(());
                            }
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
enum InputSignal {
    KeyPressed(String),
    MouseClicked {
        button: String,
        x: Option<i32>,
        y: Option<i32>,
    },
    MouseMoved {
        x: i32,
        y: i32,
    },
    ListenerError(String),
}

struct InputEventBus {
    sender: broadcast::Sender<InputSignal>,
    listener_error: Arc<Mutex<Option<String>>>,
}

static INPUT_EVENTS: OnceLock<InputEventBus> = OnceLock::new();

fn input_events() -> (broadcast::Receiver<InputSignal>, Arc<Mutex<Option<String>>>) {
    let bus = INPUT_EVENTS.get_or_init(|| {
        let (sender, _) = broadcast::channel(INPUT_CHANNEL_CAPACITY);
        let callback_sender = sender.clone();
        let callback_sender_for_event = callback_sender.clone();
        let listener_error = Arc::new(Mutex::new(None));
        let error_for_thread = Arc::clone(&listener_error);
        let error_for_spawn_failure = Arc::clone(&listener_error);
        let last_position = Arc::new(Mutex::new(None::<(i32, i32)>));
        let callback_position = Arc::clone(&last_position);

        let listener = std::thread::Builder::new()
            .name("timetracker-macos-input-listener".to_owned())
            .spawn(move || {
                let result = listen(move |event| {
                    let signal = match event.event_type {
                        EventType::KeyPress(key) => {
                            Some(InputSignal::KeyPressed(format!("{key:?}")))
                        }
                        EventType::ButtonPress(button) => {
                            let position = *callback_position
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            Some(InputSignal::MouseClicked {
                                button: format!("{button:?}"),
                                x: position.map(|(x, _)| x),
                                y: position.map(|(_, y)| y),
                            })
                        }
                        EventType::MouseMove { x, y } => {
                            let position = (x.round() as i32, y.round() as i32);
                            *callback_position
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(position);
                            Some(InputSignal::MouseMoved {
                                x: position.0,
                                y: position.1,
                            })
                        }
                        _ => None,
                    };

                    if let Some(signal) = signal {
                        let _ = callback_sender_for_event.send(signal);
                    }
                });

                if let Err(error) = result {
                    let reason = format!("macOS input event listener failed: {error:?}");
                    *error_for_thread
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(reason.clone());
                    let _ = callback_sender.send(InputSignal::ListenerError(reason));
                }
            });

        if let Err(error) = listener {
            let reason = format!("could not start macOS input listener: {error}");
            *error_for_spawn_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(reason.clone());
            let _ = sender.send(InputSignal::ListenerError(reason));
        }

        InputEventBus {
            sender,
            listener_error,
        }
    });

    (bus.sender.subscribe(), Arc::clone(&bus.listener_error))
}

fn input_probe() -> AdapterStatus {
    if input_permission_available() {
        AdapterStatus::Available
    } else {
        unavailable(
            "Accessibility/Input Monitoring permission is required for global keyboard and mouse events. Enable access for the TimeTracker process in System Settings → Privacy & Security → Accessibility.",
        )
    }
}

macro_rules! input_adapter {
    ($name:ident, $id:literal, $observation_type:expr, $matches:pat => $kind:expr) => {
        pub struct $name;

        impl $name {
            pub fn new() -> Self {
                Self
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        #[async_trait]
        impl DesktopAdapter for $name {
            fn id(&self) -> &'static str {
                $id
            }

            fn observation_type(&self) -> ObservationType {
                $observation_type
            }

            async fn probe(&self) -> AdapterStatus {
                input_probe()
            }

            async fn run(
                &mut self,
                output: mpsc::Sender<AdapterOutput>,
                mut shutdown: watch::Receiver<bool>,
            ) -> Result<(), ClientError> {
                if shutdown_requested(&shutdown) {
                    return Ok(());
                }

                if let AdapterStatus::Unavailable { reason } = input_probe() {
                    let _ = send_status(
                        &output,
                        self.id(),
                        self.observation_type(),
                        unavailable(reason),
                    ).await;
                    return Ok(());
                }

                let (mut events, listener_error) = input_events();
                loop {
                    if shutdown_requested(&shutdown) {
                        return Ok(());
                    }
                    let listener_failure = {
                        listener_error
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .clone()
                    };
                    if let Some(reason) = listener_failure {
                        let _ = send_status(
                            &output,
                            self.id(),
                            self.observation_type(),
                            unavailable(reason),
                        ).await;
                        return Ok(());
                    }
                    tokio::select! {
                        changed = shutdown.changed() => {
                            if changed.is_err() || shutdown_requested(&shutdown) {
                                return Ok(());
                            }
                        }
                        event = events.recv() => {
                            match event {
                                Ok(signal) => match signal {
                                    InputSignal::ListenerError(reason) => {
                                        let _ = send_status(
                                            &output,
                                            self.id(),
                                            self.observation_type(),
                                            unavailable(reason),
                                        ).await;
                                        return Ok(());
                                    }
                                    $matches => {
                                        if !send_observation(
                                            &output,
                                            OffsetDateTime::now_utc(),
                                            $kind,
                                        ).await {
                                            return Ok(());
                                        }
                                    }
                                    _ => {}
                                },
                                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                    if !send_status(
                                        &output,
                                        self.id(),
                                        self.observation_type(),
                                        unavailable(format!("input event consumer fell behind; {skipped} event(s) were skipped")),
                                    ).await {
                                        return Ok(());
                                    }
                                }
                                Err(broadcast::error::RecvError::Closed) => {
                                    let _ = send_status(
                                        &output,
                                        self.id(),
                                        self.observation_type(),
                                        unavailable("macOS input event stream closed"),
                                    ).await;
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
        }
    };
}

input_adapter!(
    KeyboardAdapter,
    "macos.keyboard",
    ObservationType::Keyboard,
    InputSignal::KeyPressed(key) => ObservationKind::KeyboardButtonPressed { key }
);

input_adapter!(
    MouseClickAdapter,
    "macos.mouse-click",
    ObservationType::MouseClick,
    InputSignal::MouseClicked { button, x, y } => ObservationKind::MouseClicked { button, x, y }
);

input_adapter!(
    MouseMoveAdapter,
    "macos.mouse-move",
    ObservationType::MouseMove,
    InputSignal::MouseMoved { x, y } => ObservationKind::MouseMoved { x, y }
);

pub fn default_macos_adapters() -> Vec<Box<dyn DesktopAdapter>> {
    vec![
        Box::new(FocusedApplicationAdapter::new()),
        Box::new(IdleAdapter::default()),
        Box::new(KeyboardAdapter::new()),
        Box::new(MouseClickAdapter::new()),
        Box::new(MouseMoveAdapter::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapters() -> Vec<Box<dyn DesktopAdapter>> {
        default_macos_adapters()
    }

    #[test]
    fn adapter_ids_are_unique() {
        let adapters = adapters();
        let ids: Vec<_> = adapters.iter().map(|adapter| adapter.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len());
    }

    #[test]
    fn adapters_cover_every_supported_observation_type() {
        let actual: Vec<_> = adapters()
            .iter()
            .map(|adapter| adapter.observation_type())
            .collect();
        assert_eq!(
            actual,
            vec![
                ObservationType::FocusedApplication,
                ObservationType::Idle,
                ObservationType::Keyboard,
                ObservationType::MouseClick,
                ObservationType::MouseMove,
            ]
        );
    }

    #[tokio::test]
    async fn every_adapter_can_be_probed_on_the_current_mac_session() {
        for adapter in adapters() {
            let status = adapter.probe().await;
            eprintln!(
                "{} ({:?}): {:?}",
                adapter.id(),
                adapter.observation_type(),
                status
            );
        }
    }

    #[tokio::test]
    async fn run_exits_when_shutdown_is_already_signaled() {
        for mut adapter in adapters() {
            let (output_tx, _output_rx) = mpsc::channel(4);
            let (_shutdown_tx, shutdown_rx) = watch::channel(true);
            adapter.run(output_tx, shutdown_rx).await.unwrap();
        }
    }

    #[test]
    fn mouse_coordinates_round_to_integer_pixels() {
        assert_eq!((3.4_f64.round() as i32, (-2.6_f64).round() as i32), (3, -3));
    }
}
