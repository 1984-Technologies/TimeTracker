use timetracker_model::DeviceId;

use crate::ClientError;
use std::env;
use std::fs::OpenOptions;
use std::path::Path;
use std::io::Read;
use std::io::Write;
use uuid::Uuid;

pub struct InstallationIdentity;

impl InstallationIdentity {
    pub async fn load_or_create(&self) -> Result<DeviceId, ClientError> {
        let home = env::var("HOME").expect("$HOME variable must be set");
        let path = Path::new(&home).join(".local/state/timetracker");
        std::fs::create_dir_all(&path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path.join("device-id"))?;
        let mut id = String::new();
        file.read_to_string(&mut id)?;
        match id.as_str() {
            "" => {
                let id = Uuid::new_v4();
                file.write_all(id.to_string().as_bytes())?;
                Ok(DeviceId(id))
            },
            id => Ok(DeviceId(Uuid::parse_str(id)?))
        }
    }
}
