mod focused_application;
mod idle;
mod input;

use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, Observation, ObservationKind, ObservationType};
use tokio::sync::{mpsc, watch};

use super::AdapterOutput;

pub use focused_application::FocusedApplicationAdapter;
pub use idle::IdleAdapter;
pub use input::{KeyboardAdapter, MouseClickAdapter, MouseMoveAdapter};

pub(super) fn unavailable(reason: impl Into<String>) -> AdapterStatus {
    AdapterStatus::Unavailable {
        reason: reason.into(),
    }
}

pub(super) async fn send_status(
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

pub(super) async fn send_observation(
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

pub(super) fn shutdown_requested(shutdown: &watch::Receiver<bool>) -> bool {
    *shutdown.borrow()
}

pub fn default_adapters(
    focused_application_events: mpsc::UnboundedReceiver<String>,
    initial_app_id: Option<String>,
) -> Vec<Box<dyn super::DesktopAdapter>> {
    vec![
        Box::new(FocusedApplicationAdapter::new(
            focused_application_events,
            initial_app_id,
        )),
        Box::new(IdleAdapter::default()),
        Box::new(KeyboardAdapter::new()),
        Box::new(MouseClickAdapter::new()),
        Box::new(MouseMoveAdapter::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::super::DesktopAdapter;
    use super::*;

    fn adapters() -> Vec<Box<dyn DesktopAdapter>> {
        let (_events_tx, events_rx) = mpsc::unbounded_channel();
        default_adapters(events_rx, None)
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
            assert_eq!(
                status,
                AdapterStatus::Available,
                "adapter {} ({:?}) is unavailable on this macOS session",
                adapter.id(),
                adapter.observation_type(),
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
}
