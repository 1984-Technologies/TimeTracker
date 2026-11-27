use async_trait::async_trait;
use timetracker_model::{AdapterStatus, Observation, ObservationType};
use tokio::sync::{mpsc, watch};

use crate::ClientError;

#[cfg(target_os = "linux")]
pub mod hyprland;
#[cfg(target_os = "windows")]
pub mod windows;

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
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
    if is_hyprland_session(desktop.as_deref(), signature.as_deref()) {
        Ok(hyprland::default_hyprland_adapters())
    } else {
        Ok(Vec::new())
    }
}

#[cfg(target_os = "linux")]
fn is_hyprland_session(desktop: Option<&str>, signature: Option<&str>) -> bool {
    // XDG_CURRENT_DESKTOP is a colon-separated list. Prefer it over a possibly
    // inherited instance signature; minimal sessions may only set the signature.
    match desktop.filter(|desktop| !desktop.trim().is_empty()) {
        Some(desktop) => desktop
            .split(':')
            .any(|name| name.trim().eq_ignore_ascii_case("Hyprland")),
        None => signature.is_some_and(|signature| !signature.trim().is_empty()),
    }
}

#[cfg(target_os = "windows")]
pub fn default_adapters() -> Result<Vec<Box<dyn DesktopAdapter>>, crate::ClientError> {
    Ok(windows::default_adapters())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn default_adapters() -> Result<Vec<Box<dyn DesktopAdapter>>, crate::ClientError> {
    Ok(Vec::new())
}
