use timetracker_model::DeviceId;

use crate::ClientError;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::{env, io};
use uuid::Uuid;

pub struct InstallationIdentity;

impl InstallationIdentity {
    pub async fn load_or_create(&self) -> Result<DeviceId, ClientError> {
        let path = state_directory()?;
        std::fs::create_dir_all(&path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.join("device-id"))?;
        let mut id = String::new();
        file.read_to_string(&mut id)?;
        match id.trim() {
            "" => {
                let id = Uuid::new_v4();
                file.write_all(id.to_string().as_bytes())?;
                Ok(DeviceId(id))
            }
            id => Ok(DeviceId(Uuid::parse_str(id)?)),
        }
    }
}

fn state_directory() -> Result<PathBuf, ClientError> {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("timetracker"));
    }
    let home = env::var_os("HOME").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "neither XDG_STATE_HOME nor HOME is set",
        )
    })?;
    Ok(PathBuf::from(home).join(".local/state/timetracker"))
}
