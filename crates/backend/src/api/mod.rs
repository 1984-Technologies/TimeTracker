pub mod auth;
use std::net::SocketAddr;

use async_trait::async_trait;
use tokio::sync::watch;

use crate::BackendError;

mod error;
mod grpc;
mod http;

pub use error::ApiError;
pub use grpc::GrpcMetricsServer;
pub use http::{HttpDashboardServer, dashboard_origin};

#[async_trait]
pub trait ApiServer: Send + Sync {
    async fn serve(
        &self,
        address: SocketAddr,
        shutdown: watch::Receiver<bool>,
    ) -> Result<(), BackendError>;
}
