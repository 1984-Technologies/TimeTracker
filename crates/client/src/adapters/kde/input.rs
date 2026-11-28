use std::{
    env,
    fs::OpenOptions,
    os::{fd::OwnedFd, unix::fs::OpenOptionsExt},
    path::Path,
    sync::{Mutex, OnceLock},
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
use uuid::Uuid;
use zbus::{Connection, interface};

use super::{
    kwin::{KWinScriptSession, probe_scripting},
    observation, readable, run_native, running, send, send_native, status, stopped, unavailable,
};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

const CURSOR_DBUS_INTERFACE: &str = "io.timetracker.KWinCursor";

static LAST_CURSOR_POS: OnceLock<Mutex<Option<(i32, i32)>>> = OnceLock::new();

fn cursor_cache() -> &'static Mutex<Option<(i32, i32)>> {
    LAST_CURSOR_POS.get_or_init(|| Mutex::new(None))
}

fn update_last_cursor(position: (i32, i32)) {
    *cursor_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(position);
}

fn last_cursor() -> Option<(i32, i32)> {
    *cursor_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

fn connect_libinput(kind: ObservationType) -> Result<(Libinput, usize), ClientError> {
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
        return Err(format!(
            "No readable {kind:?} devices on {seat}; raw input requires read access to /dev/input/event* (add user to the 'input' group). KDE Wayland does not expose global key or button events."
        )
            .into());
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
            let position = last_cursor();
            Some(ObservationKind::MouseClicked {
                button: event.button().to_string(),
                x: position.map(|(x, _)| x),
                y: position.map(|(_, y)| y),
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
        )
    )
}

macro_rules! input_adapter {
    ($name:ident, $id:literal, $kind:ident) => {
        #[derive(Default)]
        pub struct $name;

        #[async_trait]
        impl DesktopAdapter for $name {
            fn id(&self) -> &'static str {
                $id
            }

            fn observation_type(&self) -> ObservationType {
                ObservationType::$kind
            }

            async fn probe(&self) -> AdapterStatus {
                match tokio::task::spawn_blocking(|| {
                    connect_libinput(ObservationType::$kind).map(|_| ())
                })
                .await
                {
                    Ok(Ok(())) => AdapterStatus::Available,
                    Ok(Err(error)) => unavailable(error),
                    Err(error) => unavailable(error),
                }
            }

            async fn run(
                &mut self,
                output: mpsc::Sender<AdapterOutput>,
                shutdown: watch::Receiver<bool>,
            ) -> Result<(), ClientError> {
                run_native(self, output, shutdown, |output, mut shutdown| {
                    let kind = ObservationType::$kind;
                    let (mut context, mut devices) = connect_libinput(kind)?;
                    if !send_native(
                        &output,
                        &mut shutdown,
                        status(&$name, AdapterStatus::Available),
                    ) {
                        return Ok(());
                    }
                    while running(&output, &shutdown) {
                        if !readable(&context)? {
                            continue;
                        }
                        context.dispatch()?;
                        for event in context.by_ref() {
                            if event.device().has_capability(capability(kind)) {
                                match &event {
                                    Event::Device(DeviceEvent::Added(_)) => devices += 1,
                                    Event::Device(DeviceEvent::Removed(_)) => {
                                        devices = devices.saturating_sub(1)
                                    }
                                    _ => {}
                                }
                            }
                            if let Some(activity) = activity(event)
                                && matches_kind(&activity, kind)
                                && !send_native(&output, &mut shutdown, observation(activity))
                            {
                                return Ok(());
                            }
                        }
                        if devices == 0 {
                            return Err("all matching input devices disconnected; restart the adapter after reconnecting".into());
                        }
                    }
                    Ok(())
                })
                .await
            }
        }
    };
}

input_adapter!(KeyboardAdapter, "kde.keyboard", Keyboard);
input_adapter!(MouseClickAdapter, "kde.mouse-click", MouseClick);

struct CursorReceiver {
    tx: mpsc::UnboundedSender<(i32, i32)>,
}

#[interface(name = "io.timetracker.KWinCursor")]
impl CursorReceiver {
    #[zbus(name = "CursorMoved")]
    async fn cursor_moved(&self, x: String, y: String) {
        if let Some(position) = parse_cursor_coords(&x, &y) {
            let _ = self.tx.send(position);
        }
    }
}

fn parse_cursor_coords(x: &str, y: &str) -> Option<(i32, i32)> {
    let x = x.trim().parse::<f64>().ok()?.round() as i32;
    let y = y.trim().parse::<f64>().ok()?.round() as i32;
    Some((x, y))
}

fn build_cursor_script(service: &str, path: &str) -> String {
    format!(
        r#"
const SERVICE = {service:?};
const PATH = {path:?};
const INTERFACE = {CURSOR_DBUS_INTERFACE:?};

let lastX = null;
let lastY = null;
let lastSent = 0;

function emitCursor(force) {{
    const pos = workspace.cursorPos;
    if (!pos) return;
    const x = Math.round(pos.x);
    const y = Math.round(pos.y);
    const now = Date.now();
    if (!force && x === lastX && y === lastY) return;
    if (!force && (now - lastSent) < 16) return;
    lastX = x;
    lastY = y;
    lastSent = now;
    callDBus(SERVICE, PATH, INTERFACE, "CursorMoved", String(x), String(y));
}}

workspace.cursorPosChanged.connect(function() {{ emitCursor(false); }});
emitCursor(true);
"#
    )
}

#[derive(Default)]
pub struct MouseMoveAdapter;

impl MouseMoveAdapter {
    async fn collect(
        &self,
        output: &mpsc::Sender<AdapterOutput>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let connection = Connection::session().await?;
        let unique_name = connection
            .unique_name()
            .ok_or("session D-Bus connection has no unique name")?
            .as_str()
            .to_owned();
        let object_path = format!("/io/timetracker/KWinCursor/{}", Uuid::new_v4().simple());

        let (tx, mut rx) = mpsc::unbounded_channel();
        connection
            .object_server()
            .at(object_path.as_str(), CursorReceiver { tx })
            .await?;

        let script = build_cursor_script(&unique_name, &object_path);
        let session =
            match KWinScriptSession::start(connection.clone(), "timetracker_cursor", &script).await
            {
                Ok(session) => session,
                Err(error) => {
                    let _ = connection
                        .object_server()
                        .remove::<CursorReceiver, _>(object_path.as_str())
                        .await;
                    return Err(error);
                }
            };

        let result = self.forward_events(&mut rx, output, shutdown).await;

        session.stop().await;
        let _ = connection
            .object_server()
            .remove::<CursorReceiver, _>(object_path.as_str())
            .await;

        result
    }

    async fn forward_events(
        &self,
        rx: &mut mpsc::UnboundedReceiver<(i32, i32)>,
        output: &mpsc::Sender<AdapterOutput>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        if !send(output, shutdown, status(self, AdapterStatus::Available)).await {
            return Ok(());
        }

        let mut previous = None;
        loop {
            let position = tokio::select! {
                biased;
                _ = stopped(shutdown) => return Ok(()),
                _ = output.closed() => return Ok(()),
                pos = rx.recv() => pos,
            };
            let Some(position) = position else {
                return Ok(());
            };
            if previous != Some(position) {
                update_last_cursor(position);
                if !send(
                    output,
                    shutdown,
                    observation(ObservationKind::MouseMoved {
                        x: position.0,
                        y: position.1,
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
        "kde.mouse-move"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::MouseMove
    }

    async fn probe(&self) -> AdapterStatus {
        match probe_scripting().await {
            Ok(()) => AdapterStatus::Available,
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
        if let Err(error) = self.collect(&output, &mut shutdown).await {
            send(&output, &mut shutdown, status(self, unavailable(error))).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cursor_coords;

    #[test]
    fn parses_and_rounds_cursor_coordinates() {
        assert_eq!(parse_cursor_coords("463", "714"), Some((463, 714)));
        assert_eq!(parse_cursor_coords("-100.4", "399.6"), Some((-100, 400)));
        assert_eq!(parse_cursor_coords("bad", "10"), None);
    }
}
