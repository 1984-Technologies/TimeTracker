use std::{env, time::Duration};

use time::OffsetDateTime;
use timetracker_client::{ClientError, identity::InstallationIdentity, upload::GrpcUploadClient};
use timetracker_model::{Event, EventId, EventKind};
use uuid::Uuid;

const DEFAULT_SERVER: &str = "http://127.0.0.1:50051";
const DEFAULT_HEARTBEAT_SECONDS: u64 = 30;

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let server = env::var("TIMETRACKER_SERVER").unwrap_or_else(|_| DEFAULT_SERVER.to_owned());
    let heartbeat_seconds = env::var("TIMETRACKER_HEARTBEAT_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_HEARTBEAT_SECONDS);
    let device_id = InstallationIdentity.load_or_create().await?;

    println!(
        "TimeTracker client {} sending heartbeats to {} every {} seconds",
        device_id.0, server, heartbeat_seconds
    );

    let mut first_event = true;
    let mut client = None;
    loop {
        let kind = if first_event {
            EventKind::CollectorStarted
        } else {
            EventKind::Heartbeat
        };
        let event = Event {
            id: EventId(Uuid::new_v4()),
            device_id,
            occurred_at: OffsetDateTime::now_utc(),
            kind,
        };

        loop {
            if client.is_none() {
                match GrpcUploadClient::connect(server.clone()).await {
                    Ok(connected) => client = Some(connected),
                    Err(error) => {
                        eprintln!("server connection failed: {error}; retrying in 3 seconds");
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                }
            }

            let upload = client
                .as_mut()
                .expect("client is initialized immediately above")
                .send(device_id, std::slice::from_ref(&event))
                .await;
            match upload {
                Ok(accepted) if accepted.contains(&event.id) => break,
                Ok(_) => eprintln!("server did not acknowledge the heartbeat; retrying"),
                Err(error) => eprintln!("heartbeat upload failed: {error}; reconnecting"),
            }
            client = None;
            tokio::time::sleep(Duration::from_secs(3)).await;
        }

        first_event = false;
        tokio::time::sleep(Duration::from_secs(heartbeat_seconds)).await;
    }
}
