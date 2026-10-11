pub mod adapters;
pub mod event_service;
pub mod identity;
pub mod queue;
pub mod supervisor;
pub mod upload;

pub type ClientError = Box<dyn std::error::Error + Send + Sync>;
