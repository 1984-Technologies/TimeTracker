mod focused_application;
mod idle;
mod input;
mod kwin;

use std::{io, os::fd::AsRawFd};

use timetracker_model::{AdapterStatus, Observation, ObservationKind};
use tokio::sync::{mpsc, watch};

use super::{AdapterOutput, DesktopAdapter};
use crate::ClientError;

pub use focused_application::FocusedApplicationAdapter;
pub use idle::IdleAdapter;
pub use input::{KeyboardAdapter, MouseClickAdapter, MouseMoveAdapter};

pub fn default_kde_adapters() -> Vec<Box<dyn DesktopAdapter>> {
    vec![
        Box::new(FocusedApplicationAdapter::new(false)),
        Box::new(IdleAdapter::default()),
        Box::new(KeyboardAdapter),
        Box::new(MouseClickAdapter),
        Box::new(MouseMoveAdapter),
    ]
}

fn unavailable(reason: impl ToString) -> AdapterStatus {
    AdapterStatus::Unavailable {
        reason: reason.to_string(),
    }
}

async fn stopped(shutdown: &mut watch::Receiver<bool>) {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}

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

fn observation(kind: ObservationKind) -> AdapterOutput {
    AdapterOutput::Observation(Observation {
        occurred_at: time::OffsetDateTime::now_utc(),
        kind,
    })
}

fn status(adapter: &impl DesktopAdapter, status: AdapterStatus) -> AdapterOutput {
    AdapterOutput::Status {
        adapter_id: adapter.id(),
        observation_type: adapter.observation_type(),
        status,
    }
}

async fn run_native(
    adapter: &impl DesktopAdapter,
    output: mpsc::Sender<AdapterOutput>,
    mut shutdown: watch::Receiver<bool>,
    worker: impl FnOnce(mpsc::Sender<AdapterOutput>, watch::Receiver<bool>) -> Result<(), ClientError>
    + Send
    + 'static,
) -> Result<(), ClientError> {
    if *shutdown.borrow() || shutdown.has_changed().is_err() || output.is_closed() {
        return Ok(());
    }
    let worker_output = output.clone();
    let worker_shutdown = shutdown.clone();
    let result =
        tokio::task::spawn_blocking(move || worker(worker_output, worker_shutdown)).await?;
    if let Err(error) = result {
        send(&output, &mut shutdown, status(adapter, unavailable(error))).await;
    }
    Ok(())
}

fn running(output: &mpsc::Sender<AdapterOutput>, shutdown: &watch::Receiver<bool>) -> bool {
    !output.is_closed() && !*shutdown.borrow() && shutdown.has_changed().is_ok()
}

fn send_native(
    output: &mpsc::Sender<AdapterOutput>,
    shutdown: &mut watch::Receiver<bool>,
    event: AdapterOutput,
) -> bool {
    tokio::runtime::Handle::current().block_on(send(output, shutdown, event))
}

fn readable(fd: &impl AsRawFd) -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: descriptor points to one initialized pollfd and remains valid during poll.
    let result = unsafe { libc::poll(&mut descriptor, 1, 100) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error);
    }
    if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "native event stream disconnected",
        ));
    }
    Ok(descriptor.revents & libc::POLLIN != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn adapters_cover_all_types_and_skip_setup_on_shutdown() {
        let mut ids = std::collections::HashSet::new();
        let adapters = default_kde_adapters();
        assert_eq!(adapters.len(), 5);
        let types: Vec<_> = adapters
            .iter()
            .map(|adapter| adapter.observation_type())
            .collect();
        use timetracker_model::ObservationType::*;
        assert_eq!(
            types,
            [FocusedApplication, Idle, Keyboard, MouseClick, MouseMove]
        );
        for mut adapter in adapters {
            assert!(ids.insert(adapter.id()));
            let (output, _receiver) = mpsc::channel(1);
            let (_stop, shutdown) = watch::channel(true);
            adapter.run(output, shutdown).await.unwrap();
        }
    }
}
