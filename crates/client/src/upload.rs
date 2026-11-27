use std::{io, time::Duration};

use timetracker_model::{AdapterStatus, DeviceId, Event, EventId, EventKind, ObservationKind};
use timetracker_proto::metrics::{
    ActiveBrowserTabChangedMessage, ActiveBrowserTabTitleChangedMessage, AdapterStatusMessage,
    BrowserWindowFocusChangedMessage, EventMessage, FocusedApplicationChangedMessage,
    KeyboardButtonPressedMessage, MouseClickedMessage, MouseMovedMessage, ObservationTypeProto,
    UploadEventsRequest, UploadEventsResponse, adapter_status_message,
    client_metrics_uploader_client::ClientMetricsUploaderClient, event_message,
};
use tonic::{
    Code,
    transport::{Channel, Endpoint},
};
use uuid::Uuid;

use crate::{
    ClientError,
    registration::{ClientAuthService, GrpcAuthInterceptor},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct GrpcUploadClient {
    channel: Channel,
    client: ClientMetricsUploaderClient<Channel>,
}

impl GrpcUploadClient {
    pub fn new(address: String) -> Result<Self, ClientError> {
        let channel = Endpoint::from_shared(address)?
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .connect_lazy();

        Ok(Self {
            client: ClientMetricsUploaderClient::new(channel.clone()),
            channel,
        })
    }

    pub async fn send_authenticated(
        &self,
        auth: &ClientAuthService,
        events: &[Event],
    ) -> Result<Vec<EventId>, ClientError> {
        let request = build_upload_request(auth.device_id(), events)?;

        let token = auth.access_token().await?;
        match self.upload_with_token(request.clone(), token).await {
            Ok(response) => parse_accepted_event_ids(response),
            Err(status) if status.code() == Code::Unauthenticated => {
                auth.invalidate_token().await?;
                let fresh_token = auth.access_token().await?;
                let response = self
                    .upload_with_token(request, fresh_token)
                    .await
                    .map_err(|status| status as ClientError)?;
                parse_accepted_event_ids(response)
            }
            Err(status) => Err(status),
        }
    }

    pub async fn send(
        &self,
        device_id: DeviceId,
        events: &[Event],
    ) -> Result<Vec<EventId>, ClientError> {
        let request = build_upload_request(device_id, events)?;

        let response = self
            .client
            .clone()
            .upload_events(request)
            .await?
            .into_inner();

        parse_accepted_event_ids(response)
    }

    async fn upload_with_token(
        &self,
        request: UploadEventsRequest,
        token: String,
    ) -> Result<UploadEventsResponse, Box<tonic::Status>> {
        let mut client = ClientMetricsUploaderClient::with_interceptor(
            self.channel.clone(),
            GrpcAuthInterceptor { token },
        );

        client
            .upload_events(request)
            .await
            .map(|response| response.into_inner())
            .map_err(Box::new)
    }
}

fn build_upload_request(
    device_id: DeviceId,
    events: &[Event],
) -> Result<UploadEventsRequest, ClientError> {
    Ok(UploadEventsRequest {
        device_id: device_id.0.as_hyphenated().to_string(),
        events: events
            .iter()
            .map(event_to_proto)
            .collect::<Result<_, _>>()?,
    })
}

fn parse_accepted_event_ids(response: UploadEventsResponse) -> Result<Vec<EventId>, ClientError> {
    response
        .accepted_event_ids
        .into_iter()
        .map(|raw_id| {
            Uuid::parse_str(&raw_id).map(EventId).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("server returned invalid accepted event id '{raw_id}': {error}"),
                )
                .into()
            })
        })
        .collect()
}

fn event_to_proto(event: &Event) -> Result<EventMessage, ClientError> {
    let occurred_at_unix_nanos = event
        .occurred_at
        .unix_timestamp_nanos()
        .try_into()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "event {} timestamp does not fit into i64: {error}",
                    event.id.0
                ),
            )
        })?;

    let kind = match &event.kind {
        EventKind::CollectorStarted => event_message::Kind::CollectorStarted(true),
        EventKind::CollectorStopped => event_message::Kind::CollectorStopped(true),
        EventKind::Heartbeat => event_message::Kind::Heartbeat(true),
        EventKind::Observation(observation) => observation_to_proto(observation),
        EventKind::AdapterStatus {
            adapter_id,
            observation_type,
            status,
        } => {
            let observation_type = match observation_type {
                timetracker_model::ObservationType::Idle => ObservationTypeProto::Idle,
                timetracker_model::ObservationType::FocusedApplication => {
                    ObservationTypeProto::FocusedApplication
                }
                timetracker_model::ObservationType::Keyboard => ObservationTypeProto::Keyboard,
                timetracker_model::ObservationType::MouseClick => ObservationTypeProto::MouseClick,
                timetracker_model::ObservationType::MouseMove => ObservationTypeProto::MouseMove,
                timetracker_model::ObservationType::BrowserActivity => {
                    ObservationTypeProto::BrowserActivity
                }
            };
            let status = match status {
                AdapterStatus::Available => adapter_status_message::Status::Available(true),
                AdapterStatus::Unavailable { reason } => {
                    adapter_status_message::Status::UnavailableReason(reason.clone())
                }
            };

            event_message::Kind::AdapterStatus(AdapterStatusMessage {
                adapter_id: adapter_id.clone(),
                observation_type: observation_type.into(),
                status: Some(status),
            })
        }
    };

    Ok(EventMessage {
        id: event.id.0.as_hyphenated().to_string(),
        device_id: event.device_id.0.as_hyphenated().to_string(),
        occurred_at_unix_nanos,
        kind: Some(kind),
    })
}

fn observation_to_proto(observation: &ObservationKind) -> event_message::Kind {
    match observation {
        ObservationKind::IdleChanged { idle } => event_message::Kind::IdleChanged(*idle),
        ObservationKind::FocusedApplicationChanged { app_id, title } => {
            event_message::Kind::FocusedApplicationChanged(FocusedApplicationChangedMessage {
                app_id: app_id.clone(),
                title: title.clone(),
            })
        }
        ObservationKind::KeyboardButtonPressed { key } => {
            event_message::Kind::KeyboardButtonPressed(KeyboardButtonPressedMessage {
                key: key.clone(),
            })
        }
        ObservationKind::MouseClicked { button, x, y } => {
            event_message::Kind::MouseClicked(MouseClickedMessage {
                button: button.clone(),
                x: *x,
                y: *y,
            })
        }
        ObservationKind::MouseMoved { x, y } => {
            event_message::Kind::MouseMoved(MouseMovedMessage { x: *x, y: *y })
        }
        ObservationKind::SystemSleepChanged { sleeping } => {
            event_message::Kind::SystemSleepChanged(*sleeping)
        }
        ObservationKind::ActiveBrowserTabChanged {
            source_id,
            window_id,
            tab_id,
            title,
        } => event_message::Kind::ActiveBrowserTabChanged(ActiveBrowserTabChangedMessage {
            source_id: source_id.clone(),
            window_id: *window_id,
            tab_id: *tab_id,
            title: title.clone(),
        }),
        ObservationKind::ActiveBrowserTabTitleChanged {
            source_id,
            window_id,
            tab_id,
            title,
        } => {
            event_message::Kind::ActiveBrowserTabTitleChanged(ActiveBrowserTabTitleChangedMessage {
                source_id: source_id.clone(),
                window_id: *window_id,
                tab_id: *tab_id,
                title: title.clone(),
            })
        }
        ObservationKind::BrowserWindowFocusChanged {
            source_id,
            window_id,
            focused,
        } => event_message::Kind::BrowserWindowFocusChanged(BrowserWindowFocusChangedMessage {
            source_id: source_id.clone(),
            window_id: *window_id,
            focused: *focused,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use time::OffsetDateTime;
    use timetracker_model::auth::{DeviceCredentials, TokenResponse};
    use timetracker_proto::metrics::client_metrics_uploader_server::{
        ClientMetricsUploader, ClientMetricsUploaderServer,
    };
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Request, Response, Status, transport::Server};

    use crate::registration::HttpAuthClient;

    use super::*;

    #[test]
    fn preserves_absent_optional_title() {
        let event = Event {
            id: EventId(Uuid::new_v4()),
            device_id: DeviceId(Uuid::new_v4()),
            occurred_at: OffsetDateTime::UNIX_EPOCH,
            kind: EventKind::Observation(ObservationKind::FocusedApplicationChanged {
                app_id: "com.example.App".to_owned(),
                title: None,
            }),
        };

        let encoded = event_to_proto(&event).unwrap();
        let Some(event_message::Kind::FocusedApplicationChanged(app)) = encoded.kind else {
            panic!("expected a focused-application observation");
        };
        assert_eq!(app.app_id, "com.example.App");
        assert_eq!(app.title, None);
    }

    struct MockUploader {
        expected_token: String,
        attempts: Arc<AtomicUsize>,
        seen_event_ids: Arc<Mutex<Vec<Vec<String>>>>,
    }

    #[tonic::async_trait]
    impl ClientMetricsUploader for MockUploader {
        async fn upload_events(
            &self,
            request: Request<UploadEventsRequest>,
        ) -> Result<Response<UploadEventsResponse>, Status> {
            self.attempts.fetch_add(1, Ordering::SeqCst);

            let auth = request
                .metadata()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned();

            let payload = request.into_inner();
            let ids: Vec<String> = payload.events.iter().map(|e| e.id.clone()).collect();
            self.seen_event_ids.lock().unwrap().push(ids.clone());

            if auth != format!("Bearer {}", self.expected_token) {
                return Err(Status::unauthenticated("expired or invalid token"));
            }

            Ok(Response::new(UploadEventsResponse {
                accepted_event_ids: ids,
            }))
        }
    }

    async fn spawn_mock_http_login(counter: Arc<AtomicUsize>) -> String {
        let app = Router::new()
            .route(
                "/api/devices/login",
                post(|State(counter): State<Arc<AtomicUsize>>| async move {
                    let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    (
                        StatusCode::OK,
                        Json(TokenResponse {
                            access_token: format!("token-{n}"),
                            token_type: "Bearer".to_owned(),
                            expires_in: 300,
                        }),
                    )
                }),
            )
            .with_state(counter);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    async fn spawn_mock_grpc_server(service: MockUploader) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let incoming = TcpListenerStream::new(listener);

        tokio::spawn(async move {
            Server::builder()
                .add_service(ClientMetricsUploaderServer::new(service))
                .serve_with_incoming(incoming)
                .await
                .unwrap();
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn send_authenticated_retries_once_with_same_event_ids_on_unauthenticated() {
        let login_calls = Arc::new(AtomicUsize::new(0));
        let http_url = spawn_mock_http_login(Arc::clone(&login_calls)).await;

        let attempts = Arc::new(AtomicUsize::new(0));
        let seen_event_ids = Arc::new(Mutex::new(Vec::new()));
        let grpc_url = spawn_mock_grpc_server(MockUploader {
            expected_token: "token-2".to_owned(),
            attempts: Arc::clone(&attempts),
            seen_event_ids: Arc::clone(&seen_event_ids),
        })
        .await;

        let device_id = DeviceId(Uuid::new_v4());
        let auth = ClientAuthService::new(
            HttpAuthClient::new(http_url),
            DeviceCredentials {
                device_id,
                secret: URL_SAFE_NO_PAD.encode([1u8; 32]),
            },
        );
        let uploader = GrpcUploadClient::new(grpc_url).unwrap();

        let event_id = EventId(Uuid::new_v4());
        let events = vec![Event {
            id: event_id,
            device_id,
            occurred_at: OffsetDateTime::UNIX_EPOCH,
            kind: EventKind::Heartbeat,
        }];

        let accepted = uploader.send_authenticated(&auth, &events).await.unwrap();

        assert_eq!(accepted, vec![event_id]);
        assert_eq!(login_calls.load(Ordering::SeqCst), 2);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);

        let recorded = seen_event_ids.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0], recorded[1]);
    }

    #[tokio::test]
    async fn send_authenticated_stops_after_single_retry() {
        let login_calls = Arc::new(AtomicUsize::new(0));
        let http_url = spawn_mock_http_login(Arc::clone(&login_calls)).await;

        let attempts = Arc::new(AtomicUsize::new(0));
        let grpc_url = spawn_mock_grpc_server(MockUploader {
            expected_token: "never-issued".to_owned(),
            attempts: Arc::clone(&attempts),
            seen_event_ids: Arc::new(Mutex::new(Vec::new())),
        })
        .await;

        let device_id = DeviceId(Uuid::new_v4());
        let auth = ClientAuthService::new(
            HttpAuthClient::new(http_url),
            DeviceCredentials {
                device_id,
                secret: URL_SAFE_NO_PAD.encode([2u8; 32]),
            },
        );
        let uploader = GrpcUploadClient::new(grpc_url).unwrap();

        let events = vec![Event {
            id: EventId(Uuid::new_v4()),
            device_id,
            occurred_at: OffsetDateTime::UNIX_EPOCH,
            kind: EventKind::Heartbeat,
        }];

        let result = uploader.send_authenticated(&auth, &events).await;
        assert!(result.is_err());
        assert_eq!(login_calls.load(Ordering::SeqCst), 2);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
