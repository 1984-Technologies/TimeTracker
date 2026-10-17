use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::watch;
use timetracker_model::UserId;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::{
    api::ApiServer,
    dashboard::{DashboardService, TimeRange},
    BackendError,
};

pub struct HttpDashboardServer {
    pub(crate) dashboard: Arc<DashboardService>,
}

impl HttpDashboardServer {
    pub fn new(dashboard: Arc<DashboardService>) -> Self {
        Self { dashboard }
    }
}

#[derive(Debug, Deserialize)]
struct ActionsQuery {
    start: OffsetDateTime,
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
            .with_graceful_shutdown(async move {
                if shutdown.changed().await.is_err() {
                    // sender dropped, shutdown
                }
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::{UserActionRepository, UserRepository};
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
