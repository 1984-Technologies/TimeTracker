use std::{env, ffi::OsString, sync::Arc, time::Duration};

use timetracker_client::{
    ClientError, event_service::ClientEventService, identity::InstallationIdentity,
    queue::EventQueue, supervisor::CollectionSupervisor, upload::GrpcUploadClient,
};
use timetracker_model::EventKind;
use tokio::sync::{mpsc, watch};

const DEFAULT_SERVER: &str = "http://127.0.0.1:50051";
const DEFAULT_HEARTBEAT_SECONDS: u64 = 30;
const DEFAULT_BUFFER_CAPACITY: usize = 100;

#[derive(Debug, Eq, PartialEq)]
struct Config {
    grpc_address: String,
    heartbeat_seconds: u64,
    buffer_capacity: usize,
}

impl Config {
    fn from_env_and_args() -> Result<Option<Self>, String> {
        let grpc_address = env::var("TIMETRACKER_SERVER").unwrap_or_else(|_| DEFAULT_SERVER.into());
        let heartbeat_seconds = parse_positive(
            env::var("TIMETRACKER_HEARTBEAT_SECONDS").ok().as_deref(),
            DEFAULT_HEARTBEAT_SECONDS,
            "TIMETRACKER_HEARTBEAT_SECONDS",
        )?;
        Self::parse(env::args_os().skip(1), grpc_address, heartbeat_seconds)
    }

    fn parse(
        args: impl IntoIterator<Item = OsString>,
        mut grpc_address: String,
        mut heartbeat_seconds: u64,
    ) -> Result<Option<Self>, String> {
        let mut buffer_capacity = DEFAULT_BUFFER_CAPACITY;
        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            let argument = argument
                .into_string()
                .map_err(|_| "command-line arguments must be valid UTF-8".to_owned())?;
            let (name, inline_value) = argument
                .split_once('=')
                .map_or((argument.as_str(), None), |(name, value)| {
                    (name, Some(value))
                });
            match name {
                "-h" | "--help" => return Ok(None),
                "--grpc-address" | "--server" => {
                    grpc_address = value(name, inline_value, &mut args)?;
                }
                "--heartbeat-seconds" => {
                    let raw = value(name, inline_value, &mut args)?;
                    heartbeat_seconds = parse_positive(Some(&raw), heartbeat_seconds, name)?;
                }
                "--buffer-capacity" => {
                    let raw = value(name, inline_value, &mut args)?;
                    buffer_capacity = raw
                        .parse::<usize>()
                        .ok()
                        .filter(|value| *value > 0)
                        .ok_or_else(|| format!("{name} must be a positive integer"))?;
                }
                _ => return Err(format!("unknown argument: {argument}")),
            }
        }
        Ok(Some(Self {
            grpc_address,
            heartbeat_seconds,
            buffer_capacity,
        }))
    }
}

fn value(
    name: &str,
    inline: Option<&str>,
    args: &mut impl Iterator<Item = OsString>,
) -> Result<String, String> {
    let value = match inline {
        Some(value) => value.to_owned(),
        None => args
            .next()
            .ok_or_else(|| format!("{name} requires a value"))?
            .into_string()
            .map_err(|_| format!("{name} value must be valid UTF-8"))?,
    };
    if value.is_empty() {
        return Err(format!("{name} requires a non-empty value"));
    }
    Ok(value)
}

fn parse_positive(value: Option<&str>, default: u64, name: &str) -> Result<u64, String> {
    value.map_or(Ok(default), |value| {
        value
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| format!("{name} must be a positive integer"))
    })
}

fn print_usage() {
    println!(
        "TimeTracker desktop client\n\n\
Usage: timetracker-client [OPTIONS]\n\n\
Options:\n\
  --grpc-address <URI>       gRPC server address [env: TIMETRACKER_SERVER]\n\
  --heartbeat-seconds <N>    heartbeat interval [env: TIMETRACKER_HEARTBEAT_SECONDS]\n\
  --buffer-capacity <N>      adapter event channel capacity [default: 100]\n\
  -h, --help                 print help"
    );
}

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let Some(config) = Config::from_env_and_args().map_err(std::io::Error::other)? else {
        print_usage();
        return Ok(());
    };
    let device_id = InstallationIdentity.load_or_create().await?;
    let upload_client = GrpcUploadClient::connect(config.grpc_address.clone()).await?;
    let queue = Arc::new(EventQueue::new());
    let event_service = Arc::new(ClientEventService::new(
        device_id,
        Arc::clone(&queue),
        upload_client,
    ));
    let (output_tx, output_rx) = mpsc::channel(config.buffer_capacity);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    #[cfg(target_os = "macos")]
    let mut supervisor = CollectionSupervisor::for_current_desktop();
    #[cfg(not(target_os = "macos"))]
    let mut supervisor = CollectionSupervisor::new(Vec::new());

    println!(
        "TimeTracker client {} sending events to {} (heartbeat every {} seconds)",
        device_id.0, config.grpc_address, config.heartbeat_seconds
    );

    let recorder = tokio::spawn({
        let event_service = Arc::clone(&event_service);
        async move { event_service.record_outputs(output_rx).await }
    });
    let uploader = tokio::spawn({
        let event_service = Arc::clone(&event_service);
        async move { event_service.upload_loop(shutdown_rx).await }
    });

    event_service
        .enqueue_kind(EventKind::CollectorStarted)
        .await?;
    supervisor.start(output_tx.clone()).await?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(config.heartbeat_seconds));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result?;
                break;
            }
            _ = heartbeat.tick() => {
                event_service.enqueue_kind(EventKind::Heartbeat).await?;
            }
        }
    }

    supervisor.stop().await?;
    drop(output_tx);
    recorder.await??;
    event_service
        .enqueue_kind(EventKind::CollectorStopped)
        .await?;

    let drain = async {
        while !queue.pending(1).await?.is_empty() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok::<_, ClientError>(())
    };
    if tokio::time::timeout(Duration::from_secs(5), drain)
        .await
        .is_err()
    {
        eprintln!("timed out while flushing queued events");
    }
    shutdown_tx.send_replace(true);
    uploader.await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Config, DEFAULT_BUFFER_CAPACITY};
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_grpc_address_and_runtime_options() {
        let config = Config::parse(
            args(&[
                "--grpc-address",
                "grpc.example.test:50051",
                "--heartbeat-seconds=7",
                "--buffer-capacity",
                "256",
            ]),
            "http://127.0.0.1:50051".into(),
            30,
        )
        .unwrap()
        .unwrap();

        assert_eq!(config.grpc_address, "grpc.example.test:50051");
        assert_eq!(config.heartbeat_seconds, 7);
        assert_eq!(config.buffer_capacity, 256);
    }

    #[test]
    fn uses_defaults_and_rejects_unknown_options() {
        let config = Config::parse(args(&[]), "server:50051".into(), 12)
            .unwrap()
            .unwrap();
        assert_eq!(config.grpc_address, "server:50051");
        assert_eq!(config.heartbeat_seconds, 12);
        assert_eq!(config.buffer_capacity, DEFAULT_BUFFER_CAPACITY);
        assert!(Config::parse(args(&["--wat"]), "server".into(), 12).is_err());
    }
}
