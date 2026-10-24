use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};

use tokio::sync::watch;

use timetracker_backend::{
    BackendError,
    api::{ApiServer, GrpcMetricsServer, HttpDashboardServer},
    collection::MetricsCollectionService,
    dashboard::DashboardService,
    repositories::{
        DeviceRepository, EventRepository, InMemoryRepository, UserActionRepository, UserRepository,
    },
};

fn socket_addr(name: &str, default: &str) -> Result<SocketAddr, BackendError> {
    let value = env::var(name).unwrap_or_else(|_| default.to_owned());
    value.parse().map_err(|error| {
        BackendError::Validation(format!(
            "invalid socket address in {name} ('{value}'): {error}"
        ))
    })
}

async fn wait_for_shutdown_signal(shutdown_tx: watch::Sender<bool>) {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            eprintln!("failed to listen for Ctrl+C: {error}");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => eprintln!("failed to listen for SIGTERM: {error}"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    let _ = shutdown_tx.send(true);
}

#[tokio::main]
async fn main() -> Result<(), BackendError> {
    let grpc_bind_addr = socket_addr("GRPC_BIND_ADDR", "0.0.0.0:50051")?;
    let http_bind_addr = socket_addr("HTTP_BIND_ADDR", "0.0.0.0:8080")?;

    let repository = Arc::new(InMemoryRepository::new());
    let devices: Arc<dyn DeviceRepository> = repository.clone();
    let events: Arc<dyn EventRepository> = repository.clone();
    let users: Arc<dyn UserRepository> = repository.clone();
    let actions: Arc<dyn UserActionRepository> = repository;

    let grpc_server =
        GrpcMetricsServer::new(Arc::new(MetricsCollectionService::new(devices, events)));
    let static_dir =
        PathBuf::from(env::var("STATIC_DIR").unwrap_or_else(|_| "/app/frontend".into()));
    let mut http_server = HttpDashboardServer::new(Arc::new(DashboardService::new(users, actions)));
    if static_dir.is_dir() {
        http_server = http_server.with_static_dir(static_dir);
    }
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(wait_for_shutdown_signal(shutdown_tx.clone()));

    println!("dashboard listening on http://{http_bind_addr}");
    println!("client ingestion listening on {grpc_bind_addr}");

    let result = tokio::try_join!(
        grpc_server.serve(grpc_bind_addr, shutdown_rx.clone()),
        http_server.serve(http_bind_addr, shutdown_rx),
    );
    let _ = shutdown_tx.send(true);
    result?;
    Ok(())
}
