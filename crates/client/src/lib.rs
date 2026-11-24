pub mod adapters;
pub mod event_service;
pub mod identity;
pub mod queue;
pub mod registration;
pub mod registration_ui;
pub mod supervisor;
pub mod upload;

pub type ClientError = Box<dyn std::error::Error + Send + Sync>;
