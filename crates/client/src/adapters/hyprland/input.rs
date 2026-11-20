use std::{
    env,
    fs::OpenOptions,
    os::{fd::OwnedFd, unix::fs::OpenOptionsExt},
    path::Path,
    time::Duration,
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

use super::{
    focused_application::{request, socket_directory},
    observation, readable, run_native, running, send, send_native, status, stopped, unavailable,
};
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
    if kind == ObservationType::MouseClick {
        tokio::runtime::Handle::current().block_on(cursor_position(&socket_directory()?))?;
    }
    Ok((context, devices))
}

fn capability(kind: ObservationType) -> DeviceCapability {
    match kind {
        ObservationType::Keyboard => DeviceCapability::Keyboard,
        _ => DeviceCapability::Pointer,
    }
}

fn activity(event: Event) -> Option<ObservationKind> {
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
                x: None,
                y: None,
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
                            if let Some(mut activity) = activity(event)
                                && matches_kind(&activity, kind) {
                                if let ObservationKind::MouseClicked { x, y, .. } = &mut activity {
                                    let position = tokio::runtime::Handle::current().block_on(cursor_position(&socket_directory()?))?;
                                    *x = Some(position.x);
                                    *y = Some(position.y);
                                }
                                if !send_native(&output, &mut shutdown, observation(activity)) {
                                    return Ok(());
                                }
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

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
struct CursorPosition {
    x: i32,
    y: i32,
}

async fn cursor_position(directory: &Path) -> Result<CursorPosition, ClientError> {
    Ok(serde_json::from_slice(
        &request(directory, b"j/cursorpos").await?,
    )?)
}

#[derive(Default)]
pub struct MouseMoveAdapter;

impl MouseMoveAdapter {
    async fn collect(
        &self,
        directory: &Path,
        output: &mpsc::Sender<AdapterOutput>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let mut previous = None;
        // Device deltas cannot account for cursor clipping, scaling, or compositor warps.
        let mut ticker = tokio::time::interval(Duration::from_secs_f64(1.0 / 60.0));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let position = tokio::select! {
                biased;
                _ = stopped(shutdown) => return Ok(()),
                _ = output.closed() => return Ok(()),
                result = async {
                    ticker.tick().await;
                    cursor_position(directory).await
                } => result?,
            };
            if previous.is_none()
                && !send(output, shutdown, status(self, AdapterStatus::Available)).await
            {
                return Ok(());
            }
            if previous != Some(position) {
                if !send(
                    output,
                    shutdown,
                    observation(ObservationKind::MouseMoved {
                        x: position.x,
                        y: position.y,
                    }),
                )
                .await
                {
                    return Ok(());
                }
                previous = Some(position);
            }
        }
    }
}

#[async_trait]
impl DesktopAdapter for MouseMoveAdapter {
    fn id(&self) -> &'static str {
        "hyprland.mouse-move"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::MouseMove
    }

    async fn probe(&self) -> AdapterStatus {
        let result = async { cursor_position(&socket_directory()?).await }.await;
        match result {
            Ok(_) => AdapterStatus::Available,
            Err(error) => unavailable(error),
        }
    }

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        if !running(&output, &shutdown) {
            return Ok(());
        }
        let result = async {
            self.collect(&socket_directory()?, &output, &mut shutdown)
                .await
        }
        .await;
        if let Err(error) = result {
            send(&output, &mut shutdown, status(self, unavailable(error))).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cursor_starts_at_compositor_position_and_stops_changing_at_screen_edge() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::UnixListener,
        };

        let directory = env::temp_dir().join(format!("tt-cursor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let listener = UnixListener::bind(directory.join(".socket.sock")).unwrap();
        let server = tokio::spawn(async move {
            for (x, y) in [
                (463, 714),
                (0, 714),
                (0, 714),
                (0, 714),
                (900, 500),
                (900, 500),
                (-100, 400), // A monitor may legitimately be left of the desktop origin.
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut command = [0; 11];
                socket.read_exact(&mut command).await.unwrap();
                assert_eq!(&command, b"j/cursorpos");
                socket
                    .write_all(format!(r#"{{"x":{x},"y":{y}}}"#).as_bytes())
                    .await
                    .unwrap();
            }
            std::future::pending::<()>().await;
        });
        let (output, mut events) = mpsc::channel(16);
        let (stop, mut shutdown) = watch::channel(false);
        let collector_directory = directory.clone();
        let collector = tokio::spawn(async move {
            MouseMoveAdapter
                .collect(&collector_directory, &output, &mut shutdown)
                .await
        });

        assert!(matches!(
            events.recv().await,
            Some(AdapterOutput::Status {
                status: AdapterStatus::Available,
                ..
            })
        ));
        for (x, y) in [(463, 714), (0, 714), (900, 500), (-100, 400)] {
            let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .unwrap()
                .unwrap();
            let AdapterOutput::Observation(event) = event else {
                panic!("expected cursor observation")
            };
            assert_eq!(event.kind, ObservationKind::MouseMoved { x, y });
        }
        stop.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), collector)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(events.recv().await.is_none());
        server.abort();
        let _ = server.await;
        std::fs::remove_dir_all(directory).unwrap();
    }
}
