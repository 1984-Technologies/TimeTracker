use std::{
    io,
    os::fd::{AsFd, OwnedFd},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::sync::{mpsc, watch};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop,
    protocol::{wl_callback, wl_registry, wl_seat},
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};

use super::{observation, readable, run_native, running, send_native, status, unavailable};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

pub struct IdleAdapter {
    idle_threshold: Duration,
}

impl Default for IdleAdapter {
    fn default() -> Self {
        Self::new(Duration::from_secs(60))
    }
}

impl IdleAdapter {
    pub fn new(idle_threshold: Duration) -> Self {
        Self { idle_threshold }
    }
}

#[async_trait]
impl DesktopAdapter for IdleAdapter {
    fn id(&self) -> &'static str {
        "kde.idle"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::Idle
    }

    async fn probe(&self) -> AdapterStatus {
        let threshold = self.idle_threshold;
        match tokio::task::spawn_blocking(move || IdleSession::connect(threshold).map(|_| ())).await
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
        let threshold = self.idle_threshold;
        run_native(self, output, shutdown, move |output, mut shutdown| {
            let mut session = IdleSession::connect(threshold)?;
            if !send_native(
                &output,
                &mut shutdown,
                status(&IdleAdapter::new(threshold), AdapterStatus::Available),
            ) {
                return Ok(());
            }
            while running(&output, &shutdown) {
                session.dispatch()?;
                for idle in session.state.changes.drain(..) {
                    if !send_native(
                        &output,
                        &mut shutdown,
                        observation(ObservationKind::IdleChanged { idle }),
                    ) {
                        return Ok(());
                    }
                }
            }
            Ok(())
        })
        .await
    }
}

#[derive(Default)]
struct IdleState {
    seat: Option<(u32, wl_seat::WlSeat)>,
    notifier: Option<(u32, ExtIdleNotifierV1)>,
    ready: bool,
    removed: bool,
    changes: Vec<bool>,
}

struct IdleSession {
    queue: EventQueue<IdleState>,
    fd: OwnedFd,
    state: IdleState,
    _notification: ExtIdleNotificationV1,
}

impl IdleSession {
    fn connect(threshold: Duration) -> Result<Self, ClientError> {
        let milliseconds = u32::try_from(threshold.as_millis())
            .map_err(|_| "idle threshold exceeds the Wayland u32 millisecond limit")?;
        if milliseconds == 0 {
            return Err("idle threshold must be at least one millisecond".into());
        }
        let connection = Connection::connect_to_env()?;
        let fd = connection.as_fd().try_clone_to_owned()?;
        let mut queue = connection.new_event_queue();
        let handle = queue.handle();
        let _registry = connection.display().get_registry(&handle, ());
        let _sync = connection.display().sync(&handle, ());
        let mut state = IdleState::default();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !state.ready {
            dispatch(&mut queue, &fd, &mut state)?;
            if Instant::now() >= deadline {
                return Err("Wayland registry request timed out".into());
            }
        }
        let seat = &state.seat.as_ref().ok_or("Wayland session has no seat")?.1;
        let notifier = &state
            .notifier
            .as_ref()
            .ok_or("Wayland compositor must support ext_idle_notifier_v1 version 2")?
            .1;
        let notification = notifier.get_input_idle_notification(milliseconds, seat, &handle, ());
        queue.flush()?;
        Ok(Self {
            queue,
            fd,
            state,
            _notification: notification,
        })
    }

    fn dispatch(&mut self) -> Result<(), ClientError> {
        dispatch(&mut self.queue, &self.fd, &mut self.state)?;
        if self.state.removed {
            return Err("Wayland idle notifier or seat was removed".into());
        }
        Ok(())
    }
}

fn dispatch(
    queue: &mut EventQueue<IdleState>,
    fd: &OwnedFd,
    state: &mut IdleState,
) -> Result<(), ClientError> {
    queue.dispatch_pending(state)?;
    queue.flush()?;
    if let Some(guard) = queue.prepare_read()
        && readable(fd)?
    {
        match guard.read() {
            Ok(_) => {}
            Err(wayland_client::backend::WaylandError::Io(error))
                if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error.into()),
        }
    }
    queue.dispatch_pending(state)?;
    Ok(())
}

impl Dispatch<wl_registry::WlRegistry, ()> for IdleState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some((name, registry.bind(name, 1, handle, ())))
                }
                "ext_idle_notifier_v1" if version >= 2 => {
                    state.notifier = Some((name, registry.bind(name, 2, handle, ())))
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.removed |= state.seat.as_ref().is_some_and(|(id, _)| *id == name)
                    || state.notifier.as_ref().is_some_and(|(id, _)| *id == name);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for IdleState {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.ready = true;
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for IdleState {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => state.changes.push(true),
            ext_idle_notification_v1::Event::Resumed => state.changes.push(false),
            _ => {}
        }
    }
}

delegate_noop!(IdleState: ignore wl_seat::WlSeat);
delegate_noop!(IdleState: ignore ExtIdleNotifierV1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_thresholds_outside_protocol_range_before_connecting() {
        assert!(IdleSession::connect(Duration::ZERO).is_err());
        assert!(IdleSession::connect(Duration::from_millis(u64::from(u32::MAX) + 1)).is_err());
    }
}
