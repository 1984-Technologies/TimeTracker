use std::sync::Arc;

use crate::dashboard::DashboardService;

pub struct HttpDashboardServer {
    pub(crate) dashboard: Arc<DashboardService>,
}

impl HttpDashboardServer {
    pub fn new(dashboard: Arc<DashboardService>) -> Self {
        Self { dashboard }
    }
}
