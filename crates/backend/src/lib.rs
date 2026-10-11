pub mod api;
pub mod collection;
pub mod dashboard;
pub mod repositories;

pub type BackendError = Box<dyn std::error::Error + Send + Sync>;
