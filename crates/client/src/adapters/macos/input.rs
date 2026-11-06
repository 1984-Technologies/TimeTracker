use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use rdev::{EventType, listen};
use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::sync::{broadcast, mpsc, watch};

use super::{send_observation, send_status, shutdown_requested, unavailable};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

const INPUT_CHANNEL_CAPACITY: usize = 4096;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
}

fn input_permission_available() -> bool {
    unsafe { CGPreflightListenEventAccess() }
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

fn rounded_mouse_position(x: f64, y: f64) -> (i32, i32) {
    (x.round() as i32, y.round() as i32)
}

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
                            let position = rounded_mouse_position(x, y);
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

#[cfg(test)]
mod tests {
    use super::rounded_mouse_position;

    #[test]
    fn mouse_coordinates_are_rounded_to_nearest_integer_pixel() {
        assert_eq!(rounded_mouse_position(3.4, -2.6), (3, -3));
        assert_eq!(rounded_mouse_position(3.5, -2.5), (4, -3));
        assert_eq!(rounded_mouse_position(0.0, 0.0), (0, 0));
    }
}
