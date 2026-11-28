use super::ApiError;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware,
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use timetracker_model::UserId;
use tokio::sync::watch;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};

use crate::{
    BackendError,
    api::ApiServer,
    dashboard::{DashboardService, TimeRange},
};

pub struct HttpDashboardServer {
    pub(crate) dashboard: Arc<DashboardService>,
    auth: Option<Arc<crate::auth::AuthService>>,
}

impl HttpDashboardServer {
    pub fn new(dashboard: Arc<DashboardService>) -> Self {
        Self {
            dashboard,
            auth: None,
        }
    }
    pub fn with_auth(mut self, auth: Arc<crate::auth::AuthService>) -> Self {
        self.auth = Some(auth);
        self
    }
    fn private_router(&self) -> Router {
        Router::new()
            .route("/users", get(list_users))
            .route("/users/{id}/actions", get(user_actions))
            .with_state(self.dashboard.clone())
    }
    pub fn router(&self) -> Router {
        let reads = self.private_router();
        let reads = match &self.auth {
            Some(auth) => reads.route_layer(middleware::from_fn_with_state(
                auth.clone(),
                super::auth::authenticate_http,
            )),
            None => reads.route_layer(middleware::from_fn(
                |_request: axum::extract::Request, _next: middleware::Next| async {
                    ApiError::Unauthorized.into_response()
                },
            )),
        };
        let mut app = Router::new()
            .route("/health", get(health_check))
            .merge(reads);
        if let Some(auth) = &self.auth {
            app = app.merge(
                super::auth::HttpAuthApi::new(auth.clone(), self.dashboard._users.clone()).router(),
            );
        }
        let cors = dashboard_origin()
            .map(|origin| {
                CorsLayer::new()
                    .allow_origin(AllowOrigin::list([origin]))
                    .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::OPTIONS])
                    .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
            })
            .unwrap_or_else(|_| CorsLayer::new());
        app.layer(TraceLayer::new_for_http()).layer(cors)
    }
}

pub fn dashboard_origin() -> Result<HeaderValue, BackendError> {
    let invalid = || {
        BackendError::Validation(
            "DASHBOARD_ORIGIN must be one HTTP(S) origin, for example http://localhost:5173".into(),
        )
    };
    let value =
        std::env::var("DASHBOARD_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".into());
    let url = url::Url::parse(value.trim()).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    url.origin()
        .ascii_serialization()
        .parse()
        .map_err(|_| invalid())
}

async fn health_check() -> StatusCode {
    StatusCode::OK
}

#[cfg(test)]
mod health_tests {
    use super::*;
    use crate::dashboard::{TimeRange, UserAction, UserSummary};
    use crate::repositories::{UserActionRepository, UserRepository};
    use async_trait::async_trait;
    use std::time::Duration;
    use timetracker_model::UserId;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::net::TcpStream;
    use tokio::sync::watch;

    struct DummyUserRepo;
    #[async_trait]
    impl UserRepository for DummyUserRepo {
        async fn create_user(&self, _name: &str) -> Result<UserSummary, BackendError> {
            todo!()
        }

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
        async fn preceding_actions(
            &self,
            _id: UserId,
            _before: OffsetDateTime,
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
) -> Result<impl IntoResponse, ApiError> {
    let users = dashboard.list_users().await?;
    Ok(Json(users))
}

async fn user_actions(
    State(dashboard): State<Arc<DashboardService>>,
    Path(user_id): Path<UserId>,
    Query(query): Query<ActionsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if query.limit == 0 || query.limit > MAX_LIMIT {
        return Err(ApiError::BadRequest(format!(
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
        let app = self.router();

        let listener = tokio::net::TcpListener::bind(address).await?;

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(wait_for_shutdown(shutdown))
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::{UserAction, UserSummary};
    use crate::repositories::{UserActionRepository, UserRepository};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    use uuid::Uuid;

    struct MockUserRepo;
    struct MockActionRepo;

    #[async_trait::async_trait]
    impl UserRepository for MockUserRepo {
        async fn create_user(&self, _name: &str) -> Result<UserSummary, BackendError> {
            todo!()
        }

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
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            wait_for_shutdown(receiver),
        )
        .await
        .expect("an already requested shutdown must finish immediately");
    }

    #[tokio::test]
    async fn shutdown_ignores_false_updates() {
        let (sender, receiver) = watch::channel(false);
        let shutdown = wait_for_shutdown(receiver);
        tokio::pin!(shutdown);
        sender.send(false).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut shutdown)
                .await
                .is_err()
        );
        sender.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), shutdown)
            .await
            .expect("a true update must stop the server");
    }

    #[tokio::test]
    async fn shutdown_finishes_when_sender_is_dropped() {
        let (sender, receiver) = watch::channel(false);
        drop(sender);
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            wait_for_shutdown(receiver),
        )
        .await
        .expect("a dropped sender must stop the server");
    }

    #[tokio::test]
    async fn test_list_users_returns_200() {
        let app = HttpDashboardServer::new(create_test_dashboard()).private_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_user_actions_returns_200() {
        let app = HttpDashboardServer::new(create_test_dashboard()).private_router();

        let start = OffsetDateTime::from_unix_timestamp(100).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(200).unwrap();
        let uri = format!(
            "/users/{}/actions?start={}&end={}",
            Uuid::new_v4(),
            start
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            end.format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
        );

        let response = app
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_user_actions_rejects_invalid_range() {
        let app = HttpDashboardServer::new(create_test_dashboard()).private_router();

        let start = OffsetDateTime::from_unix_timestamp(200).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(100).unwrap();
        let uri = format!(
            "/users/{}/actions?start={}&end={}",
            Uuid::new_v4(),
            start
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            end.format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
        );

        let response = app
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_user_actions_rejects_zero_limit() {
        let app = HttpDashboardServer::new(create_test_dashboard()).private_router();

        let start = OffsetDateTime::from_unix_timestamp(100).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(200).unwrap();
        let uri = format!(
            "/users/{}/actions?start={}&end={}&limit=0",
            Uuid::new_v4(),
            start
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            end.format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
        );

        let response = app
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_user_actions_rejects_limit_too_large() {
        let app = HttpDashboardServer::new(create_test_dashboard()).private_router();

        let start = OffsetDateTime::from_unix_timestamp(100).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(200).unwrap();
        let uri = format!(
            "/users/{}/actions?start={}&end={}&limit=1001",
            Uuid::new_v4(),
            start
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            end.format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
        );

        let response = app
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_mapping_validation() {
        let err = ApiError::Backend(BackendError::Validation("test".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_mapping_not_found() {
        let err = ApiError::Backend(BackendError::NotFound("test".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_error_mapping_internal() {
        let err = ApiError::Backend(BackendError::Internal("test".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_error_mapping_bad_request() {
        let err = ApiError::BadRequest("test".to_string());
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
