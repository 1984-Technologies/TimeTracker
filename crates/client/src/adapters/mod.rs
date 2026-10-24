use async_trait::async_trait;
use timetracker_model::{AdapterStatus, Observation, ObservationType};
use tokio::sync::{mpsc, watch};

use crate::ClientError;

pub enum AdapterOutput {
    Observation(Observation),
    Status {
        adapter_id: &'static str,
        observation_type: ObservationType,
        status: AdapterStatus,
    },
}

#[async_trait]
pub trait DesktopAdapter: Send {
    fn id(&self) -> &'static str;

    fn observation_type(&self) -> ObservationType;

    async fn probe(&self) -> AdapterStatus;

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError>;
}

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::default_adapters;
