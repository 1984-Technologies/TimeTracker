use std::{
    io,
    sync::mpsc as std_mpsc,
    thread::{self, JoinHandle},
    time::Duration,
};

use nsworkspace::{Event, Monitor, NotificationListener};
use tokio::sync::mpsc;

use super::build_default_adapters;
use crate::{ClientError, adapters::DesktopAdapter};

const WORKSPACE_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct MacOsAdapterRuntime {
    monitor: Monitor,
    monitor_stop_tx: std_mpsc::Sender<()>,
    focused_application_events: Option<mpsc::UnboundedReceiver<String>>,
    initial_app_id: Option<String>,
    forwarder_stop_tx: std_mpsc::Sender<()>,
    event_forwarder: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct MacOsRuntimeShutdownHandle {
    stop_tx: std_mpsc::Sender<()>,
}

impl MacOsRuntimeShutdownHandle {
    pub fn request_shutdown(&self) -> bool {
        self.stop_tx.send(()).is_ok()
    }
}

impl MacOsAdapterRuntime {
    pub fn new() -> Result<Self, ClientError> {
        let Some((monitor, workspace_events, monitor_stop_tx)) = Monitor::new() else {
            return Err(io::Error::other(
                "NSWorkspace monitor must be initialized on the macOS main thread",
            )
            .into());
        };

        monitor.subscribe(NotificationListener::DidActivateApplication);
        let initial_app_id = monitor.get_active_application();

        let (focused_application_tx, focused_application_events) =
            mpsc::unbounded_channel::<String>();
        let (forwarder_stop_tx, forwarder_stop_rx) = std_mpsc::channel::<()>();

        let event_forwarder = thread::Builder::new()
            .name("timetracker-nsworkspace-events".to_owned())
            .spawn(move || {
                loop {
                    match forwarder_stop_rx.try_recv() {
                        Ok(()) | Err(std_mpsc::TryRecvError::Disconnected) => break,
                        Err(std_mpsc::TryRecvError::Empty) => {}
                    }

                    match workspace_events.recv_timeout(WORKSPACE_EVENT_POLL_INTERVAL) {
                        Ok(Event::DidActivateApplication(app_id)) => {
                            if focused_application_tx.send(app_id).is_err() {
                                break;
                            }
                        }
                        Ok(Event::Error(error)) => {
                            eprintln!("[NSWorkspace ERROR] {error:?}");
                        }
                        Ok(_) => {}
                        Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?;

        Ok(Self {
            monitor,
            monitor_stop_tx,
            focused_application_events: Some(focused_application_events),
            initial_app_id,
            forwarder_stop_tx,
            event_forwarder: Some(event_forwarder),
        })
    }

    pub fn default_adapters(&mut self) -> Result<Vec<Box<dyn DesktopAdapter>>, ClientError> {
        let focused_application_events = self
            .focused_application_events
            .take()
            .ok_or_else(|| io::Error::other("default adapters were already created"))?;

        Ok(build_default_adapters(
            focused_application_events,
            self.initial_app_id.take(),
        ))
    }

    pub fn shutdown_handle(&self) -> MacOsRuntimeShutdownHandle {
        MacOsRuntimeShutdownHandle {
            stop_tx: self.monitor_stop_tx.clone(),
        }
    }

    pub fn run(&self) {
        self.monitor.run();
    }
}

impl Drop for MacOsAdapterRuntime {
    fn drop(&mut self) {
        let _ = self.forwarder_stop_tx.send(());
        if let Some(event_forwarder) = self.event_forwarder.take() {
            if event_forwarder.join().is_err() {
                eprintln!("[NSWorkspace ERROR] event-forwarder thread panicked");
            }
        }
    }
}
