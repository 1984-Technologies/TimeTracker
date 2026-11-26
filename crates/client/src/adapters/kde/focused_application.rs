use async_trait::async_trait;
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;
use zbus::{Connection, interface};

use super::{
    kwin::{KWinScriptSession, probe_scripting},
    observation, send, status, stopped, unavailable,
};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

const DBUS_INTERFACE: &str = "io.timetracker.KWinFocus";

#[derive(Debug, Clone, PartialEq, Eq)]
struct FocusEvent {
    app_id: String,
    title: String,
}

struct FocusReceiver {
    tx: mpsc::UnboundedSender<FocusEvent>,
}

#[interface(name = "io.timetracker.KWinFocus")]
impl FocusReceiver {
    #[zbus(name = "FocusChanged")]
    async fn focus_changed(&self, app_id: String, title: String) {
        let _ = self.tx.send(FocusEvent { app_id, title });
    }
}

fn build_focus_script(service: &str, path: &str) -> String {
    format!(
        r#"
const SERVICE = {service:?};
const PATH = {path:?};
const INTERFACE = {DBUS_INTERFACE:?};

let trackedWindow = null;
let captionHandler = null;

function emitWindow(win) {{
    if (!win) {{
        callDBus(SERVICE, PATH, INTERFACE, "FocusChanged", "", "");
        return;
    }}
    const appId = String(win.resourceClass || win.desktopFileName || win.resourceName || "");
    const title = String(win.caption || "");
    callDBus(SERVICE, PATH, INTERFACE, "FocusChanged", appId, title);
}}

function trackWindow(win) {{
    if (trackedWindow && captionHandler) {{
        try {{ trackedWindow.captionChanged.disconnect(captionHandler); }} catch (e) {{}}
    }}
    trackedWindow = win;
    captionHandler = null;
    emitWindow(win);
    if (win) {{
        captionHandler = function() {{
            if (workspace.activeWindow === win) {{
                emitWindow(win);
            }}
        }};
        win.captionChanged.connect(captionHandler);
    }}
}}

workspace.windowActivated.connect(trackWindow);
trackWindow(workspace.activeWindow);
"#
    )
}

fn to_observation(event: &FocusEvent, collect_titles: bool) -> ObservationKind {
    let app_id = event.app_id.trim().to_owned();
    let title = if collect_titles {
        let trimmed = event.title.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    } else {
        None
    };
    ObservationKind::FocusedApplicationChanged { app_id, title }
}

#[derive(Default)]
pub struct FocusedApplicationAdapter {
    collect_titles: bool,
}

impl FocusedApplicationAdapter {
    /// Window titles are collected only when explicitly enabled.
    pub fn new(collect_titles: bool) -> Self {
        Self { collect_titles }
    }

    async fn collect(
        &self,
        output: &mpsc::Sender<AdapterOutput>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let connection = Connection::session().await?;
        let unique_name = connection
            .unique_name()
            .ok_or("session D-Bus connection has no unique name")?
            .as_str()
            .to_owned();
        let object_path = format!("/io/timetracker/KWinFocus/{}", Uuid::new_v4().simple());

        let (tx, mut rx) = mpsc::unbounded_channel();
        connection
            .object_server()
            .at(object_path.as_str(), FocusReceiver { tx })
            .await?;

        let script = build_focus_script(&unique_name, &object_path);
        let session = match KWinScriptSession::start(
            connection.clone(),
            "timetracker_focus",
            &script,
        )
        .await
        {
            Ok(session) => session,
            Err(error) => {
                let _ = connection
                    .object_server()
                    .remove::<FocusReceiver, _>(object_path.as_str())
                    .await;
                return Err(error);
            }
        };

        let result = self.forward_events(&mut rx, output, shutdown).await;

        session.stop().await;
        let _ = connection
            .object_server()
            .remove::<FocusReceiver, _>(object_path.as_str())
            .await;

        result
    }

    async fn forward_events(
        &self,
        rx: &mut mpsc::UnboundedReceiver<FocusEvent>,
        output: &mpsc::Sender<AdapterOutput>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        if !send(output, shutdown, status(self, AdapterStatus::Available)).await {
            return Ok(());
        }

        let mut previous: Option<ObservationKind> = None;
        loop {
            let event = tokio::select! {
                biased;
                _ = stopped(shutdown) => return Ok(()),
                _ = output.closed() => return Ok(()),
                event = rx.recv() => event,
            };
            let Some(event) = event else {
                return Ok(());
            };
            let next = to_observation(&event, self.collect_titles);
            if previous.as_ref() != Some(&next) {
                if !send(output, shutdown, observation(next.clone())).await {
                    return Ok(());
                }
                previous = Some(next);
            }
        }
    }
}

#[async_trait]
impl DesktopAdapter for FocusedApplicationAdapter {
    fn id(&self) -> &'static str {
        "kde.focused-application"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::FocusedApplication
    }

    async fn probe(&self) -> AdapterStatus {
        match probe_scripting().await {
            Ok(()) => AdapterStatus::Available,
            Err(error) => unavailable(error),
        }
    }

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        if *shutdown.borrow() || shutdown.has_changed().is_err() || output.is_closed() {
            return Ok(());
        }
        if let Err(error) = self.collect(&output, &mut shutdown).await {
            send(&output, &mut shutdown, status(self, unavailable(error))).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_focus_events_and_respects_title_opt_in() {
        let event = FocusEvent {
            app_id: " org.kde.konsole ".into(),
            title: " bash — Konsole ".into(),
        };
        assert_eq!(
            to_observation(&event, true),
            ObservationKind::FocusedApplicationChanged {
                app_id: "org.kde.konsole".into(),
                title: Some("bash — Konsole".into()),
            }
        );
        assert_eq!(
            to_observation(&event, false),
            ObservationKind::FocusedApplicationChanged {
                app_id: "org.kde.konsole".into(),
                title: None,
            }
        );
        let unfocused = FocusEvent {
            app_id: String::new(),
            title: "   ".into(),
        };
        assert_eq!(
            to_observation(&unfocused, true),
            ObservationKind::FocusedApplicationChanged {
                app_id: String::new(),
                title: None,
            }
        );
    }
}
