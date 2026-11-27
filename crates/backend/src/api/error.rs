use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{BackendError, auth::AuthError};

#[derive(Debug)]
pub enum ApiError {
    Backend(BackendError),
    BadRequest(String),
    Unauthorized,
    Forbidden,
    RateLimited,
    MethodNotAllowed,
}

impl From<BackendError> for ApiError {
    fn from(error: BackendError) -> Self {
        Self::Backend(error)
    }
}

impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::InvalidCredentials | AuthError::InvalidToken => Self::Unauthorized,
            AuthError::Forbidden => Self::Forbidden,
            AuthError::RateLimited => Self::RateLimited,
            AuthError::Backend(error) => Self::Backend(error),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Backend(BackendError::Validation(message)) | Self::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, "bad_request", message)
            }
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Invalid credentials or token".into(),
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden", "Access denied".into()),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many login attempts; retry in 60 seconds".into(),
            ),
            Self::MethodNotAllowed => (
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method not allowed".into(),
            ),
            Self::Backend(BackendError::NotFound(message)) => {
                (StatusCode::NOT_FOUND, "not_found", message)
            }
            Self::Backend(BackendError::Conflict(message)) => {
                (StatusCode::CONFLICT, "conflict", message)
            }
            Self::Backend(BackendError::Internal(_)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error".into(),
            ),
        };
        let mut response = (
            status,
            Json(serde_json::json!({"code": code, "message": message})),
        )
            .into_response();
        if status == StatusCode::TOO_MANY_REQUESTS {
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from_static("60"),
            );
        }
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                axum::http::header::WWW_AUTHENTICATE,
                axum::http::HeaderValue::from_static("Bearer"),
            );
        }
        response
    }
}

// Axum's default extractor rejections are plain text (and JSON errors use 422).
// Keep the API's documented JSON/400 contract without echoing request secrets.
pub(crate) struct ApiJson<T>(pub T);
pub(crate) struct ApiPath<T>(pub T);
pub(crate) struct ApiQuery<T>(pub T);

impl<S, T> axum::extract::FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;
    async fn from_request(request: axum::extract::Request, state: &S) -> Result<Self, ApiError> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(v)| Self(v))
            .map_err(|_| ApiError::BadRequest("Invalid JSON body".into()))
    }
}

impl<S, T> axum::extract::FromRequestParts<S> for ApiPath<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned + Send,
{
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, ApiError> {
        use axum::extract::Path;
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(v)| Self(v))
            .map_err(|_| ApiError::BadRequest("Invalid path parameter".into()))
    }
}

impl<S, T> axum::extract::FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, ApiError> {
        use axum::extract::Query;
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(v)| Self(v))
            .map_err(|_| ApiError::BadRequest("Invalid query parameters".into()))
    }
}
