use std::net::SocketAddr;
use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::watch::Receiver;
use crate::api::ApiServer;
use crate::BackendError;
use crate::collection::MetricsCollectionService;

pub struct GrpcMetricsServer {
    pub(crate) collection: Arc<MetricsCollectionService>,
}

impl GrpcMetricsServer {
    pub fn new(collection: Arc<MetricsCollectionService>) -> Self {
        Self { collection }
    }
}

#[async_trait]
impl ApiServer for GrpcMetricsServer {
    async fn serve(&self, _address: SocketAddr, mut shutdown: Receiver<bool>) -> Result<(), BackendError> {
        while !*shutdown.borrow_and_update() {
            if shutdown.changed().await.is_err() {
                break;
            }
        }
        Ok(())
    }
}
