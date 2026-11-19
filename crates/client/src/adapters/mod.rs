use async_trait::async_trait;
use timetracker_model::{AdapterStatus, Observation, ObservationType};
use tokio::sync::{mpsc, watch};

use crate::ClientError;

#[cfg(target_os = "linux")]
pub mod hyprland;

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
pub use macos::{MacOsAdapterRuntime, MacOsRuntimeShutdownHandle, default_adapters};

#[cfg(target_os = "linux")]
pub fn default_adapters() -> Result<Vec<Box<dyn DesktopAdapter>>, crate::ClientError> {
    return Ok(hyprland::default_hyprland_adapters());
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn default_adapters() -> Result<Vec<Box<dyn DesktopAdapter>>, crate::ClientError> {
    Ok(Vec::new())
}
