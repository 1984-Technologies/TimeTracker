use std::{
    env, io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use async_trait::async_trait;
use serde_json::Value;
use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, Observation, ObservationKind, ObservationType};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::{mpsc, watch},
};

use super::{AdapterOutput, DesktopAdapter};
use crate::ClientError;

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const KWIN_SERVICE: &str = "org.kde.KWin";
const KWIN_PLUGIN_NAME: &str = "timetracker-focused-application";

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveApplication {
    app_id: String,
    title: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backend {
    KwinWayland,
    X11,
}

pub struct FocusedApplicationAdapter {
    previous: Option<ActiveApplication>,
}

impl FocusedApplicationAdapter {
    pub fn new() -> Self {
        Self { previous: None }
    }

    fn preferred_backend() -> Backend {
        let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        if env::var_os("WAYLAND_DISPLAY").is_some()
            && desktop
                .split(':')
                .any(|part| part.eq_ignore_ascii_case("KDE"))
        {
            Backend::KwinWayland
        } else {
            Backend::X11
        }
    }

    async fn publish(
        &mut self,
        application: ActiveApplication,
        output: &mpsc::Sender<AdapterOutput>,
    ) -> Result<bool, ClientError> {
        if self.previous.as_ref() == Some(&application) {
            return Ok(true);
        }
        let observation = Observation {
            occurred_at: OffsetDateTime::now_utc(),
            kind: ObservationKind::FocusedApplicationChanged {
                app_id: application.app_id.clone(),
                title: application.title.clone(),
            },
        };
        if output
            .send(AdapterOutput::Observation(observation))
            .await
            .is_err()
        {
            return Ok(false);
        }
        self.previous = Some(application);
        Ok(true)
    }

    async fn probe_backend(backend: Backend) -> Result<(), ClientError> {
        match backend {
            Backend::KwinWayland => {
                command_output(
                    "qdbus6",
                    &[KWIN_SERVICE, "/KWin", "org.kde.KWin.activeOutputName"],
                )
                .await?;
                command_output("journalctl", &["--user", "-n", "0", "--no-pager"]).await?;
                Ok(())
            }
            Backend::X11 => {
                if env::var_os("DISPLAY").is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "DISPLAY is not set and no supported Wayland compositor was detected",
                    )
                    .into());
                }
                command_output("xprop", &["-root", "_NET_ACTIVE_WINDOW"])
                    .await
                    .map(|_| ())
            }
        }
    }

    async fn run_x11(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
                _ = ticker.tick() => {
                    if let Some(application) = read_x11_active_application().await?
                        && !self.publish(application, &output).await?
                    {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn run_kwin(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let process_id = std::process::id();
        let marker = format!("TIMETRACKER_FOCUS_{process_id}:");
        let script_path = write_kwin_script(&marker)?;

        // Clean up a script left behind if an earlier client was killed without a graceful stop.
        let _ = unload_kwin_script(KWIN_PLUGIN_NAME).await;

        let mut journal = Command::new("journalctl")
            .args([
                "--user",
                "--follow",
                "--since",
                "now",
                "--output=cat",
                "--no-pager",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = journal.stdout.take().ok_or_else(|| {
            io::Error::other("journalctl did not provide a readable stdout stream")
        })?;
        let mut lines = BufReader::new(stdout).lines();

        tokio::time::sleep(Duration::from_millis(100)).await;
        let script_id = match load_kwin_script(&script_path, KWIN_PLUGIN_NAME).await {
            Ok(script_id) => script_id,
            Err(error) => {
                let _ = journal.kill().await;
                let _ = std::fs::remove_file(&script_path);
                return Err(error);
            }
        };

        let run_result = command_output(
            "qdbus6",
            &[
                KWIN_SERVICE,
                &format!("/Scripting/Script{script_id}"),
                "org.kde.kwin.Script.run",
            ],
        )
        .await;

        let result = match run_result {
            Err(error) => Err(error),
            Ok(_) => loop {
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break Ok(());
                        }
                    }
                    line = lines.next_line() => {
                        let Some(line) = line? else {
                            break Err(io::Error::other("journalctl event stream closed").into());
                        };
                        if let Some(application) = parse_kwin_line(&line, &marker)?
                            && !self.publish(application, &output).await?
                        {
                            break Ok(());
                        }
                    }
                }
            },
        };

        let _ = journal.kill().await;
        let _ = journal.wait().await;
        let _ = unload_kwin_script(KWIN_PLUGIN_NAME).await;
        let _ = std::fs::remove_file(script_path);
        result
    }
}

impl Default for FocusedApplicationAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DesktopAdapter for FocusedApplicationAdapter {
    fn id(&self) -> &'static str {
        "linux.focused-application"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::FocusedApplication
    }

    async fn probe(&self) -> AdapterStatus {
        let backend = Self::preferred_backend();
        match Self::probe_backend(backend).await {
            Ok(()) => AdapterStatus::Available,
            Err(error) => AdapterStatus::Unavailable {
                reason: format!("{} backend is unavailable: {error}", backend.name()),
            },
        }
    }

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        match Self::preferred_backend() {
            Backend::KwinWayland => self.run_kwin(output, shutdown).await,
            Backend::X11 => self.run_x11(output, shutdown).await,
        }
    }
}

impl Backend {
    fn name(self) -> &'static str {
        match self {
            Self::KwinWayland => "KWin Wayland",
            Self::X11 => "X11/EWMH",
        }
    }
}

pub fn default_adapters() -> Vec<Box<dyn DesktopAdapter>> {
    vec![Box::new(FocusedApplicationAdapter::new())]
}

async fn command_output(program: &str, args: &[&str]) -> Result<String, ClientError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("could not execute {program}: {error}"),
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "{program} exited with {}: {}",
            output.status,
            stderr.trim()
        ))
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

async fn read_x11_active_application() -> Result<Option<ActiveApplication>, ClientError> {
    let root = command_output("xprop", &["-root", "_NET_ACTIVE_WINDOW"]).await?;
    let Some(window_id) = root.split_whitespace().last() else {
        return Ok(None);
    };
    if window_id == "0x0" || !window_id.starts_with("0x") {
        return Ok(None);
    }

    let properties = command_output(
        "xprop",
        &["-id", window_id, "WM_CLASS", "_NET_WM_NAME", "WM_NAME"],
    )
    .await?;
    Ok(parse_xprop_properties(&properties))
}

fn parse_xprop_properties(properties: &str) -> Option<ActiveApplication> {
    let class_line = properties
        .lines()
        .find(|line| line.starts_with("WM_CLASS"))?;
    let classes = quoted_values(class_line);
    let app_id = classes.last()?.trim().to_owned();
    if app_id.is_empty() {
        return None;
    }

    let title = properties
        .lines()
        .find(|line| line.starts_with("_NET_WM_NAME") && !line.contains("not found"))
        .or_else(|| {
            properties
                .lines()
                .find(|line| line.starts_with("WM_NAME") && !line.contains("not found"))
        })
        .and_then(|line| quoted_values(line).into_iter().next())
        .filter(|title| !title.trim().is_empty());

    Some(ActiveApplication { app_id, title })
}

fn quoted_values(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' && quoted {
            escaped = true;
        } else if character == '"' {
            if quoted {
                values.push(std::mem::take(&mut current));
            }
            quoted = !quoted;
        } else if quoted {
            current.push(character);
        }
    }
    values
}

fn write_kwin_script(marker: &str) -> Result<PathBuf, ClientError> {
    let path = env::temp_dir().join(format!(
        "timetracker-kwin-focused-application-{}.js",
        std::process::id()
    ));
    let marker = serde_json::to_string(marker)?;
    let source = format!(
        r#"const marker = {marker};
function reportActiveWindow() {{
    const window = workspace.activeWindow;
    if (!window) return;
    const appId = window.desktopFileName || window.resourceClass || window.resourceName;
    if (!appId) return;
    print(marker + JSON.stringify({{
        app_id: String(appId),
        title: window.caption ? String(window.caption) : null
    }}));
}}
workspace.windowActivated.connect(reportActiveWindow);
reportActiveWindow();
"#
    );
    std::fs::write(&path, source)?;
    Ok(path)
}

async fn load_kwin_script(path: &Path, plugin_name: &str) -> Result<i32, ClientError> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("temporary KWin script path is not valid UTF-8"))?;
    let output = command_output(
        "qdbus6",
        &[
            KWIN_SERVICE,
            "/Scripting",
            "org.kde.kwin.Scripting.loadScript",
            path,
            plugin_name,
        ],
    )
    .await?;
    output.parse::<i32>().map_err(|error| {
        io::Error::other(format!(
            "KWin returned an invalid script id '{output}': {error}"
        ))
        .into()
    })
}

async fn unload_kwin_script(plugin_name: &str) -> Result<(), ClientError> {
    command_output(
        "qdbus6",
        &[
            KWIN_SERVICE,
            "/Scripting",
            "org.kde.kwin.Scripting.unloadScript",
            plugin_name,
        ],
    )
    .await
    .map(|_| ())
}

fn parse_kwin_line(line: &str, marker: &str) -> Result<Option<ActiveApplication>, ClientError> {
    let Some(payload) = line.strip_prefix(marker) else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(payload)?;
    let Some(app_id) = value.get("app_id").and_then(Value::as_str) else {
        return Ok(None);
    };
    if app_id.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(ActiveApplication {
        app_id: app_id.to_owned(),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }))
}

#[cfg(test)]
mod tests {
    use super::{ActiveApplication, parse_kwin_line, parse_xprop_properties};

    #[test]
    fn parses_xprop_application_and_utf8_title() {
        let properties = r#"WM_CLASS(STRING) = "code", "Code"
_NET_WM_NAME(UTF8_STRING) = "TimeTracker — main.rs"
WM_NAME(STRING) = "fallback"
"#;
        assert_eq!(
            parse_xprop_properties(properties),
            Some(ActiveApplication {
                app_id: "Code".to_owned(),
                title: Some("TimeTracker — main.rs".to_owned()),
            })
        );
    }

    #[test]
    fn parses_only_matching_kwin_journal_lines() {
        assert_eq!(parse_kwin_line("unrelated", "FOCUS:").unwrap(), None);
        assert_eq!(
            parse_kwin_line(
                r#"FOCUS:{"app_id":"org.kde.konsole","title":"Terminal"}"#,
                "FOCUS:"
            )
            .unwrap(),
            Some(ActiveApplication {
                app_id: "org.kde.konsole".to_owned(),
                title: Some("Terminal".to_owned()),
            })
        );
    }
}
