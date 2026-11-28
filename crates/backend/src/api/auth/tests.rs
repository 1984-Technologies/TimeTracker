use super::*;
use crate::{
    api::{GrpcMetricsServer, HttpDashboardServer},
    collection::MetricsCollectionService,
    dashboard::DashboardService,
    test_support::*,
};
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request as HttpRequest},
};
use serde_json::{Value, json};
use timetracker_model::UserId;
use timetracker_proto::metrics::{
    EventMessage, UploadEventsRequest, client_metrics_uploader_server::ClientMetricsUploader,
    event_message,
};
use tonic::{Code, Request};
use tower::ServiceExt;

fn app(store: Arc<Store>, auth: Arc<AuthService>) -> Router {
    HttpDashboardServer::new(
        Arc::new(DashboardService::new(store.clone(), store.clone())),
        Arc::new(HttpAuthApi::new(auth, store)),
        "https://dashboard.example",
    )
    .unwrap()
    .router()
}

#[tokio::test]
async fn invalid_jwts_are_rejected_by_both_http_and_grpc() {
    use jsonwebtoken::Algorithm;
    let (store, auth) = fixture().await;
    let app = app(store.clone(), auth.clone());
    let server = GrpcMetricsServer::new(
        Arc::new(MetricsCollectionService::new(store.clone(), store.clone())),
        auth,
    );
    let now = jsonwebtoken::get_current_timestamp();
    let valid = json!({"sub":DEVICE_ID.0, "kind":"device", "iss":"timetracker", "aud":"timetracker-api", "iat":now, "exp":now+900});
    let mut invalid_tokens = vec![
        sign(valid.clone(), Algorithm::HS256, &[0; 32]),
        sign(valid.clone(), Algorithm::HS384, &JWT_KEY),
    ];
    for (field, value) in [
        ("exp", json!(now - 1)),
        ("iss", json!("wrong")),
        ("aud", json!("wrong")),
        ("sub", json!("not a uuid")),
        ("kind", json!("root")),
    ] {
        let mut claims = valid.clone();
        claims[field] = value;
        invalid_tokens.push(sign(claims, Algorithm::HS256, &JWT_KEY));
    }
    for jwt in invalid_tokens {
        assert_eq!(
            send(&app, Method::GET, "/users", Some(&jwt), None).await.0,
            StatusCode::UNAUTHORIZED
        );
        let batch = UploadEventsRequest {
            device_id: DEVICE_ID.0.to_string(),
            events: vec![event(DEVICE_ID.0)],
        };
        assert_eq!(
            server
                .upload_events(rpc_request(batch, Some(&jwt)))
                .await
                .unwrap_err()
                .code(),
            Code::Unauthenticated
        );
    }
    assert_eq!(store.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn malformed_and_duplicate_authorization_headers_are_rejected() {
    let (store, auth) = fixture().await;
    let app = app(store, auth.clone());
    let admin = token(ADMIN_ID, PrincipalKind::Admin);
    let device = token(DEVICE_ID.0, PrincipalKind::Device);
    for header in [
        "".into(),
        "Bearer".into(),
        "Basic abc".into(),
        format!("Bearer  {admin}"),
        format!("Bearer {admin} extra"),
        format!("Bearer {admin},{admin}"),
    ] {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/users")
                    .header(AUTHORIZATION, &header)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/users")
                .header(AUTHORIZATION, format!("Bearer {admin}"))
                .header(AUTHORIZATION, format!("Bearer {admin}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/users")
                .header(AUTHORIZATION, format!("bEaReR {admin}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut metadata = tonic::metadata::MetadataMap::new();
    metadata.append("authorization", format!("Bearer {device}").parse().unwrap());
    metadata.append("authorization", format!("Bearer {device}").parse().unwrap());
    assert_eq!(
        authenticate_grpc(&auth, &metadata, DEVICE_ID)
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
}

#[tokio::test]
async fn cors_configuration_requires_a_single_valid_origin_and_normalizes_a_trailing_slash() {
    let (store, auth) = fixture().await;
    for origin in [
        "*",
        "null",
        "file:///tmp/dashboard",
        "https://example.com/path",
        "https://example.com?token=secret",
        "https://name@example.com",
        "https://example.com,https://other.com",
    ] {
        let result = HttpDashboardServer::new(
            Arc::new(DashboardService::new(store.clone(), store.clone())),
            Arc::new(HttpAuthApi::new(auth.clone(), store.clone())),
            origin,
        );
        assert!(
            matches!(result, Err(BackendError::Validation(_))),
            "{origin}"
        );
    }
    let app = HttpDashboardServer::new(
        Arc::new(DashboardService::new(store.clone(), store.clone())),
        Arc::new(HttpAuthApi::new(auth, store)),
        "https://dashboard.example/",
    )
    .unwrap()
    .router();
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/health")
                .header("origin", "https://dashboard.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        "https://dashboard.example"
    );
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value, HeaderMap) {
    let mut request = HttpRequest::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    let body = match body {
        Some(value) => {
            request = request.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("API response must be JSON")
    };
    (status, value, headers)
}

#[tokio::test]
async fn management_and_activity_routes_require_an_existing_admin() {
    let (store, auth) = fixture().await;
    let app = app(store.clone(), auth);
    let admin = token(ADMIN_ID, PrincipalKind::Admin);
    let device = token(DEVICE_ID.0, PrincipalKind::Device);
    let routes = [
        (Method::GET, "/api/auth/me"),
        (Method::GET, "/api/admins"),
        (Method::POST, "/api/admins"),
        (
            Method::PATCH,
            "/api/admins/00000000-0000-0000-0000-000000000001",
        ),
        (Method::GET, "/api/devices"),
        (Method::POST, "/api/devices"),
        (
            Method::GET,
            "/api/devices/00000000-0000-0000-0000-000000000002",
        ),
        (Method::POST, "/users"),
        (Method::GET, "/users"),
        (
            Method::GET,
            "/users/00000000-0000-0000-0000-000000000003/actions",
        ),
    ];
    for (method, uri) in routes {
        for (jwt, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some(device.as_str()), StatusCode::FORBIDDEN),
            (Some("invalid"), StatusCode::UNAUTHORIZED),
        ] {
            let (status, body, _) = send(&app, method.clone(), uri, jwt, None).await;
            assert_eq!(status, expected, "{method} {uri}");
            assert!(body["code"].is_string());
        }
    }
    for uri in [
        "/api/auth/me",
        "/api/admins",
        "/api/devices",
        "/api/devices/00000000-0000-0000-0000-000000000002",
        "/users",
        "/users/00000000-0000-0000-0000-000000000003/actions?start=2026-01-01T00:00:00Z&end=2026-01-02T00:00:00Z",
    ] {
        let (status, body, _) = send(&app, Method::GET, uri, Some(&admin), None).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        let text = body.to_string();
        assert!(!text.contains("password_hash"));
        assert!(!text.contains("credential_hash"));
    }
    store.admins.lock().unwrap().clear();
    assert_eq!(
        send(&app, Method::GET, "/users", Some(&admin), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(store.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn admin_creation_password_change_and_login_return_documented_statuses() {
    let (store, auth) = fixture().await;
    let app = app(store.clone(), auth);
    let admin = token(ADMIN_ID, PrincipalKind::Admin);
    for password in ["wrong", "correct-password"] {
        let (status, body, _) = send(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({"username":"admin","password":password})),
        )
        .await;
        assert_eq!(
            status,
            if password == "wrong" {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::OK
            }
        );
        if status == StatusCode::OK {
            assert_eq!(body["expires_in"], 900);
            assert_eq!(body["token_type"], "Bearer");
        }
    }
    let body = json!({"username":"second","password":"second password"});
    let (status, created, _) = send(
        &app,
        Method::POST,
        "/api/admins",
        Some(&admin),
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created.as_object().unwrap().len(), 2);
    let id: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    assert!(
        store.admins.lock().unwrap()[&id]
            .password_hash
            .starts_with("$argon2id$")
    );
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/admins",
            Some(&admin),
            Some(body.clone())
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(body.clone())
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, body, _) = send(
        &app,
        Method::PATCH,
        &format!("/api/admins/{id}"),
        Some(&admin),
        Some(json!({"password":"new password"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null());
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({"username":"second","password":"second password"}))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({"username":"second","password":"new password"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            Method::PATCH,
            &format!("/api/admins/{}", Uuid::new_v4()),
            Some(&admin),
            Some(json!({"password":"new password"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn device_registration_approval_and_conflicts_are_read_only_or_atomic() {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use sha2::{Digest, Sha256};
    let (store, auth) = fixture().await;
    let app = app(store.clone(), auth);
    let admin = token(ADMIN_ID, PrincipalKind::Admin);
    let id = Uuid::new_v4();
    let secret = URL_SAFE_NO_PAD.encode([9; 32]);
    let credentials = json!({"device_id":id,"secret":secret});
    let (status, body, _) = send(
        &app,
        Method::POST,
        "/api/devices/register",
        None,
        Some(credentials.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"registered":false}));
    assert_eq!(store.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/devices/login",
            None,
            Some(credentials.clone())
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let approval = json!({"device_id":id,"user_id":USER_ID,"credential_hash":format!("{:x}",Sha256::digest([9;32]))});
    for _ in 0..2 {
        let (status, body, _) = send(
            &app,
            Method::POST,
            "/api/devices",
            Some(&admin),
            Some(approval.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_object().unwrap().len(), 2);
    }
    assert_eq!(store.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/devices/register",
            None,
            Some(credentials.clone())
        )
        .await
        .1,
        json!({"registered":true})
    );
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/devices/login",
            None,
            Some(credentials)
        )
        .await
        .0,
        StatusCode::OK
    );
    let wrong = json!({"device_id":id,"secret":URL_SAFE_NO_PAD.encode([8;32])});
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/devices/register",
            None,
            Some(wrong.clone())
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&app, Method::POST, "/api/devices/login", None, Some(wrong))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let mut conflict = approval.clone();
    conflict["credential_hash"] = json!("00".repeat(32));
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/devices",
            Some(&admin),
            Some(conflict)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let other_user = UserId(Uuid::new_v4());
    store.users.lock().unwrap().insert(
        other_user,
        UserSummary {
            id: other_user,
            name: "Other".into(),
        },
    );
    let mut conflict = approval.clone();
    conflict["user_id"] = json!(other_user);
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/devices",
            Some(&admin),
            Some(conflict)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let mut invalid = approval.clone();
    invalid["user_id"] = json!(Uuid::new_v4());
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/devices",
            Some(&admin),
            Some(invalid)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut invalid = approval;
    invalid["credential_hash"] = json!("AA".repeat(32));
    assert_eq!(
        send(
            &app,
            Method::POST,
            "/api/devices",
            Some(&admin),
            Some(invalid)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &app,
            Method::GET,
            &format!("/api/devices/{}", Uuid::new_v4()),
            Some(&admin),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn extraction_errors_are_json_and_do_not_echo_passwords() {
    let (store, auth) = fixture().await;
    let app = app(store, auth);
    let admin = token(ADMIN_ID, PrincipalKind::Admin);
    for (method, uri, body) in [
        (Method::POST, "/users", Some(json!({"name":"   "}))),
        (
            Method::POST,
            "/api/admins",
            Some(json!({"username":"","password":"private-value"})),
        ),
        (
            Method::POST,
            "/api/auth/login",
            Some(json!({"password":"private-value"})),
        ),
        (Method::GET, "/api/devices/not-a-uuid", None),
        (
            Method::GET,
            "/users/00000000-0000-0000-0000-000000000003/actions?start=oops",
            None,
        ),
    ] {
        let (status, body, _) = send(&app, method, uri, Some(&admin), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "bad_request");
        assert!(!body.to_string().contains("private-value"));
    }
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from("{\"password\":\"private-value\""))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("private-value"));
    assert_eq!(
        send(&app, Method::DELETE, "/api/auth/login", None, None)
            .await
            .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        send(&app, Method::GET, "/missing", None, None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn cors_preflight_and_health_are_public_but_other_origins_are_not_allowed() {
    let (store, auth) = fixture().await;
    let app = app(store, auth);
    assert_eq!(
        send(&app, Method::GET, "/health", None, None).await.0,
        StatusCode::OK
    );
    for origin in ["https://dashboard.example", "https://other.example"] {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("OPTIONS")
                    .uri("/api/admins/1")
                    .header("origin", origin)
                    .header("access-control-request-method", "PATCH")
                    .header(
                        "access-control-request-headers",
                        "authorization,content-type",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // A fixed allowed origin never reflects an attacker-supplied Origin.
        assert_ne!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "https://other.example"
        );
        assert!(
            response.headers()["access-control-allow-headers"]
                .to_str()
                .unwrap()
                .contains("authorization")
        );
    }
}

#[tokio::test]
async fn login_rate_limit_and_repository_errors_have_safe_json_responses() {
    let (store, auth) = fixture().await;
    let app = app(store.clone(), auth);
    for _ in 0..20 {
        assert_eq!(
            send(
                &app,
                Method::POST,
                "/api/auth/login",
                None,
                Some(json!({"username":"missing","password":"wrong"}))
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, body, headers) = send(
        &app,
        Method::POST,
        "/api/devices/login",
        None,
        Some(json!({"device_id":DEVICE_ID,"secret":"ignored"})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rate_limited");
    assert_eq!(headers["retry-after"], "60");
    store.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    let (status, body, _) = send(
        &app,
        Method::GET,
        "/users",
        Some(&token(ADMIN_ID, PrincipalKind::Admin)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["message"], "Internal server error");
}

fn event(device_id: Uuid) -> EventMessage {
    EventMessage {
        id: Uuid::new_v4().to_string(),
        device_id: device_id.to_string(),
        occurred_at_unix_nanos: 1_700_000_000_000_000_000,
        kind: Some(event_message::Kind::Heartbeat(true)),
    }
}
fn rpc_request(payload: UploadEventsRequest, jwt: Option<&str>) -> Request<UploadEventsRequest> {
    let mut request = Request::new(payload);
    if let Some(jwt) = jwt {
        request
            .metadata_mut()
            .insert("authorization", format!("Bearer {jwt}").parse().unwrap());
    }
    request
}

#[tokio::test]
async fn grpc_authenticates_before_conversion_and_rejects_cross_device_batches_without_writes() {
    let (store, auth) = fixture().await;
    let server = GrpcMetricsServer::new(
        Arc::new(MetricsCollectionService::new(store.clone(), store.clone())),
        auth.clone(),
    );
    let device_token = token(DEVICE_ID.0, PrincipalKind::Device);
    let batch = UploadEventsRequest {
        device_id: DEVICE_ID.0.to_string(),
        events: vec![event(DEVICE_ID.0)],
    };
    for (jwt, code) in [
        (None, Code::Unauthenticated),
        (Some("bad".into()), Code::Unauthenticated),
        (
            Some(token(ADMIN_ID, PrincipalKind::Admin)),
            Code::PermissionDenied,
        ),
        (
            Some(token(Uuid::new_v4(), PrincipalKind::Device)),
            Code::Unauthenticated,
        ),
    ] {
        let malformed = UploadEventsRequest {
            device_id: "invalid".into(),
            events: vec![EventMessage::default()],
        };
        assert_eq!(
            server
                .upload_events(rpc_request(malformed, jwt.as_deref()))
                .await
                .unwrap_err()
                .code(),
            code
        );
    }
    let mut cross_batch = batch.clone();
    cross_batch.device_id = Uuid::new_v4().to_string();
    let mut cross_event = batch.clone();
    cross_event.events.push(event(Uuid::new_v4()));
    let mut invalid_event = batch.clone();
    invalid_event.events.push(EventMessage::default());
    for invalid in [cross_batch, cross_event, invalid_event] {
        assert_eq!(
            server
                .upload_events(rpc_request(invalid, Some(&device_token)))
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
    }
    assert_eq!(store.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    for _ in 0..2 {
        let response = server
            .upload_events(rpc_request(batch.clone(), Some(&device_token)))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            response.accepted_event_ids,
            vec![batch.events[0].id.clone()]
        );
    }
    assert_eq!(store.events.lock().unwrap().len(), 1);
    let mut conflict = batch.clone();
    conflict.events[0].kind = Some(event_message::Kind::CollectorStopped(true));
    assert_eq!(
        server
            .upload_events(rpc_request(conflict, Some(&device_token)))
            .await
            .unwrap_err()
            .code(),
        Code::AlreadyExists
    );
    let mut metadata = tonic::metadata::MetadataMap::new();
    metadata.insert(
        "authorization",
        format!("Bearer {device_token}").parse().unwrap(),
    );
    assert_eq!(
        authenticate_grpc(&auth, &metadata, DEVICE_ID)
            .await
            .unwrap()
            .sub,
        DEVICE_ID.0
    );
    assert_eq!(
        authenticate_grpc(&auth, &metadata, DeviceId(Uuid::new_v4()))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    store.devices.lock().unwrap().clear();
    assert_eq!(
        server
            .upload_events(rpc_request(batch, Some(&device_token)))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
}
