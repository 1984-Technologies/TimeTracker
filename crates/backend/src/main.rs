use sqlx::postgres::PgPoolOptions;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::watch;

use timetracker_backend::{
    BackendError,
    api::{ApiServer, GrpcMetricsServer, HttpDashboardServer},
    collection::MetricsCollectionService,
    dashboard::DashboardService,
    repositories::{
        DeviceRepository, EventRepository, PostgresDeviceRepository, PostgresEventRepository,
        PostgresUserActionRepository, PostgresUserRepository, UserActionRepository, UserRepository,
    },
};

fn read_env_var(name: &str) -> Result<String, BackendError> {
    env::var(name).map_err(|e| format!("Failed to read enviroment variable {name}: {e}").into())
}

fn read_socket_addr(name: &str) -> Result<SocketAddr, BackendError> {
    let var = read_env_var(name)?;
    var.parse::<SocketAddr>().map_err(|e| format!("Invalid socket address in {name} ('{var}'): {e}").into())
}

async fn wait_for_shutdown_signal(shutdown_tx: watch::Sender<bool>) {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            eprintln!("Failed to listen for Ctrl+C signal: {e}");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => eprintln!("Failed to listen for SIGTERM signal: {e}"),
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
    let database_url = read_env_var("DATABASE_URL")?;
    let grpc_bind_addr = read_socket_addr("GRPC_BIND_ADDR")?;
    let http_bind_addr = read_socket_addr("HTTP_BIND_ADDR")?;

    let _db_pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .map_err(|e| format!("Failed to connect to PostgreSQL: {e}"))?;

    #[allow(unreachable_code)]
    let (device_repository, event_repository, user_repository, user_action_repository) = {
        let device_repo: PostgresDeviceRepository = todo!("Wait for backend-postgres constructors");
        let event_repo: PostgresEventRepository = todo!("Wait for backend-postgres constructors");
        let user_repo: PostgresUserRepository = todo!("Wait for backend-postgres constructors");
        let action_repo: PostgresUserActionRepository = todo!("Wait for backend-postgres constructors");

        let device_repo: Arc<dyn DeviceRepository> = Arc::new(device_repo);
        let event_repo: Arc<dyn EventRepository> = Arc::new(event_repo);
        let user_repo: Arc<dyn UserRepository> = Arc::new(user_repo);
        let action_repo: Arc<dyn UserActionRepository> = Arc::new(action_repo);

        (device_repo, event_repo, user_repo, action_repo)
    };

    let collection_service = Arc::new(MetricsCollectionService::new(
        device_repository,
        event_repository,
    ));
    let dashboard_service = Arc::new(DashboardService::new(
        user_repository,
        user_action_repository,
    ));

    let grpc_server = GrpcMetricsServer::new(collection_service);
    let http_server = HttpDashboardServer::new(dashboard_service);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(wait_for_shutdown_signal(shutdown_tx.clone()));

    let grpc_future = grpc_server.serve(grpc_bind_addr, shutdown_rx.clone());
    let http_future = http_server.serve(http_bind_addr, shutdown_rx);

    let result = tokio::try_join!(grpc_future, http_future);

    let _ = shutdown_tx.send(true);
    result?;

    Ok(())
}
