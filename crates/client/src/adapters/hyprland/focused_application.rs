use std::{env, io, path::PathBuf, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{mpsc, watch},
};

use super::{observation, send, status, stopped, unavailable};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

const IPC_TIMEOUT: Duration = Duration::from_secs(2);

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
        directory: &std::path::Path,
        output: &mpsc::Sender<AdapterOutput>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let events = tokio::time::timeout(
            IPC_TIMEOUT,
            UnixStream::connect(directory.join(".socket2.sock")),
        )
        .await??;
        let mut lines = BufReader::new(events).lines();
        let mut window = current_window(directory, self.collect_titles).await?;
        let mut previous = window.observation();
        if !send(output, shutdown, status(self, AdapterStatus::Available)).await
            || !send(output, shutdown, observation(previous.clone())).await
        {
            return Ok(());
        }
        loop {
            let line = tokio::select! {
                biased;
                _ = stopped(shutdown) => return Ok(()),
                _ = output.closed() => return Ok(()),
                line = lines.next_line() => line?,
            };
            let Some(line) = line else {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Hyprland event socket closed",
                )
                .into());
            };
            window.update(&line, self.collect_titles)?;
            let next = window.observation();
            if next != previous {
                if !send(output, shutdown, observation(next.clone())).await {
                    return Ok(());
                }
                previous = next;
            }
        }
    }
}

#[async_trait]
impl DesktopAdapter for FocusedApplicationAdapter {
    fn id(&self) -> &'static str {
        "hyprland.focused-application"
    }
    fn observation_type(&self) -> ObservationType {
        ObservationType::FocusedApplication
    }

    async fn probe(&self) -> AdapterStatus {
        let result = async {
            let directory = socket_directory()?;
            tokio::time::timeout(
                IPC_TIMEOUT,
                UnixStream::connect(directory.join(".socket2.sock")),
            )
            .await??;
            current_window(&directory, self.collect_titles).await
        }
        .await;
        match result {
            Ok(_) => AdapterStatus::Available,
            Err(error) => unavailable(error),
        }
    }

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let mut collecting_shutdown = shutdown.clone();
        let result = tokio::select! {
            biased;
            _ = stopped(&mut shutdown) => return Ok(()),
            _ = output.closed() => return Ok(()),
            result = async {
                let directory = socket_directory()?;
                self.collect(&directory, &output, &mut collecting_shutdown).await
            } => result,
        };
        if let Err(error) = result {
            send(&output, &mut shutdown, status(self, unavailable(error))).await;
        }
        Ok(())
    }
}

pub(super) fn socket_directory() -> Result<PathBuf, ClientError> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .ok_or("XDG_RUNTIME_DIR is not set; run inside the Hyprland session")?;
    let signature = env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .map_err(|_| "HYPRLAND_INSTANCE_SIGNATURE is not set; run inside the Hyprland session")?;
    if signature.is_empty() || signature == "." || signature == ".." || signature.contains('/') {
        return Err("invalid HYPRLAND_INSTANCE_SIGNATURE".into());
    }
    Ok(PathBuf::from(runtime).join("hypr").join(signature))
}

async fn current_window(
    directory: &std::path::Path,
    collect_titles: bool,
) -> Result<Window, ClientError> {
    parse_window(
        &request(directory, b"j/activewindow").await?,
        collect_titles,
    )
}

pub(super) async fn request(
    directory: &std::path::Path,
    command: &[u8],
) -> Result<Vec<u8>, ClientError> {
    tokio::time::timeout(IPC_TIMEOUT, async {
        let mut socket = UnixStream::connect(directory.join(".socket.sock")).await?;
        socket.write_all(command).await?;
        let mut response = Vec::new();
        socket.take(1024 * 1024).read_to_end(&mut response).await?;
        Ok(response)
    })
    .await?
}

#[derive(Default, Deserialize)]
struct Window {
    #[serde(default)]
    address: String,
    class: String,
    title: Option<String>,
}

impl Window {
    fn observation(&self) -> ObservationKind {
        ObservationKind::FocusedApplicationChanged {
            app_id: self.class.clone(),
            title: self.title.clone(),
        }
    }

    fn update(&mut self, line: &str, collect_titles: bool) -> Result<(), ClientError> {
        match line.split_once(">>") {
            Some(("activewindow", data)) => {
                let (class, title) = data
                    .split_once(',')
                    .ok_or("invalid Hyprland activewindow event")?;
                self.class = class.into();
                self.title = (collect_titles && !title.is_empty()).then(|| title.into());
                self.address.clear();
            }
            Some(("activewindowv2", address)) => {
                self.address = address.trim_start_matches("0x").into()
            }
            Some(("windowtitlev2", data)) if collect_titles => {
                let (address, title) = data
                    .split_once(',')
                    .ok_or("invalid Hyprland windowtitlev2 event")?;
                if !self.address.is_empty() && address.trim_start_matches("0x") == self.address {
                    self.title = (!title.is_empty()).then(|| title.into());
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn parse_window(response: &[u8], collect_titles: bool) -> Result<Window, ClientError> {
    let json: serde_json::Value = serde_json::from_slice(response)?;
    if json.as_object().is_some_and(|object| object.is_empty()) {
        // The model represents loss of focus with an empty application identifier.
        return Ok(Window::default());
    }
    let mut window: Window = serde_json::from_value(json)?;
    window.address = window.address.trim_start_matches("0x").into();
    window.title = window
        .title
        .filter(|title| collect_titles && !title.is_empty());
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_titles_without_splitting_commas_and_requires_opt_in() {
        let json = br#"{"class":"terminal","title":"one,two"}"#;
        assert_eq!(
            parse_window(json, true).unwrap().observation(),
            ObservationKind::FocusedApplicationChanged {
                app_id: "terminal".into(),
                title: Some("one,two".into())
            }
        );
        assert_eq!(
            parse_window(json, false).unwrap().observation(),
            ObservationKind::FocusedApplicationChanged {
                app_id: "terminal".into(),
                title: None
            }
        );
        assert_eq!(
            parse_window(b"{}", true).unwrap().observation(),
            ObservationKind::FocusedApplicationChanged {
                app_id: String::new(),
                title: None
            }
        );
        assert!(parse_window(b"null", true).is_err());
        assert!(parse_window(br#"{"title":"missing class"}"#, true).is_err());
        assert!(parse_window(b"not json", true).is_err());
    }

    #[tokio::test]
    async fn socket_stream_preserves_rapid_focus_changes_and_reports_disconnect() {
        use tokio::net::UnixListener;
        let directory = std::env::temp_dir().join(format!("tt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let commands = UnixListener::bind(directory.join(".socket.sock")).unwrap();
        let events = UnixListener::bind(directory.join(".socket2.sock")).unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = commands.accept().await.unwrap();
            let mut command = [0; 14];
            socket.read_exact(&mut command).await.unwrap();
            assert_eq!(&command, b"j/activewindow");
            socket
                .write_all(br#"{"class":"initial","title":"private","address":"0x123"}"#)
                .await
                .unwrap();
        });
        let (output, mut receiver) = mpsc::channel(16);
        let (_stop, mut shutdown) = watch::channel(false);
        let collector_directory = directory.clone();
        let collector = tokio::spawn(async move {
            FocusedApplicationAdapter::new(true)
                .collect(&collector_directory, &output, &mut shutdown)
                .await
        });
        let (mut socket, _) = events.accept().await.unwrap();
        server.await.unwrap();
        assert!(matches!(
            receiver.recv().await,
            Some(AdapterOutput::Status {
                status: AdapterStatus::Available,
                ..
            })
        ));
        socket.write_all(b"activewindow>>second,one,two\nactivewindowv2>>456\nwindowtitlev2>>999,background\nwindowtitlev2>>456,new,title\nactivewindow>>third,\nactivewindowv2>>789\nactivewindow>>third,\nactivewindow>>,\n").await.unwrap();
        for (class, title) in [
            ("initial", Some("private")),
            ("second", Some("one,two")),
            ("second", Some("new,title")),
            ("third", None),
            ("", None),
        ] {
            let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
                .await
                .unwrap()
                .unwrap();
            let AdapterOutput::Observation(event) = event else {
                panic!("expected observation")
            };
            assert_eq!(
                event.kind,
                ObservationKind::FocusedApplicationChanged {
                    app_id: class.into(),
                    title: title.map(Into::into)
                }
            );
        }
        drop(socket);
        assert!(
            tokio::time::timeout(Duration::from_secs(2), collector)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        assert!(receiver.recv().await.is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
