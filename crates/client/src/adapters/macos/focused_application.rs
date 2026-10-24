use std::time::Duration;

use active_win_pos_rs::get_active_window;
use async_trait::async_trait;
use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::sync::{mpsc, watch};

use super::{send_observation, send_status, shutdown_requested, unavailable};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

const WINDOW_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

fn screen_capture_permission_available() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveWindow {
    app_id: String,
    title: Option<String>,
}

#[derive(Debug, Default)]
pub struct FocusedApplicationAdapter {
    previous: Option<ActiveWindow>,
}

impl FocusedApplicationAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    fn current_window() -> Result<Option<ActiveWindow>, String> {
        if !screen_capture_permission_available() {
            return Err(
                "Screen Recording permission is required to read the active window title. Enable it in System Settings → Privacy & Security → Screen Recording.".to_owned(),
            );
        }

        let Ok(window) = get_active_window() else {
            return Ok(None);
        };

        let process_path = window.process_path.to_string_lossy().into_owned();
        let app_id = if process_path.is_empty() {
            window.app_name
        } else {
            process_path
        };
        if app_id.trim().is_empty() {
            return Ok(None);
        }

        let title = (!window.title.trim().is_empty()).then_some(window.title);
        Ok(Some(ActiveWindow { app_id, title }))
    }
}

#[async_trait]
impl DesktopAdapter for FocusedApplicationAdapter {
    fn id(&self) -> &'static str {
        "macos.focused-application"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::FocusedApplication
    }

    async fn probe(&self) -> AdapterStatus {
        match Self::current_window() {
            Ok(_) => AdapterStatus::Available,
            Err(reason) => unavailable(reason),
        }
    }

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        if shutdown_requested(&shutdown) {
            return Ok(());
        }

        let mut ticker = tokio::time::interval(WINDOW_POLL_INTERVAL);
        let mut last_error: Option<String> = None;

        loop {
            if shutdown_requested(&shutdown) {
                return Ok(());
            }
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || shutdown_requested(&shutdown) {
                        return Ok(());
                    }
                }
                _ = ticker.tick() => {
                    match Self::current_window() {
                        Ok(window) => {
                            if last_error.take().is_some()
                                && !send_status(
                                    &output,
                                    self.id(),
                                    self.observation_type(),
                                    AdapterStatus::Available,
                                ).await
                            {
                                return Ok(());
                            }

                            if let Some(window) = window {
                                if self.previous.as_ref() != Some(&window) {
                                    if !send_observation(
                                        &output,
                                        OffsetDateTime::now_utc(),
                                        ObservationKind::FocusedApplicationChanged {
                                            app_id: window.app_id.clone(),
                                            title: window.title.clone(),
                                        },
                                    ).await {
                                        return Ok(());
                                    }
                                    self.previous = Some(window);
                                }
                            } else {
                                self.previous = None;
                            }
                        }
                        Err(reason) => {
                            if last_error.as_ref() != Some(&reason) {
                                if !send_status(
                                    &output,
                                    self.id(),
                                    self.observation_type(),
                                    unavailable(reason.clone()),
                                ).await {
                                    return Ok(());
                                }
                                last_error = Some(reason);
                            }
                        }
                    }
                }
            }
        }
    }
}
