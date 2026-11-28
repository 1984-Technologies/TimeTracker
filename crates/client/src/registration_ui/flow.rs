use crate::ClientError;
use serde::Serialize;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum Status {
    Checking,
    Unregistered,
    #[cfg(feature = "desktop-ui")]
    Registered,
    ConnectionError {
        retry_seconds: u64,
    },
    #[cfg(feature = "desktop-ui")]
    Stopping,
    #[cfg(feature = "desktop-ui")]
    Stopped {
        failed: bool,
    },
}

// The check includes login, so approval alone can never start collection.
pub(super) async fn await_registration<Check, CheckFuture>(
    mut check: Check,
    status: &watch::Sender<Status>,
    retry: &mut watch::Receiver<u64>,
    shutdown: &mut watch::Receiver<bool>,
) -> bool
where
    Check: FnMut() -> CheckFuture,
    CheckFuture: std::future::Future<Output = Result<bool, ClientError>>,
{
    let mut backoff = 5;
    loop {
        retry.borrow_and_update();
        status.send_replace(Status::Checking);
        let result = tokio::select! {
            biased;
            _ = shutdown.wait_for(|stopping| *stopping) => return false,
            result = tokio::time::timeout(Duration::from_secs(15), check()) => result,
        };
        let delay = match result {
            Ok(Ok(true)) => return true,
            Ok(Ok(false)) => {
                backoff = 5;
                status.send_replace(Status::Unregistered);
                5
            }
            _ => {
                status.send_replace(Status::ConnectionError {
                    retry_seconds: backoff,
                });
                let delay = backoff;
                backoff = (backoff * 2).min(60);
                delay
            }
        };
        tokio::select! {
            biased;
            _ = shutdown.wait_for(|stopping| *stopping) => return false,
            changed = retry.changed() => {
                if changed.is_err() { return false; }
                backoff = 5;
            }
            _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
        }
    }
}
