use std::time::Duration;

use async_trait::async_trait;
use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::sync::{mpsc, watch};

use super::{send_observation, send_status, shutdown_requested, unavailable};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_IDLE_THRESHOLD: Duration = Duration::from_secs(60);

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
}

fn idle_seconds() -> Option<f64> {
    let seconds = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
}

pub struct IdleAdapter {
    idle_threshold: Duration,
    previous_idle: Option<bool>,
}

impl Default for IdleAdapter {
    fn default() -> Self {
        Self::new(DEFAULT_IDLE_THRESHOLD)
    }
}

impl IdleAdapter {
    pub fn new(idle_threshold: Duration) -> Self {
        Self {
            idle_threshold,
            previous_idle: None,
        }
    }
}

#[async_trait]
impl DesktopAdapter for IdleAdapter {
    fn id(&self) -> &'static str {
        "macos.idle-sleep"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::Idle
    }

    async fn probe(&self) -> AdapterStatus {
        if idle_seconds().is_none() {
            return unavailable("macOS could not read the time since the last user input");
        }

        match powerwatch::PowerWatch::start() {
            Ok((watcher, _events)) => {
                drop(watcher);
                AdapterStatus::Available
            }
            Err(error) => unavailable(format!(
                "macOS sleep/wake notifications could not be registered: {error}"
            )),
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

        let mut sleep_watch = match powerwatch::PowerWatch::start() {
            Ok(watch) => Some(watch),
            Err(error) => {
                if !send_status(
                    &output,
                    self.id(),
                    self.observation_type(),
                    unavailable(format!(
                        "macOS sleep/wake notifications could not be registered: {error}"
                    )),
                )
                .await
                {
                    return Ok(());
                }
                None
            }
        };

        let mut ticker = tokio::time::interval(IDLE_POLL_INTERVAL);
        let mut idle_error_reported = false;

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
                    match idle_seconds() {
                        Some(seconds) => {
                            if idle_error_reported {
                                if !send_status(
                                    &output,
                                    self.id(),
                                    self.observation_type(),
                                    AdapterStatus::Available,
                                ).await {
                                    return Ok(());
                                }
                                idle_error_reported = false;
                            }
                            let idle = seconds >= self.idle_threshold.as_secs_f64();
                            if self.previous_idle != Some(idle) {
                                if !send_observation(
                                    &output,
                                    OffsetDateTime::now_utc(),
                                    ObservationKind::IdleChanged { idle },
                                ).await {
                                    return Ok(());
                                }
                                self.previous_idle = Some(idle);
                            }
                        }
                        None if !idle_error_reported => {
                            if !send_status(
                                &output,
                                self.id(),
                                self.observation_type(),
                                unavailable("macOS failed to read the idle timer"),
                            ).await {
                                return Ok(());
                            }
                            idle_error_reported = true;
                        }
                        None => {}
                    }
                }
                power_event = async {
                    match sleep_watch.as_mut() {
                        Some((_watch, events)) => events.recv_async().await.map(Some),
                        None => std::future::pending().await,
                    }
                } => {
                    match power_event {
                        Ok(Some(event)) => {
                            match event {
                                powerwatch::PowerEvent::Suspend => {
                                    if !send_observation(
                                        &output,
                                        OffsetDateTime::now_utc(),
                                        ObservationKind::SystemSleepChanged { sleeping: true },
                                    ).await {
                                        return Ok(());
                                    }
                                }
                                powerwatch::PowerEvent::Resume => {
                                    if !send_observation(
                                        &output,
                                        OffsetDateTime::now_utc(),
                                        ObservationKind::SystemSleepChanged { sleeping: false },
                                    ).await {
                                        return Ok(());
                                    }
                                }
                                _ => {}
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            if !send_status(
                                &output,
                                self.id(),
                                self.observation_type(),
                                unavailable(format!("macOS sleep/wake event handler failed: {error}")),
                            ).await {
                                return Ok(());
                            }
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}
