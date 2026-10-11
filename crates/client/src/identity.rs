use timetracker_model::DeviceId;

use crate::ClientError;

pub struct InstallationIdentity;

impl InstallationIdentity {
    pub async fn load_or_create(&self) -> Result<DeviceId, ClientError> {
        todo!()
    }
}
