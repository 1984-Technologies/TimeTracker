use std::{path::Path, time::Duration};

use active_win_pos_rs::get_active_window;
use async_trait::async_trait;
use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::sync::{mpsc, watch};

use super::{send_observation, shutdown_requested};
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

pub struct FocusedApplicationAdapter {
    application_events: mpsc::UnboundedReceiver<String>,
    initial_app_id: Option<String>,
    previous: Option<ActiveWindow>,
}

impl FocusedApplicationAdapter {
    pub fn new(
        application_events: mpsc::UnboundedReceiver<String>,
        initial_app_id: Option<String>,
    ) -> Self {
        Self {
            application_events,
            initial_app_id,
            previous: None,
        }
    }

    fn title_for_application(app_id: &str) -> Option<String> {
        if !screen_capture_permission_available() {
            return None;
        }

        let window = get_active_window().ok()?;
        let title = window.title.trim();
        if title.is_empty() {
            return None;
        }

        let expected_name = app_id.rsplit('.').next().unwrap_or(app_id);
        let expected_name_lower = expected_name.to_ascii_lowercase();
        let process_path = window.process_path.to_string_lossy().into_owned();
        let process_bundle_name = Path::new(&process_path)
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .find_map(|component| component.strip_suffix(".app"));

        let path_matches = process_bundle_name.is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            name == expected_name_lower || name.ends_with(&format!(" {expected_name_lower}"))
        });
        let app_name = window.app_name.trim().to_ascii_lowercase();
        let reported_name_matches = app_name == expected_name_lower
            || app_name.ends_with(&format!(" {expected_name_lower}"));

        (path_matches || reported_name_matches).then(|| title.to_owned())
    }

    async fn publish_application(
        &mut self,
        app_id: String,
        output: &mpsc::Sender<AdapterOutput>,
    ) -> bool {
        let window = ActiveWindow {
            title: Self::title_for_application(&app_id),
            app_id,
        };

        if self.previous.as_ref() == Some(&window) {
            return true;
        }

        if !send_observation(
            output,
            OffsetDateTime::now_utc(),
            ObservationKind::FocusedApplicationChanged {
                app_id: window.app_id.clone(),
                title: window.title.clone(),
            },
        )
        .await
        {
            return false;
        }

        self.previous = Some(window);
        true
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
        AdapterStatus::Available
    }

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        if shutdown_requested(&shutdown) {
            return Ok(());
        }

        let mut active_app_id = self.initial_app_id.take();
        if let Some(app_id) = active_app_id.clone() {
            if !self.publish_application(app_id, &output).await {
                return Ok(());
            }
        }

        let mut ticker = tokio::time::interval(WINDOW_POLL_INTERVAL);
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
                event = self.application_events.recv() => {
                    let Some(app_id) = event else {
                        return Ok(());
                    };
                    active_app_id = Some(app_id.clone());
                    if !self.publish_application(app_id, &output).await {
                        return Ok(());
                    }
                }
                _ = ticker.tick() => {
                    if let Some(app_id) = active_app_id.clone() {
                        if !self.publish_application(app_id, &output).await {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}
