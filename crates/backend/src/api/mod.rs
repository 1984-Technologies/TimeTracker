use std::net::SocketAddr;

use async_trait::async_trait;
use tokio::sync::watch;

use crate::BackendError;

mod grpc;
mod http;

pub use grpc::GrpcMetricsServer;
pub use http::HttpDashboardServer;

#[async_trait]
pub trait ApiServer: Send + Sync {
    async fn serve(
        &self,
        address: SocketAddr,
        shutdown: watch::Receiver<bool>,
    ) -> Result<(), BackendError>;
}
