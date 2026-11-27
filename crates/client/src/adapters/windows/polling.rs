use std::{io, time::Duration};

use async_trait::async_trait;
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::sync::{mpsc, watch};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{
        SystemInformation::GetTickCount,
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
    },
    UI::{
        Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId},
    },
};

use super::{desktop_available, observation, send, stopped, unavailable};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

fn idle_duration() -> io::Result<Duration> {
    desktop_available()?;
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: info points to an initialized LASTINPUTINFO with the required size.
    unsafe {
        if GetLastInputInfo(&mut info) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(elapsed(GetTickCount(), info.dwTime))
    }
}

fn elapsed(now: u32, last: u32) -> Duration {
    // LASTINPUTINFO is also a 32-bit tick count: handle the 49.7-day wrap.
    Duration::from_millis(now.wrapping_sub(last) as u64)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveWindow {
    app_id: String,
    title: Option<String>,
}

fn active_window() -> io::Result<ActiveWindow> {
    desktop_available()?;
    // SAFETY: all output buffers are initialized and sized for the Win32 calls.
    // The process handle is closed before returning, including on API failure.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return Err(io::Error::other(
                "no foreground window on the input desktop",
            ));
        }
        let mut pid = 0;
        if GetWindowThreadProcessId(window, &mut pid) == 0 || pid == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        let result = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length);
        let error = io::Error::last_os_error();
        CloseHandle(process);
        if result == 0 {
            return Err(error);
        }
        let mut title = vec![0u16; 32768];
        let title_length = GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32);
        // Avoid pairing a title with a different application during focus changes.
        if GetForegroundWindow() != window {
            return Err(io::Error::other("foreground window changed while sampling"));
        }
        Ok(ActiveWindow {
            app_id: String::from_utf16_lossy(&path[..length as usize]).to_lowercase(),
            title: (title_length > 0)
                .then(|| String::from_utf16_lossy(&title[..title_length as usize])),
        })
    }
}

#[derive(Default)]
pub struct FocusedApplicationAdapter;

pub struct IdleAdapter {
    idle_threshold: Duration,
}

impl Default for IdleAdapter {
    fn default() -> Self {
        Self::new(Duration::from_secs(60))
    }
}

impl IdleAdapter {
    pub fn new(idle_threshold: Duration) -> Self {
        Self { idle_threshold }
    }
}

async fn poll<T: PartialEq + Clone>(
    adapter_id: &'static str,
    observation_type: ObservationType,
    output: mpsc::Sender<AdapterOutput>,
    mut shutdown: watch::Receiver<bool>,
    period: Duration,
    mut sample: impl FnMut() -> io::Result<T>,
    kind: impl Fn(T) -> ObservationKind,
) -> Result<(), ClientError> {
    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut previous = None;
    let mut status = None;
    loop {
        tokio::select! {
            biased;
            _ = stopped(&mut shutdown) => return Ok(()),
            _ = output.closed() => return Ok(()),
            _ = ticker.tick() => {}
        }
        let value = sample();
        let current_status = match &value {
            Ok(_) => AdapterStatus::Available,
            Err(error) => unavailable(error),
        };
        if status.as_ref() != Some(&current_status) {
            let event = AdapterOutput::Status {
                adapter_id,
                observation_type,
                status: current_status.clone(),
            };
            if !send(&output, &mut shutdown, event).await {
                return Ok(());
            }
            status = Some(current_status);
        }
        match value {
            Ok(value) if previous.as_ref() != Some(&value) => {
                previous = Some(value.clone());
                if !send(&output, &mut shutdown, observation(kind(value))).await {
                    return Ok(());
                }
            }
            Err(_) => previous = None,
            _ => {}
        }
    }
}

#[async_trait]
impl DesktopAdapter for FocusedApplicationAdapter {
    fn id(&self) -> &'static str {
        "windows.focused-application"
    }
    fn observation_type(&self) -> ObservationType {
        ObservationType::FocusedApplication
    }
    async fn probe(&self) -> AdapterStatus {
        active_window().map_or_else(unavailable, |_| AdapterStatus::Available)
    }
    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        poll(
            self.id(),
            self.observation_type(),
            output,
            shutdown,
            Duration::from_millis(500),
            active_window,
            |window| ObservationKind::FocusedApplicationChanged {
                app_id: window.app_id,
                title: window.title,
            },
        )
        .await
    }
}

#[async_trait]
impl DesktopAdapter for IdleAdapter {
    fn id(&self) -> &'static str {
        "windows.idle"
    }
    fn observation_type(&self) -> ObservationType {
        ObservationType::Idle
    }
    async fn probe(&self) -> AdapterStatus {
        idle_duration().map_or_else(unavailable, |_| AdapterStatus::Available)
    }
    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let threshold = self.idle_threshold;
        poll(
            self.id(),
            self.observation_type(),
            output,
            shutdown,
            Duration::from_secs(1),
            || idle_duration().map(|duration| duration >= threshold),
            |idle| ObservationKind::IdleChanged { idle },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_ticks_handle_wraparound() {
        assert_eq!(elapsed(15, u32::MAX - 4), Duration::from_millis(20));
        assert_eq!(elapsed(42, 42), Duration::ZERO);
    }

    #[tokio::test]
    async fn polling_deduplicates_changes_and_recovers_after_failure() {
        let (output, mut events) = mpsc::channel(32);
        let (shutdown, stop) = watch::channel(false);
        let mut samples = [
            Ok(false),
            Ok(false),
            Ok(true),
            Err(io::Error::other("unavailable")),
            Err(io::Error::other("unavailable")),
            Ok(true),
            Ok(false),
        ]
        .into_iter();
        let task = tokio::spawn(poll(
            "test",
            ObservationType::Idle,
            output,
            stop,
            Duration::from_millis(1),
            move || samples.next().unwrap_or(Ok(false)),
            |idle| ObservationKind::IdleChanged { idle },
        ));
        let mut states = Vec::new();
        let mut statuses = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), async {
            while states.len() < 4 {
                match events.recv().await.unwrap() {
                    AdapterOutput::Observation(event) => {
                        if let ObservationKind::IdleChanged { idle } = event.kind {
                            states.push(idle);
                        }
                    }
                    AdapterOutput::Status { status, .. } => statuses.push(status),
                }
            }
        })
        .await
        .unwrap();
        shutdown.send(true).unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(states, [false, true, true, false]);
        assert_eq!(
            statuses,
            [
                AdapterStatus::Available,
                unavailable("unavailable"),
                AdapterStatus::Available
            ]
        );
    }
}
