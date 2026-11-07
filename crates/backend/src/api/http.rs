use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use timetracker_model::UserId;
use tokio::{
    net::TcpListener,
    sync::{watch, watch::Receiver},
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{
    BackendError,
    api::ApiServer,
    dashboard::{DashboardService, TimeRange},
};

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
    async fn serve(
        &self,
        address: SocketAddr,
        mut shutdown: Receiver<bool>,
    ) -> Result<(), BackendError> {
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
    use crate::dashboard::{TimeRange, UserAction, UserSummary};
    use crate::repositories::{UserActionRepository, UserRepository};
    use std::time::Duration;
    use timetracker_model::UserId;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::watch;

    struct DummyUserRepo;
    #[async_trait]
    impl UserRepository for DummyUserRepo {
        async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> {
            Ok(vec![])
        }
        async fn find_user(&self, _id: UserId) -> Result<Option<UserSummary>, BackendError> {
            Ok(None)
        }
    }

    struct DummyActionRepo;
    #[async_trait]
    impl UserActionRepository for DummyActionRepo {
        async fn user_actions(
            &self,
            _id: UserId,
            _range: TimeRange,
        ) -> Result<Vec<UserAction>, BackendError> {
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

        let server_handle = tokio::spawn(async move { server.serve(addr, shutdown_rx).await });

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

#[derive(Debug, Deserialize)]
struct ActionsQuery {
    #[serde(with = "time::serde::rfc3339")]
    start: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    end: OffsetDateTime,
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
}

fn default_limit() -> u32 {
    100
}

const MAX_LIMIT: u32 = 1000;

#[derive(Debug, Serialize)]
struct PaginatedResponse<T> {
    items: Vec<T>,
    total: u32,
    limit: u32,
    offset: u32,
}

enum HttpError {
    Backend(BackendError),
    BadRequest(String),
}

impl IntoResponse for HttpError {
    fn into_response(self) -> axum::response::Response {
        match self {
            HttpError::Backend(e) => {
                let status = match &e {
                    BackendError::Validation(_) => StatusCode::BAD_REQUEST,
                    BackendError::NotFound(_) => StatusCode::NOT_FOUND,
                    BackendError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, e.to_string()).into_response()
            }
            HttpError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, msg).into_response()
            }
        }
    }
}

impl From<BackendError> for HttpError {
    fn from(e: BackendError) -> Self {
        HttpError::Backend(e)
    }
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow_and_update() {
            return;
        }
        if shutdown.changed().await.is_err() {
            return;
        }
    }
}

async fn list_users(
    State(dashboard): State<Arc<DashboardService>>,
) -> Result<impl IntoResponse, HttpError> {
    let users = dashboard.list_users().await?;
    Ok(Json(users))
}

async fn user_actions(
    State(dashboard): State<Arc<DashboardService>>,
    Path(user_id): Path<UserId>,
    Query(query): Query<ActionsQuery>,
) -> Result<impl IntoResponse, HttpError> {
    if query.limit == 0 || query.limit > MAX_LIMIT {
        return Err(HttpError::BadRequest(format!(
            "limit must be between 1 and {}",
            MAX_LIMIT
        )));
    }

    let range = TimeRange {
        start: query.start,
        end: query.end,
    };

    let actions = dashboard.user_actions(user_id, range).await?;

    let total = actions.len() as u32;
    let paginated: Vec<_> = actions
        .into_iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .collect();

    Ok(Json(PaginatedResponse {
        items: paginated,
        total,
        limit: query.limit,
        offset: query.offset,
    }))
}

#[async_trait::async_trait]
impl ApiServer for HttpDashboardServer {
    async fn serve(
        &self,
        address: SocketAddr,
        shutdown: watch::Receiver<bool>,
    ) -> Result<(), BackendError> {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = Router::new()
            .route("/users", get(list_users))
            .route("/users/{id}/actions", get(user_actions))
            .layer(TraceLayer::new_for_http())
            .layer(cors)
            .with_state(self.dashboard.clone());

        let listener = tokio::net::TcpListener::bind(address).await?;

        axum::serve(listener, app)
            .with_graceful_shutdown(wait_for_shutdown(shutdown))
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::{UserActionRepository, UserRepository};
    use crate::dashboard::{UserAction, UserSummary};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    use uuid::Uuid;

    struct MockUserRepo;
    struct MockActionRepo;

    #[async_trait::async_trait]
    impl UserRepository for MockUserRepo {
        async fn list_users(&self) -> Result<Vec<UserSummary>, BackendError> {
            Ok(vec![UserSummary {
                id: UserId(Uuid::new_v4()),
                name: "Test User".to_string(),
            }])
        }

        async fn find_user(&self, _user_id: UserId) -> Result<Option<UserSummary>, BackendError> {
            Ok(None)
        }
    }

    #[async_trait::async_trait]
    impl UserActionRepository for MockActionRepo {
        async fn user_actions(
            &self,
            _user_id: UserId,
            _range: TimeRange,
        ) -> Result<Vec<UserAction>, BackendError> {
            Ok(vec![])
        }

        async fn preceding_actions(
            &self,
            _user_id: UserId,
            _before: OffsetDateTime,
        ) -> Result<Vec<UserAction>, BackendError> {
            Ok(vec![])
        }
    }

    fn create_test_dashboard() -> Arc<DashboardService> {
        Arc::new(DashboardService::new(
            Arc::new(MockUserRepo),
            Arc::new(MockActionRepo),
        ))
    }

    #[tokio::test]
    async fn shutdown_respects_initial_true_value() {
        let (_sender, receiver) = watch::channel(true);
        tokio::time::timeout(std::time::Duration::from_secs(1), wait_for_shutdown(receiver))
            .await
            .expect("an already requested shutdown must finish immediately");
    }

    #[tokio::test]
    async fn shutdown_ignores_false_updates() {
        let (sender, receiver) = watch::channel(false);
        let shutdown = wait_for_shutdown(receiver);
        tokio::pin!(shutdown);
        sender.send(false).unwrap();
        assert!(tokio::time::timeout(std::time::Duration::from_millis(20), &mut shutdown)
            .await
            .is_err());
        sender.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), shutdown)
            .await
            .expect("a true update must stop the server");
    }

    #[tokio::test]
    async fn shutdown_finishes_when_sender_is_dropped() {
        let (sender, receiver) = watch::channel(false);
        drop(sender);
        tokio::time::timeout(std::time::Duration::from_secs(1), wait_for_shutdown(receiver))
            .await
            .expect("a dropped sender must stop the server");
    }

    #[tokio::test]
    async fn test_list_users_returns_200() {
        let app = Router::new()
            .route("/users", get(list_users))
            .with_state(create_test_dashboard());

        let response = app
            .oneshot(Request::builder().uri("/users").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_user_actions_returns_200() {
        let app = Router::new()
            .route("/users/{id}/actions", get(user_actions))
            .with_state(create_test_dashboard());

        let start = OffsetDateTime::from_unix_timestamp(100).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(200).unwrap();
        let uri = format!(
            "/users/{}/actions?start={}&end={}",
            Uuid::new_v4(),
            start.format(&time::format_description::well_known::Rfc3339).unwrap(),
            end.format(&time::format_description::well_known::Rfc3339).unwrap(),
        );

        let response = app
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_user_actions_rejects_invalid_range() {
        let app = Router::new()
            .route("/users/{id}/actions", get(user_actions))
            .with_state(create_test_dashboard());

        let start = OffsetDateTime::from_unix_timestamp(200).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(100).unwrap();
        let uri = format!(
            "/users/{}/actions?start={}&end={}",
            Uuid::new_v4(),
            start.format(&time::format_description::well_known::Rfc3339).unwrap(),
            end.format(&time::format_description::well_known::Rfc3339).unwrap(),
        );

        let response = app
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_user_actions_rejects_zero_limit() {
        let app = Router::new()
            .route("/users/{id}/actions", get(user_actions))
            .with_state(create_test_dashboard());

        let start = OffsetDateTime::from_unix_timestamp(100).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(200).unwrap();
        let uri = format!(
            "/users/{}/actions?start={}&end={}&limit=0",
            Uuid::new_v4(),
            start.format(&time::format_description::well_known::Rfc3339).unwrap(),
            end.format(&time::format_description::well_known::Rfc3339).unwrap(),
        );

        let response = app
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_user_actions_rejects_limit_too_large() {
        let app = Router::new()
            .route("/users/{id}/actions", get(user_actions))
            .with_state(create_test_dashboard());

        let start = OffsetDateTime::from_unix_timestamp(100).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(200).unwrap();
        let uri = format!(
            "/users/{}/actions?start={}&end={}&limit=1001",
            Uuid::new_v4(),
            start.format(&time::format_description::well_known::Rfc3339).unwrap(),
            end.format(&time::format_description::well_known::Rfc3339).unwrap(),
        );

        let response = app
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_mapping_validation() {
        let err = HttpError::Backend(BackendError::Validation("test".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_mapping_not_found() {
        let err = HttpError::Backend(BackendError::NotFound("test".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_error_mapping_internal() {
        let err = HttpError::Backend(BackendError::Internal("test".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_error_mapping_bad_request() {
        let err = HttpError::BadRequest("test".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_default_limit() {
        assert_eq!(default_limit(), 100);
    }

    #[test]
    fn test_max_limit() {
        assert_eq!(MAX_LIMIT, 1000);
    }
}
