use std::{future::Future, io, sync::Arc};

use tokio::{signal, sync::mpsc, sync::watch, task::JoinError};

use crate::{
    ClientError, adapters::DesktopAdapter, event_service::ClientEventService, queue::EventQueue,
    registration::ClientAuthService, supervisor::CollectionSupervisor, upload::GrpcUploadClient,
};

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

pub(super) async fn wait_for_shutdown_signal() -> Result<(), ClientError> {
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

pub(super) async fn run_collection<Shutdown, OnStopping>(
    adapters: Vec<Box<dyn DesktopAdapter>>,
    auth: Arc<ClientAuthService>,
    shutdown_signal: Shutdown,
    on_stopping: OnStopping,
) -> Result<(), ClientError>
where
    Shutdown: Future<Output = Result<(), ClientError>> + Send,
    OnStopping: FnOnce(),
{
    let event_service = Arc::new(ClientEventService::new(
        auth.device_id(),
        Arc::new(EventQueue::new()),
        GrpcUploadClient::new(grpc_server_uri())?,
    ));
    run_services(
        adapters,
        event_service,
        Some(auth),
        shutdown_signal,
        on_stopping,
    )
    .await
}

async fn run_services<Shutdown, OnStopping>(
    adapters: Vec<Box<dyn DesktopAdapter>>,
    event_service: Arc<ClientEventService>,
    auth: Option<Arc<ClientAuthService>>,
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
    let mut upload_task = tokio::spawn(async move {
        match auth {
            Some(auth) => {
                upload_service
                    .upload_loop_authenticated(upload_shutdown_rx, &auth)
                    .await
            }
            None => upload_service.upload_loop(upload_shutdown_rx).await,
        }
    });

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
