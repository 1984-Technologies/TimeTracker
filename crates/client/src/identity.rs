use timetracker_model::DeviceId;

use crate::ClientError;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub struct InstallationIdentity;

impl InstallationIdentity {
    pub fn load_or_create(&self) -> Result<DeviceId, ClientError> {
        let state_directory = state_directory()?;
        load_or_create_at(&state_directory)
    }
}

fn load_or_create_at(
    state_directory: &Path,
) -> Result<DeviceId, ClientError> {
    fs::create_dir_all(state_directory)?;
    let device_id_path = state_directory.join("device-id");

    if let Some(device_id) = read_device_id(&device_id_path)? {
        return Ok(device_id);
    }

    let device_id = DeviceId(Uuid::new_v4());
    write_device_id(&device_id_path, device_id)?;
    Ok(device_id)
}

fn read_device_id(path: &Path) -> Result<Option<DeviceId>, ClientError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let contents = contents.trim();
    if contents.is_empty() {
        return Ok(None);
    }
    Ok(Some(DeviceId(Uuid::parse_str(contents)?)))
}

fn write_device_id(path: &Path, device_id: DeviceId) -> Result<(), ClientError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(device_id.0.to_string().as_bytes())?;
    file.sync_all()?;
    Ok(())
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
