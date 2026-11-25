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
        (
            status,
            Json(serde_json::json!({"code": code, "message": message})),
        )
            .into_response()
    }
}
