use timetracker_model::DeviceId;

use crate::ClientError;
use std::{
    env,
    fs::OpenOptions,
    io::{self, Read, Write},
    path::PathBuf,
};
use uuid::Uuid;

pub struct InstallationIdentity;

impl InstallationIdentity {
    pub fn load_or_create(&self) -> Result<DeviceId, ClientError> {
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
                file.sync_all()?;
                Ok(DeviceId(id))
            }
            id => Ok(DeviceId(Uuid::parse_str(id)?)),
        }
    }
}

fn state_directory() -> Result<PathBuf, ClientError> {
    if let Some(path) = env::var_os("TIMETRACKER_STATE_DIR") {
        return Ok(path.into());
    }

    #[cfg(target_os = "windows")]
    if let Some(path) = env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(path).join("TimeTracker"));
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home).join("Library/Application Support/TimeTracker"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(path) = env::var_os("XDG_STATE_HOME") {
            return Ok(PathBuf::from(path).join("timetracker"));
        }
        if let Some(home) = env::var_os("HOME") {
            return Ok(PathBuf::from(home).join(".local/state/timetracker"));
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "cannot determine a directory for TimeTracker state",
    )
    .into())
}
