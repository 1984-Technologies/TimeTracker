//! Windows desktop adapters. Run in the interactive user's session, not a service.
mod input;
mod polling;

#[cfg(test)]
mod session_test;

use std::io;

use timetracker_model::{AdapterStatus, Observation, ObservationKind};
use tokio::sync::{mpsc, watch};
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_READOBJECTS, OpenInputDesktop,
};

use super::{AdapterOutput, DesktopAdapter};
pub use input::{KeyboardAdapter, MouseClickAdapter, MouseMoveAdapter};
pub use polling::{FocusedApplicationAdapter, IdleAdapter};

pub fn default_adapters() -> Vec<Box<dyn DesktopAdapter>> {
    vec![
        Box::new(FocusedApplicationAdapter),
        Box::new(IdleAdapter::default()),
        Box::new(KeyboardAdapter),
        Box::new(MouseClickAdapter),
        Box::new(MouseMoveAdapter),
    ]
}

fn unavailable(error: impl std::fmt::Display) -> AdapterStatus {
    AdapterStatus::Unavailable {
        reason: error.to_string(),
    }
}

fn desktop_available() -> io::Result<()> {
    // SAFETY: no borrowed pointers; the returned desktop handle is closed here.
    unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if desktop.is_null() {
            return Err(io::Error::last_os_error());
        }
        CloseDesktop(desktop);
    }
    Ok(())
}

fn observation(kind: ObservationKind) -> AdapterOutput {
    AdapterOutput::Observation(Observation {
        occurred_at: time::OffsetDateTime::now_utc(),
        kind,
    })
}

// Both channel backpressure and receiver closure must allow shutdown to finish.
async fn send(
    output: &mpsc::Sender<AdapterOutput>,
    shutdown: &mut watch::Receiver<bool>,
    event: AdapterOutput,
) -> bool {
    tokio::select! {
        biased;
        _ = stopped(shutdown) => false,
        result = output.send(event) => result.is_ok(),
    }
}

async fn stopped(shutdown: &mut watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow_and_update() || shutdown.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use timetracker_model::ObservationType;

    #[test]
    fn defaults_cover_all_types_with_unique_ids() {
        let adapters = default_adapters();
        assert_eq!(
            adapters
                .iter()
                .map(|a| a.observation_type())
                .collect::<Vec<_>>(),
            vec![
                ObservationType::FocusedApplication,
                ObservationType::Idle,
                ObservationType::Keyboard,
                ObservationType::MouseClick,
                ObservationType::MouseMove,
            ]
        );
        let mut ids = adapters.iter().map(|a| a.id()).collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), adapters.len());
    }

    #[tokio::test]
    async fn every_adapter_honors_initial_shutdown_and_closed_output() {
        for mut adapter in default_adapters() {
            let (output, _events) = mpsc::channel(1);
            let (_shutdown, stop) = watch::channel(true);
            tokio::time::timeout(Duration::from_secs(2), adapter.run(output, stop))
                .await
                .unwrap()
                .unwrap();

            let (output, events) = mpsc::channel(1);
            drop(events);
            let (_shutdown, stop) = watch::channel(false);
            tokio::time::timeout(Duration::from_secs(2), adapter.run(output, stop))
                .await
                .unwrap()
                .unwrap();
        }
    }

    #[tokio::test]
    async fn shutdown_interrupts_backpressure_and_sender_disconnection() {
        for disconnect in [false, true] {
            let (output, _events) = mpsc::channel(1);
            output
                .send(observation(ObservationKind::MouseMoved {}))
                .await
                .unwrap();
            let (shutdown, mut stop) = watch::channel(false);
            let task = tokio::spawn(async move {
                send(
                    &output,
                    &mut stop,
                    observation(ObservationKind::MouseMoved {}),
                )
                .await
            });
            tokio::task::yield_now().await;
            if !disconnect {
                shutdown.send(true).unwrap();
            }
            drop(shutdown);
            assert!(
                !tokio::time::timeout(Duration::from_secs(2), task)
                    .await
                    .unwrap()
                    .unwrap()
            );
        }
    }
}
