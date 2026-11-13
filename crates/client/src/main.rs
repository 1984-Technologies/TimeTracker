use std::{future::Future, io, sync::Arc};

use tokio::{signal, sync::mpsc, sync::watch, task::JoinError};

use timetracker_client::{
    ClientError, adapters::DesktopAdapter, event_service::ClientEventService,
    identity::InstallationIdentity, queue::EventQueue, supervisor::CollectionSupervisor,
    upload::GrpcUploadClient,
};

#[cfg(target_os = "macos")]
use timetracker_client::adapters::default_adapters;

const ADAPTER_OUTPUT_BUFFER_CAPACITY: usize = 100;
const DEFAULT_GRPC_SERVER_ADDR: &str = "127.0.0.1:50051";

fn grpc_server_uri() -> String {
    let address =
        std::env::var("GRPC_SERVER_ADDR").unwrap_or_else(|_| DEFAULT_GRPC_SERVER_ADDR.to_owned());
    if address.contains("://") {
        address
    } else {
        format!("http://{address}")
    }
}

async fn wait_for_shutdown_signal() -> Result<(), ClientError> {
    #[cfg(unix)]
    {
        let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = signal::ctrl_c() => result?,
            _ = terminate.recv() => {}
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        signal::ctrl_c().await?;
        Ok(())
    }
}

fn joined_task_result(
    result: Result<Result<(), ClientError>, JoinError>,
) -> Result<(), ClientError> {
    match result {
        Ok(result) => result,
        Err(error) => Err(Box::new(error)),
    }
}

fn unexpected_task_exit(
    task_name: &str,
    result: Result<Result<(), ClientError>, JoinError>,
) -> ClientError {
    match result {
        Ok(Ok(())) => io::Error::other(format!("{task_name} stopped unexpectedly")).into(),
        Ok(Err(error)) => error,
        Err(error) => Box::new(error),
    }
}

fn remember_first_error(first_error: &mut Option<ClientError>, result: Result<(), ClientError>) {
    if first_error.is_none() {
        *first_error = result.err();
    }
}

#[cfg(any(target_os = "macos", test))]
async fn run_with_completion_hook<Operation, OnComplete>(
    operation: Operation,
    on_complete: OnComplete,
) -> Result<(), ClientError>
where
    Operation: Future<Output = Result<(), ClientError>>,
    OnComplete: FnOnce(),
{
    let result = operation.await;
    on_complete();
    result
}

async fn run_client<Shutdown, OnStopping>(
    adapters: Vec<Box<dyn DesktopAdapter>>,
    shutdown_signal: Shutdown,
    on_stopping: OnStopping,
) -> Result<(), ClientError>
where
    Shutdown: Future<Output = Result<(), ClientError>> + Send,
    OnStopping: FnOnce(),
{
    let device_id = InstallationIdentity.load_or_create()?;
    let queue = Arc::new(EventQueue::new());
    let upload_client = GrpcUploadClient::new(grpc_server_uri())?;
    let event_service = Arc::new(ClientEventService::new(device_id, queue, upload_client));

    run_client_services(adapters, event_service, shutdown_signal, on_stopping).await
}

async fn run_client_services<Shutdown, OnStopping>(
    adapters: Vec<Box<dyn DesktopAdapter>>,
    event_service: Arc<ClientEventService>,
    shutdown_signal: Shutdown,
    on_stopping: OnStopping,
) -> Result<(), ClientError>
where
    Shutdown: Future<Output = Result<(), ClientError>> + Send,
    OnStopping: FnOnce(),
{
    let (adapter_output_tx, adapter_output_rx) = mpsc::channel(ADAPTER_OUTPUT_BUFFER_CAPACITY);
    let (upload_shutdown_tx, upload_shutdown_rx) = watch::channel(false);

    // Consumers start before producers so adapter output can be handled immediately.
    let record_service = Arc::clone(&event_service);
    let mut record_task =
        tokio::spawn(async move { record_service.record_outputs(adapter_output_rx).await });

    let upload_service = Arc::clone(&event_service);
    let mut upload_task =
        tokio::spawn(async move { upload_service.upload_loop(upload_shutdown_rx).await });

    let supervisor = CollectionSupervisor::start(adapters, adapter_output_tx.clone()).await;

    tokio::pin!(shutdown_signal);
    let mut record_finished = false;
    let mut upload_finished = false;
    let mut first_error = None;

    tokio::select! {
        result = &mut shutdown_signal => remember_first_error(&mut first_error, result),
        result = &mut record_task => {
            record_finished = true;
            first_error = Some(unexpected_task_exit("adapter output recorder", result));
        }
        result = &mut upload_task => {
            upload_finished = true;
            first_error = Some(unexpected_task_exit("event uploader", result));
        }
    }

    // Stop producers first, then let the recorder drain and close its channel.
    on_stopping();
    remember_first_error(&mut first_error, supervisor.stop().await);
    drop(adapter_output_tx);

    if !record_finished {
        remember_first_error(&mut first_error, joined_task_result(record_task.await));
    }

    // The uploader is stopped last, after every adapter output has reached the queue.
    upload_shutdown_tx.send_replace(true);
    if !upload_finished {
        remember_first_error(&mut first_error, joined_task_result(upload_task.await));
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(not(target_os = "macos"))]
#[tokio::main]
async fn main() -> Result<(), ClientError> {
    let adapters: Vec<Box<dyn DesktopAdapter>> = Vec::new();
    run_client(adapters, wait_for_shutdown_signal(), || {}).await
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<(), ClientError> {
    use tokio::sync::oneshot;

    let (runtime, adapters) = default_adapters()?;
    let runtime_shutdown = runtime.shutdown_handle();
    let completion_runtime_shutdown = runtime_shutdown.clone();
    let (runtime_stopped_tx, mut runtime_stopped_rx) = oneshot::channel();

    let client = run_client(
        adapters,
        async move {
            tokio::select! {
                result = wait_for_shutdown_signal() => result,
                _ = &mut runtime_stopped_rx => Ok(()),
            }
        },
        move || {
            let _ = runtime_shutdown.request_shutdown();
        },
    );
    let client_task = tokio::spawn(run_with_completion_hook(client, move || {
        let _ = completion_runtime_shutdown.request_shutdown();
    }));

    // NSWorkspace requires its event loop to remain on the macOS main thread.
    runtime.run();
    let _ = runtime_stopped_tx.send(());

    joined_task_result(client_task.await)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use timetracker_model::DeviceId;
    use uuid::Uuid;

    use super::*;

    #[tokio::test]
    async fn client_tasks_stop_in_order_after_shutdown_is_requested() {
        let event_service = Arc::new(ClientEventService::new(
            DeviceId(Uuid::new_v4()),
            Arc::new(EventQueue::new()),
            GrpcUploadClient::new("http://127.0.0.1:50051".to_owned()).unwrap(),
        ));
        let stopping_hook_called = Arc::new(AtomicBool::new(false));
        let hook_state = Arc::clone(&stopping_hook_called);

        let result = tokio::time::timeout(
            Duration::from_millis(100),
            run_client_services(Vec::new(), event_service, async { Ok(()) }, move || {
                hook_state.store(true, Ordering::SeqCst);
            }),
        )
        .await
        .expect("client lifecycle did not finish after shutdown was requested");

        assert!(result.is_ok());
        assert!(stopping_hook_called.load(Ordering::SeqCst));
    }
}
