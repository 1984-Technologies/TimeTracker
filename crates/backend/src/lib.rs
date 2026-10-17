pub mod api;
pub mod collection;
pub mod dashboard;
pub mod repositories;

#[derive(Debug)]
pub enum BackendError {
    Validation(String),
    NotFound(String),
    Internal(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Validation(msg) => write!(f, "Validation error: {}", msg),
            BackendError::NotFound(msg) => write!(f, "Not found: {}", msg),
            BackendError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for BackendError {}
