use std::{io, sync::Arc};

use timetracker_model::DeviceId;
use url::Url;

use crate::{ClientError, registration::ClientAuthService};

#[cfg(feature = "desktop-ui")]
mod collection;
#[cfg(feature = "desktop-ui")]
pub mod desktop;
#[cfg(any(feature = "desktop-ui", test))]
mod flow;

pub struct RegistrationUiState {
    pub auth: Arc<ClientAuthService>,
    pub frontend_url: String,
}

impl RegistrationUiState {
    pub fn enrollment_url(&self) -> Result<String, ClientError> {
        enrollment_url(
            &self.frontend_url,
            self.auth.device_id(),
            &self.auth.credential_hash()?,
        )
    }
}

fn enrollment_url(base: &str, device_id: DeviceId, hash: &str) -> Result<String, ClientError> {
    let mut url = Url::parse(base)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid enrollment URL or credential hash",
        )
        .into());
    }
    url.set_path(&format!(
        "{}/devices/register",
        url.path().trim_end_matches('/')
    ));
    url.set_fragment(Some(&format!(
        "device_id={}&credential_hash={hash}",
        device_id.0
    )));
    Ok(url.into())
}
