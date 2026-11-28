use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{future::IntoFuture, sync::Arc};
use time::OffsetDateTime;
use timetracker_backend::{
    api::{GrpcMetricsServer, HttpDashboardServer, auth::HttpAuthApi},
    auth::{AuthService, hash_password, jwt_key},
    collection::MetricsCollectionService,
    dashboard::DashboardService,
    repositories::*,
};
use timetracker_client::{
    registration::{ClientAuthService, HttpAuthClient},
    upload::GrpcUploadClient,
};
use timetracker_model::{DeviceId, Event, EventId, EventKind, ObservationKind, auth::*};
use timetracker_proto::metrics::{
    UploadEventsRequest, client_metrics_uploader_client::ClientMetricsUploaderClient,
    client_metrics_uploader_server::ClientMetricsUploaderServer,
};
use tokio_stream::wrappers::TcpListenerStream;
use tower::ServiceExt;
use uuid::Uuid;

fn service(pool: &PgPool) -> Arc<AuthService> {
    Arc::new(AuthService::new(
        Arc::new(PostgresAdminAccountRepository::new(pool.clone())),
        Arc::new(PostgresDeviceRepository::new(pool.clone())),
        URL_SAFE_NO_PAD.encode([42u8; 32]),
    ))
}
fn sign(claims: &Value, algorithm: Algorithm) -> String {
    jsonwebtoken::encode(
        &Header::new(algorithm),
        claims,
        &EncodingKey::from_secret(&[42; 32]),
    )
    .unwrap()
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

#[tokio::test]
async fn jwt_rejects_invalid_keys_signatures_claims_and_expiry() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    let auth = service(&pool);
    assert!(jwt_key("not-a-key").is_err());
    assert!(jwt_key(&URL_SAFE_NO_PAD.encode([0; 31])).is_err());
    let now = OffsetDateTime::now_utc().unix_timestamp() as u64;
    let claims = json!({"sub":Uuid::new_v4(),"kind":"admin","iss":"timetracker","aud":"timetracker-api","iat":now,"exp":now+900});
    assert!(
        auth.validate_token(&sign(&claims, Algorithm::HS256))
            .is_ok()
    );
    for (field, value) in [
        ("sub", json!("invalid")),
        ("kind", json!("other")),
        ("iss", json!("other")),
        ("aud", json!("other")),
        ("iat", json!(now + 1)),
        ("exp", json!(now + 901)),
    ] {
        let mut invalid = claims.clone();
        invalid[field] = value;
        assert!(
            auth.validate_token(&sign(&invalid, Algorithm::HS256))
                .is_err(),
            "{field}"
        );
    }
    let mut expired = claims.clone();
    expired["iat"] = json!(now - 901);
    expired["exp"] = json!(now - 1);
    assert!(
        auth.validate_token(&sign(&expired, Algorithm::HS256))
            .is_err()
    );
    assert!(
        auth.validate_token(&sign(&claims, Algorithm::HS384))
            .is_err()
    );
    let wrong_key = jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&[43; 32]),
    )
    .unwrap();
    assert!(auth.validate_token(&wrong_key).is_err());
}

#[tokio::test]
async fn login_budget_is_per_peer_and_errors_are_json() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    let app =
        HttpAuthApi::new(service(&pool), Arc::new(PostgresUserRepository::new(pool))).router();
    for attempt in 0..32 {
        let peer = if attempt == 31 {
            "127.0.0.2:1234"
        } else {
            "127.0.0.1:1234"
        };
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header("content-type", "application/json")
            .body(Body::from("{"))
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<std::net::SocketAddr>().unwrap()));
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            if attempt == 30 {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::BAD_REQUEST
            }
        );
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert!(body["code"].is_string());
        assert!(body["message"].is_string());
    }
}

#[tokio::test]
#[ignore = "requires an isolated TEST_DATABASE_URL"]
async fn approval_login_authenticated_upload_and_activity_work_together() {
    let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../schema.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let admins = PostgresAdminAccountRepository::new(pool.clone());
    admins
        .ensure_initial_account("demo-admin", &hash_password("demo-password").unwrap())
        .await
        .unwrap();
    let auth = service(&pool);
    let users = Arc::new(PostgresUserRepository::new(pool.clone()));
    let dashboard = Arc::new(DashboardService::new(
        users,
        Arc::new(PostgresUserActionRepository::new(pool.clone())),
    ));
    let app = HttpDashboardServer::new(dashboard)
        .with_auth(auth.clone())
        .router();
    assert_eq!(
        call(&app, "GET", "/users", None, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let preflight = app
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/auth/login")
                .header("origin", "http://localhost:5173")
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "content-type,authorization",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);
    assert_eq!(
        preflight.headers()["access-control-allow-origin"],
        "http://localhost:5173"
    );
    let (status, login) = call(
        &app,
        "POST",
        "/api/auth/login",
        None,
        json!({"username":"demo-admin","password":"demo-password"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(login["expires_in"], 900);
    assert_eq!(login["token_type"], "Bearer");
    let token = login["access_token"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", "/api/auth/me", Some(token), Value::Null)
            .await
            .1["username"],
        "demo-admin"
    );
    let (status, user) = call(
        &app,
        "POST",
        "/users",
        Some(token),
        json!({"name":"Demo User"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, other_user) = call(
        &app,
        "POST",
        "/users",
        Some(token),
        json!({"name":"Other User"}),
    )
    .await;
    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_url = format!("http://{}", http_listener.local_addr().unwrap());
    let http = tokio::spawn(
        axum::serve(
            http_listener,
            app.clone()
                .into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let device_id = DeviceId(Uuid::new_v4());
    let client_auth = ClientAuthService::new(
        HttpAuthClient::new(http_url),
        DeviceCredentials {
            device_id,
            secret: URL_SAFE_NO_PAD.encode([7u8; 32]),
        },
    );
    assert!(!client_auth.check_registration().await.unwrap().registered);
    let approval = json!({"device_id":device_id,"user_id":user["id"],"credential_hash":client_auth.credential_hash().unwrap()});
    assert_eq!(
        call(&app, "POST", "/api/devices", Some(token), approval.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", "/api/devices", Some(token), approval.clone())
            .await
            .0,
        StatusCode::OK
    );
    let mut conflict = approval.clone();
    conflict["user_id"] = other_user["id"].clone();
    assert_eq!(
        call(&app, "POST", "/api/devices", Some(token), conflict)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert!(client_auth.check_registration().await.unwrap().registered);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/devices/register",
            None,
            json!({"device_id":device_id,"secret":URL_SAFE_NO_PAD.encode([8u8;32])})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let device_token = client_auth.access_token().await.unwrap();
    assert_eq!(
        call(&app, "GET", "/users", Some(&device_token), Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (_, devices) = call(&app, "GET", "/api/devices", Some(token), Value::Null).await;
    assert!(devices[0].get("credential_hash").is_none());
    let grpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let grpc_url = format!("http://{}", grpc_listener.local_addr().unwrap());
    let collection = Arc::new(MetricsCollectionService::new(
        auth.devices.clone(),
        Arc::new(PostgresEventRepository::new(pool.clone())),
    ));
    let grpc_service = GrpcMetricsServer::new(collection).with_auth(auth.clone());
    let grpc = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(ClientMetricsUploaderServer::new(grpc_service))
            .serve_with_incoming(TcpListenerStream::new(grpc_listener)),
    );
    let mut raw = ClientMetricsUploaderClient::connect(grpc_url.clone())
        .await
        .unwrap();
    let payload = UploadEventsRequest {
        device_id: device_id.0.to_string(),
        events: vec![],
    };
    assert_eq!(
        raw.upload_events(tonic::Request::new(payload.clone()))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::Unauthenticated
    );
    for (bearer, id, expected) in [
        (token, device_id.0, tonic::Code::PermissionDenied),
        (
            device_token.as_str(),
            Uuid::new_v4(),
            tonic::Code::InvalidArgument,
        ),
    ] {
        let mut request = tonic::Request::new(UploadEventsRequest {
            device_id: id.to_string(),
            ..payload.clone()
        });
        request
            .metadata_mut()
            .insert("authorization", format!("Bearer {bearer}").parse().unwrap());
        assert_eq!(
            raw.upload_events(request).await.unwrap_err().code(),
            expected
        );
    }
    let now = OffsetDateTime::now_utc();
    let events = [
        Event {
            id: EventId(Uuid::new_v4()),
            device_id,
            occurred_at: now - time::Duration::minutes(2),
            kind: EventKind::Observation(ObservationKind::FocusedApplicationChanged {
                app_id: String::new(),
                title: None,
            }),
        },
        Event {
            id: EventId(Uuid::new_v4()),
            device_id,
            occurred_at: now,
            kind: EventKind::Heartbeat,
        },
    ];
    let uploader = GrpcUploadClient::new(grpc_url).unwrap();
    assert_eq!(
        uploader
            .send_authenticated(&client_auth, &events)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        uploader
            .send_authenticated(&client_auth, &events)
            .await
            .unwrap()
            .len(),
        2
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM events WHERE device_id=$1")
        .bind(device_id.0)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    let start = (now - time::Duration::minutes(1))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let end = (now + time::Duration::minutes(1))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let path = format!(
        "/users/{}/actions?start={start}&end={end}&limit=1",
        user["id"].as_str().unwrap()
    );
    let (status, page) = call(&app, "GET", &path, Some(token), Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 2);
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    let second = call(
        &app,
        "GET",
        &format!("{path}&offset=1"),
        Some(token),
        Value::Null,
    )
    .await
    .1;
    assert_ne!(page["items"][0]["id"], second["items"][0]["id"]);
    let (status, admin) = call(
        &app,
        "POST",
        "/api/admins",
        Some(token),
        json!({"username":"second-admin","password":"initial"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(admin.get("password_hash").is_none());
    assert_eq!(
        call(
            &app,
            "PATCH",
            &format!("/api/admins/{}", admin["id"].as_str().unwrap()),
            Some(token),
            json!({"password":"updated"})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            json!({"username":"second-admin","password":"initial"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, second_login) = call(
        &app,
        "POST",
        "/api/auth/login",
        None,
        json!({"username":"second-admin","password":"updated"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("DELETE FROM admin_accounts WHERE id=$1")
        .bind(Uuid::parse_str(admin["id"].as_str().unwrap()).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            "/users",
            second_login["access_token"].as_str(),
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    http.abort();
    grpc.abort();
    pool.close().await;
}
