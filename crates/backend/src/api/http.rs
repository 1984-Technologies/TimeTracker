use std::net::SocketAddr;
use std::sync::Arc;
use async_trait::async_trait;
use axum::http::StatusCode;
use axum::Router;
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::sync::watch::Receiver;
use crate::api::ApiServer;
use crate::BackendError;
use crate::dashboard::DashboardService;

pub struct HttpDashboardServer {
    pub(crate) dashboard: Arc<DashboardService>,
}

impl HttpDashboardServer {
    pub fn new(dashboard: Arc<DashboardService>) -> Self {
        Self { dashboard }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/health", get(health_check))
            .with_state(Arc::clone(&self.dashboard))
    }
}

async fn health_check() -> StatusCode {
    StatusCode::OK
}

#[async_trait]
impl ApiServer for HttpDashboardServer {
    async fn serve(&self, address: SocketAddr, mut shutdown: Receiver<bool>) -> Result<(), BackendError> {
        let app = self.router();

        let listener = TcpListener::bind(address).await?;

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                while !*shutdown.borrow_and_update() {
                    if shutdown.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::watch;
    use timetracker_model::UserId;
    use crate::dashboard::{TimeRange, UserAction, UserSummary};
    use crate::repositories::{UserActionRepository, UserRepository};

    struct DummyUserRepo;
    #[async_trait]
    impl UserRepository for DummyUserRepo {
        async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> { Ok(vec![]) }
        async fn find_user(&self, _id: UserId) -> Result<Option<UserSummary>, BackendError> { Ok(None) }
    }

    struct DummyActionRepo;
    #[async_trait]
    impl UserActionRepository for DummyActionRepo {
        async fn user_actions(&self, _id: UserId, _range: TimeRange) -> Result<Vec<UserAction>, BackendError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn http_server_responds_to_health_and_shuts_down_gracefully() {
        let dashboard = Arc::new(DashboardService::new(
            Arc::new(DummyUserRepo),
            Arc::new(DummyActionRepo),
        ));
        let server = HttpDashboardServer::new(dashboard);

        let temp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = temp_listener.local_addr().unwrap();
        drop(temp_listener);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let server_handle = tokio::spawn(async move {
            server.serve(addr, shutdown_rx).await
        });

        tokio::time::sleep(Duration::from_millis(20)).await;

        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();

        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "Expected 200 OK from /health, got: {response}"
        );

        shutdown_tx.send(true).unwrap();

        let join_res = tokio::time::timeout(Duration::from_millis(200), server_handle)
            .await
            .expect("Server did not shut down in time")
            .unwrap();

        assert!(join_res.is_ok(), "Server returned an error on shutdown");
    }
}