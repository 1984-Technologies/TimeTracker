pub mod api;
pub mod auth;
pub mod collection;
pub mod dashboard;
pub mod repositories;

#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzz_support;

#[cfg(test)]
pub(crate) mod test_support;

#[derive(Debug)]
pub enum BackendError {
    Validation(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Validation(msg) => write!(f, "Validation error: {}", msg),
            BackendError::NotFound(msg) => write!(f, "Not found: {}", msg),
            BackendError::Conflict(msg) => write!(f, "Conflict: {}", msg),
            BackendError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for BackendError {}

impl From<std::io::Error> for BackendError {
    fn from(error: std::io::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<sqlx::Error> for BackendError {
    fn from(error: sqlx::Error) -> Self {
        if error
            .as_database_error()
            .is_some_and(|error| error.is_unique_violation())
        {
            Self::Conflict("record already exists".into())
        } else {
            Self::Internal(error.to_string())
        }
    }
}
