use std::{
    env,
    fs::OpenOptions,
    os::{fd::OwnedFd, unix::fs::OpenOptionsExt},
    path::Path,
};

use async_trait::async_trait;
use input::{
    DeviceCapability, Event, Libinput, LibinputInterface,
    event::{
        EventTrait,
        device::DeviceEvent,
        keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait},
        pointer::{ButtonState, PointerEvent},
    },
};
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::sync::{mpsc, watch};

use super::{observation, readable, run_native, running, send_native, status, unavailable};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        OpenOptions::new()
            .read(flags & libc::O_ACCMODE != libc::O_WRONLY)
            .write(flags & libc::O_ACCMODE != libc::O_RDONLY)
            .custom_flags(flags)
            .open(path)
            .map(Into::into)
            .map_err(|error| error.raw_os_error().unwrap_or(libc::EIO))
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        drop(fd);
    }
}

fn connect(kind: ObservationType) -> Result<(Libinput, usize), ClientError> {
    let mut context = Libinput::new_with_udev(Interface);
    let seat = env::var("XDG_SEAT").unwrap_or_else(|_| "seat0".into());
    context
        .udev_assign_seat(&seat)
        .map_err(|_| "libinput could not monitor the session seat")?;
    context.dispatch()?;
    let devices = context
        .by_ref()
        .filter(|event| {
            matches!(event, Event::Device(DeviceEvent::Added(_)))
                && event.device().has_capability(capability(kind))
        })
        .count();
    if devices == 0 {
        return Err(format!("No readable {kind:?} devices on {seat}; raw input requires read access to /dev/input/event*. Hyprland IPC does not expose global key or button events.").into());
    }
    Ok((context, devices))
}

fn capability(kind: ObservationType) -> DeviceCapability {
    match kind {
        ObservationType::Keyboard => DeviceCapability::Keyboard,
        _ => DeviceCapability::Pointer,
    }
}

fn activity(event: Event, position: &mut (i32, i32)) -> Option<ObservationKind> {
    match event {
        Event::Keyboard(KeyboardEvent::Key(event)) if event.key_state() == KeyState::Pressed => {
            Some(ObservationKind::KeyboardButtonPressed {
                key: event.key().to_string(),
            })
        }
        Event::Pointer(PointerEvent::Button(event))
            if event.button_state() == ButtonState::Pressed =>
        {
            Some(ObservationKind::MouseClicked {
                button: event.button().to_string(),
                x: Some(position.0),
                y: Some(position.1),
            })
        }
        Event::Pointer(PointerEvent::Motion(event)) => {
            position.0 = position.0.saturating_add(event.dx().round() as i32);
            position.1 = position.1.saturating_add(event.dy().round() as i32);
            Some(ObservationKind::MouseMoved {
                x: position.0,
                y: position.1,
            })
        }
        Event::Pointer(PointerEvent::MotionAbsolute(event)) => {
            position.0 = event.absolute_x().round() as i32;
            position.1 = event.absolute_y().round() as i32;
            Some(ObservationKind::MouseMoved {
                x: position.0,
                y: position.1,
            })
        }
        _ => None,
    }
}

fn matches_kind(activity: &ObservationKind, kind: ObservationType) -> bool {
    matches!(
        (activity, kind),
        (
            ObservationKind::KeyboardButtonPressed { .. },
            ObservationType::Keyboard
        ) | (
            ObservationKind::MouseClicked { .. },
            ObservationType::MouseClick
        ) | (
            ObservationKind::MouseMoved { .. },
            ObservationType::MouseMove
        )
    )
}

macro_rules! input_adapter {
    ($name:ident, $id:literal, $kind:ident) => {
        #[derive(Default)]
        pub struct $name;

        #[async_trait]
        impl DesktopAdapter for $name {
            fn id(&self) -> &'static str { $id }
            fn observation_type(&self) -> ObservationType { ObservationType::$kind }

            async fn probe(&self) -> AdapterStatus {
                match tokio::task::spawn_blocking(|| connect(ObservationType::$kind).map(|_| ())).await {
                    Ok(Ok(())) => AdapterStatus::Available,
                    Ok(Err(error)) => unavailable(error),
                    Err(error) => unavailable(error),
                }
            }

            async fn run(&mut self, output: mpsc::Sender<AdapterOutput>, shutdown: watch::Receiver<bool>) -> Result<(), ClientError> {
                run_native(self, output, shutdown, |output, mut shutdown| {
                    let kind = ObservationType::$kind;
                    let (mut context, mut devices) = connect(kind)?;
                    if !send_native(&output, &mut shutdown, status(&$name, AdapterStatus::Available)) {
                        return Ok(());
                    }
                    let mut position = (0, 0);
                    while running(&output, &shutdown) {
                        if !readable(&context)? { continue; }
                        context.dispatch()?;
                        for event in context.by_ref() {
                            if event.device().has_capability(capability(kind)) {
                                match &event {
                                    Event::Device(DeviceEvent::Added(_)) => devices += 1,
                                    Event::Device(DeviceEvent::Removed(_)) => devices = devices.saturating_sub(1),
                                    _ => {},
                                }
                            }
                            if let Some(activity) = activity(event, &mut position)
                                && matches_kind(&activity, kind)
                                && !send_native(&output, &mut shutdown, observation(activity)) {
                                return Ok(());
                            }
                        }
                        if devices == 0 {
                            return Err("all matching input devices disconnected; restart the adapter after reconnecting".into());
                        }
                    }
                    Ok(())
                }).await
            }
        }
    };
}

input_adapter!(KeyboardAdapter, "hyprland.keyboard", Keyboard);
input_adapter!(MouseClickAdapter, "hyprland.mouse-click", MouseClick);
input_adapter!(MouseMoveAdapter, "hyprland.mouse-move", MouseMove);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_input_adapter_accepts_only_its_own_activity() {
        for (activity, expected) in [
            (
                ObservationKind::KeyboardButtonPressed { key: "30".into() },
                ObservationType::Keyboard,
            ),
            (
                ObservationKind::MouseClicked {
                    button: "272".into(),
                    x: Some(10),
                    y: Some(20),
                },
                ObservationType::MouseClick,
            ),
            (
                ObservationKind::MouseMoved { x: 10, y: 20 },
                ObservationType::MouseMove,
            ),
        ] {
            for kind in [
                ObservationType::Keyboard,
                ObservationType::MouseClick,
                ObservationType::MouseMove,
            ] {
                assert_eq!(matches_kind(&activity, kind), kind == expected);
            }
        }
    }
}
