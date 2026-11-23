use timetracker_model::{
    DeviceId,
    auth::{DeviceCredentials, RegistrationResponse, TokenResponse},
};

use crate::ClientError;

pub struct DeviceCredentialStore;

impl DeviceCredentialStore {
    pub fn load_or_create(&self) -> Result<DeviceCredentials, ClientError> {
        todo!(
            "Preserve the installation UUID and securely load or create its random 32-byte secret."
        )
    }
}

pub struct HttpAuthClient {
    pub base_url: String,
}

impl HttpAuthClient {
    pub fn new(_base_url: String) -> Self {
        todo!()
    }
    pub async fn check_registration(
        &self,
        _credentials: &DeviceCredentials,
    ) -> Result<RegistrationResponse, ClientError> {
        todo!("Perform one registration check and return its result; the UI owns polling.")
    }
    pub async fn login(
        &self,
        _credentials: &DeviceCredentials,
    ) -> Result<TokenResponse, ClientError> {
        todo!("Exchange device credentials for a JWT through POST /api/devices/login.")
    }
}

pub struct ClientAuthService {
    pub http: HttpAuthClient,
    pub credentials: DeviceCredentials,
}

impl ClientAuthService {
    pub fn new(_http: HttpAuthClient, _credentials: DeviceCredentials) -> Self {
        todo!()
    }
    pub fn device_id(&self) -> DeviceId {
        todo!()
    }
    pub fn credential_hash(&self) -> Result<String, ClientError> {
        todo!(
            "Return the lowercase SHA-256 hex digest of the decoded device secret for enrollment."
        )
    }
    pub async fn check_registration(&self) -> Result<RegistrationResponse, ClientError> {
        todo!("Perform one registration check and return its result; the UI owns polling.")
    }
    pub async fn access_token(&self) -> Result<String, ClientError> {
        todo!("Reuse an unexpired JWT or renew it, coordinating concurrent callers.")
    }
    pub async fn invalidate_token(&self) -> Result<(), ClientError> {
        todo!("Discard the cached JWT so the next request authenticates again.")
    }
}

pub struct GrpcAuthInterceptor {
    pub token: String,
}

impl tonic::service::Interceptor for GrpcAuthInterceptor {
    fn call(&mut self, _request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        todo!("Attach Authorization: Bearer metadata using the supplied current token.")
    }
}
