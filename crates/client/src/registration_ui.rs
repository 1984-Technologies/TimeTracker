use std::sync::Arc;

use crate::{ClientError, registration::ClientAuthService};

pub struct RegistrationUiState {
    pub auth: Arc<ClientAuthService>,
    pub frontend_url: String,
}

impl RegistrationUiState {
    pub fn enrollment_url(&self) -> Result<String, ClientError> {
        todo!("Build the frontend registration URL with the device ID and hash in its fragment.")
    }
}
