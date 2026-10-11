use std::sync::Arc;

use crate::collection::MetricsCollectionService;

pub struct GrpcMetricsServer {
    pub(crate) collection: Arc<MetricsCollectionService>,
}

impl GrpcMetricsServer {
    pub fn new(collection: Arc<MetricsCollectionService>) -> Self {
        Self { collection }
    }
}
