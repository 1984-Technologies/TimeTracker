use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{str::FromStr, sync::Arc};
use timetracker_backend::{
    BackendError,
    api::{GrpcMetricsServer, HttpDashboardServer, auth::HttpAuthApi},
    auth::{AuthService, hash_password},
    collection::MetricsCollectionService,
    dashboard::DashboardService,
    repositories::*,
};
use timetracker_model::{DeviceId, UserId, auth::*};
use timetracker_proto::metrics::{
    EventMessage, UploadEventsRequest, client_metrics_uploader_client::ClientMetricsUploaderClient,
    client_metrics_uploader_server::ClientMetricsUploaderServer, event_message,
};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Code, Request, transport::Server};
use uuid::Uuid;

struct Database {
    pool: PgPool,
    root: PgPool,
    schema: String,
}
impl Database {
    async fn new() -> Self {
        let url = std::env::var("TEST_DATABASE_URL").expect("set isolated TEST_DATABASE_URL");
        let root = PgPool::connect(&url).await.unwrap();
        let schema = format!("jwt_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&root)
            .await
            .unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect_with(
                PgConnectOptions::from_str(&url)
                    .unwrap()
                    .options([("search_path", schema.as_str())]),
            )
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../schema.sql"))
            .execute(&pool)
            .await
            .unwrap();
        // Schema setup must be safe for an already initialized database.
        sqlx::raw_sql(include_str!("../../schema.sql"))
            .execute(&pool)
            .await
            .unwrap();
        Self { pool, root, schema }
    }
    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.root)
            .await
            .unwrap();
        self.root.close().await;
    }
}

#[tokio::test]
#[ignore = "requires an isolated TEST_DATABASE_URL"]
async fn concurrent_bootstrap_and_approval_are_atomic_and_conflicts_are_explicit() {
    let db = Database::new().await;
    let admins = Arc::new(PostgresAdminAccountRepository::new(db.pool.clone()));
    let mut starts = tokio::task::JoinSet::new();
    for i in 0..8 {
        let admins = admins.clone();
        starts.spawn(async move {
            admins
                .ensure_initial_account(&format!("bootstrap-{i}"), "test-hash")
                .await
        });
    }
    while let Some(result) = starts.join_next().await {
        result.unwrap().unwrap();
    }
    let accounts = admins.list().await.unwrap();
    assert_eq!(accounts.len(), 1);
    let first = &accounts[0];
    assert!(matches!(
        admins.create(&first.username, "other hash").await,
        Err(BackendError::Conflict(_))
    ));
    admins
        .ensure_initial_account("restart", "new hash")
        .await
        .unwrap();
    assert_eq!(admins.list().await.unwrap().len(), 1);
    assert_eq!(
        admins
            .find_by_id(first.id)
            .await
            .unwrap()
            .unwrap()
            .password_hash,
        "test-hash"
    );
    admins
        .change_password(first.id, "updated hash")
        .await
        .unwrap();
    assert_eq!(
        admins
            .find_by_username(&first.username)
            .await
            .unwrap()
            .unwrap()
            .password_hash,
        "updated hash"
    );
    assert!(matches!(
        admins.change_password(Uuid::new_v4(), "x").await,
        Err(BackendError::NotFound(_))
    ));

    let users = PostgresUserRepository::new(db.pool.clone());
    let user = users.create_user("Concurrent Approval").await.unwrap();
    let other = users.create_user("Other Owner").await.unwrap();
    let devices = Arc::new(PostgresDeviceRepository::new(db.pool.clone()));
    let device_id = DeviceId(Uuid::new_v4());
    let hash = format!("{:x}", Sha256::digest([3; 32]));
    for _ in 0..8 {
        let devices = devices.clone();
        let hash = hash.clone();
        starts.spawn(async move {
            devices
                .create_approved(DeviceApprovalRequest {
                    device_id,
                    user_id: user.id,
                    credential_hash: hash,
                })
                .await
                .map(|_| ())
        });
    }
    while let Some(result) = starts.join_next().await {
        result.unwrap().unwrap();
    }
    assert_eq!(devices.list().await.unwrap().len(), 1);
    for (owner, hash) in [(other.id, hash.clone()), (user.id, "00".repeat(32))] {
        assert!(matches!(
            devices
                .create_approved(DeviceApprovalRequest {
                    device_id,
                    user_id: owner,
                    credential_hash: hash
                })
                .await,
            Err(BackendError::Conflict(_))
        ));
    }
    assert!(matches!(
        devices
            .create_approved(DeviceApprovalRequest {
                device_id: DeviceId(Uuid::new_v4()),
                user_id: UserId(Uuid::new_v4()),
                credential_hash: hash.clone()
            })
            .await,
        Err(BackendError::NotFound(_))
    ));
    let stored = devices.find(device_id).await.unwrap().unwrap();
    assert_eq!(stored.user_id, user.id);
    assert_eq!(stored.credential_hash, hash);
    drop(admins);
    drop(devices);
    drop(users);
    db.cleanup().await;
}

#[tokio::test]
#[ignore = "requires an isolated TEST_DATABASE_URL"]
async fn legacy_devices_require_approval_and_can_gain_credentials_without_losing_events() {
    let db = Database::new().await;
    let user = PostgresUserRepository::new(db.pool.clone())
        .create_user("Legacy Owner")
        .await
        .unwrap();
    let id = DeviceId(Uuid::new_v4());
    sqlx::query("ALTER TABLE devices DROP COLUMN credential_hash")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO devices (id,user_id) VALUES ($1,$2)")
        .bind(id.0)
        .bind(user.id.0)
        .execute(&db.pool)
        .await
        .unwrap();
    let event_id = Uuid::new_v4();
    sqlx::query("INSERT INTO events (id,user_id,device_id,occurred_at,payload) VALUES ($1,$2,$3,CURRENT_TIMESTAMP,'{}')")
        .bind(event_id).bind(user.id.0).bind(id.0).execute(&db.pool).await.unwrap();
    sqlx::raw_sql(include_str!("../../schema.sql"))
        .execute(&db.pool)
        .await
        .unwrap();
    let devices = PostgresDeviceRepository::new(db.pool.clone());
    assert!(devices.find(id).await.unwrap().is_none());
    devices
        .create_approved(DeviceApprovalRequest {
            device_id: id,
            user_id: user.id,
            credential_hash: "00".repeat(32),
        })
        .await
        .unwrap();
    assert!(devices.find(id).await.unwrap().is_some());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM events WHERE device_id=$1")
        .bind(id.0)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    drop(devices);
    db.cleanup().await;
}

async fn http_json(
    client: &Client,
    method: reqwest::Method,
    url: String,
    token: Option<&str>,
    body: Option<Value>,
    status: StatusCode,
) -> Value {
    let mut request = client.request(method, url);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    assert_eq!(response.status(), status);
    if status == StatusCode::NO_CONTENT {
        Value::Null
    } else {
        response.json().await.unwrap()
    }
}

fn upload_request(
    device: Uuid,
    events: Vec<EventMessage>,
    token: Option<&str>,
) -> Request<UploadEventsRequest> {
    let mut request = Request::new(UploadEventsRequest {
        device_id: device.to_string(),
        events,
    });
    if let Some(token) = token {
        request
            .metadata_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
    }
    request
}

#[tokio::test]
#[ignore = "requires an isolated TEST_DATABASE_URL"]
async fn authenticated_http_and_grpc_round_trip_persists_activity_and_enforces_batch_isolation() {
    let db = Database::new().await;
    let admins = Arc::new(PostgresAdminAccountRepository::new(db.pool.clone()));
    admins
        .ensure_initial_account(
            "admin",
            &hash_password("integration password".into()).await.unwrap(),
        )
        .await
        .unwrap();
    let devices = Arc::new(PostgresDeviceRepository::new(db.pool.clone()));
    let users = Arc::new(PostgresUserRepository::new(db.pool.clone()));
    let events = Arc::new(PostgresEventRepository::new(db.pool.clone()));
    let auth = Arc::new(
        AuthService::new(
            admins.clone(),
            devices.clone(),
            URL_SAFE_NO_PAD.encode([7; 32]),
        )
        .unwrap(),
    );
    let dashboard = Arc::new(DashboardService::new(
        users.clone(),
        Arc::new(PostgresUserActionRepository::new(db.pool.clone())),
    ));
    let http = HttpDashboardServer::new(
        dashboard,
        Arc::new(HttpAuthApi::new(auth.clone(), users.clone())),
        "https://dashboard.example",
    )
    .unwrap();
    let grpc = GrpcMetricsServer::new(
        Arc::new(MetricsCollectionService::new(
            devices.clone(),
            events.clone(),
        )),
        auth.clone(),
    );
    let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_url = format!("http://{}", http_listener.local_addr().unwrap());
    let grpc_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let grpc_url = format!("http://{}", grpc_listener.local_addr().unwrap());
    let (stop_http, http_stop) = oneshot::channel();
    let (stop_grpc, grpc_stop) = oneshot::channel();
    let http_task = tokio::spawn(async move {
        axum::serve(http_listener, http.router())
            .with_graceful_shutdown(async {
                let _ = http_stop.await;
            })
            .await
            .unwrap();
    });
    let grpc_task = tokio::spawn(async move {
        Server::builder()
            .add_service(ClientMetricsUploaderServer::new(grpc))
            .serve_with_incoming_shutdown(TcpListenerStream::new(grpc_listener), async {
                let _ = grpc_stop.await;
            })
            .await
            .unwrap();
    });
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    const GET: reqwest::Method = reqwest::Method::GET;
    const PATCH: reqwest::Method = reqwest::Method::PATCH;
    const POST: reqwest::Method = reqwest::Method::POST;
    let login = http_json(
        &client,
        POST,
        format!("{http_url}/api/auth/login"),
        None,
        Some(json!({"username":"admin","password":"integration password"})),
        StatusCode::OK,
    )
    .await;
    let admin_token = login["access_token"].as_str().unwrap();
    let user = http_json(
        &client,
        POST,
        format!("{http_url}/users"),
        Some(admin_token),
        Some(json!({"name":"Integration User"})),
        StatusCode::CREATED,
    )
    .await;
    let user_id: UserId = serde_json::from_value(user["id"].clone()).unwrap();
    let id = Uuid::new_v4();
    let credentials = json!({"device_id":id,"secret":URL_SAFE_NO_PAD.encode([3;32])});
    let registered = http_json(
        &client,
        POST,
        format!("{http_url}/api/devices/register"),
        None,
        Some(credentials.clone()),
        StatusCode::OK,
    )
    .await;
    assert_eq!(registered["registered"], false);
    assert!(devices.list().await.unwrap().is_empty());
    let approval = json!({"device_id":id,"user_id":user_id,"credential_hash":format!("{:x}",Sha256::digest([3;32]))});
    for _ in 0..2 {
        http_json(
            &client,
            POST,
            format!("{http_url}/api/devices"),
            Some(admin_token),
            Some(approval.clone()),
            StatusCode::OK,
        )
        .await;
    }
    let registered = http_json(
        &client,
        POST,
        format!("{http_url}/api/devices/register"),
        None,
        Some(credentials.clone()),
        StatusCode::OK,
    )
    .await;
    assert_eq!(registered["registered"], true);
    let login = http_json(
        &client,
        POST,
        format!("{http_url}/api/devices/login"),
        None,
        Some(credentials),
        StatusCode::OK,
    )
    .await;
    let device_token = login["access_token"].as_str().unwrap();
    let mut uploader = ClientMetricsUploaderClient::connect(grpc_url)
        .await
        .unwrap();
    let event_id = Uuid::new_v4();
    let event = EventMessage {
        id: event_id.to_string(),
        device_id: id.to_string(),
        occurred_at_unix_nanos: 1_700_000_000_000_000_000,
        kind: Some(event_message::Kind::Heartbeat(true)),
    };
    for _ in 0..2 {
        let accepted = uploader
            .upload_events(upload_request(id, vec![event.clone()], Some(device_token)))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(accepted.accepted_event_ids, vec![event_id.to_string()]);
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM events")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let activity_url = format!(
        "{http_url}/users/{}/actions?start=2023-11-14T00:00:00Z&end=2023-11-15T00:00:00Z",
        user_id.0
    );
    let activity = http_json(
        &client,
        GET,
        activity_url.clone(),
        Some(admin_token),
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(activity["items"][0]["id"], event_id.to_string());
    assert_eq!(activity["items"][0]["kind"], "Heartbeat");
    // The preceding-state path also reads authenticated, persisted observations.
    let activity = http_json(
        &client,
        GET,
        format!(
            "{http_url}/users/{}/actions?start=2023-11-15T00:00:00Z&end=2023-11-16T00:00:00Z",
            user_id.0
        ),
        Some(admin_token),
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(activity["total"], 1);
    http_json(
        &client,
        GET,
        activity_url.clone(),
        None,
        None,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    http_json(
        &client,
        GET,
        activity_url,
        Some(device_token),
        None,
        StatusCode::FORBIDDEN,
    )
    .await;
    for (request, expected) in [
        (
            upload_request(id, vec![EventMessage::default()], None),
            Code::Unauthenticated,
        ),
        (
            upload_request(id, vec![event.clone()], Some(admin_token)),
            Code::PermissionDenied,
        ),
        (
            upload_request(Uuid::new_v4(), vec![event.clone()], Some(device_token)),
            Code::InvalidArgument,
        ),
    ] {
        assert_eq!(
            uploader.upload_events(request).await.unwrap_err().code(),
            expected
        );
    }
    let mut other = event.clone();
    other.id = Uuid::new_v4().to_string();
    other.device_id = Uuid::new_v4().to_string();
    assert_eq!(
        uploader
            .upload_events(upload_request(
                id,
                vec![event.clone(), other],
                Some(device_token)
            ))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    let mut conflict = event.clone();
    conflict.kind = Some(event_message::Kind::CollectorStopped(true));
    let mut new_event = event.clone();
    new_event.id = Uuid::new_v4().to_string();
    assert_eq!(
        uploader
            .upload_events(upload_request(
                id,
                vec![new_event, conflict],
                Some(device_token)
            ))
            .await
            .unwrap_err()
            .code(),
        Code::AlreadyExists
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM events")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    // tonic's existing default 4 MiB inbound size limit must survive the auth wiring.
    let mut large = event;
    large.id = "x".repeat(5 * 1024 * 1024);
    let code = uploader
        .upload_events(upload_request(id, vec![large], Some(device_token)))
        .await
        .unwrap_err()
        .code();
    assert!(matches!(code, Code::OutOfRange | Code::ResourceExhausted));

    let me = http_json(
        &client,
        GET,
        format!("{http_url}/api/auth/me"),
        Some(admin_token),
        None,
        StatusCode::OK,
    )
    .await;
    http_json(
        &client,
        PATCH,
        format!("{http_url}/api/admins/{}", me["id"].as_str().unwrap()),
        Some(admin_token),
        Some(json!({"password":"changed password"})),
        StatusCode::NO_CONTENT,
    )
    .await;
    http_json(
        &client,
        POST,
        format!("{http_url}/api/auth/login"),
        None,
        Some(json!({"username":"admin","password":"integration password"})),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    http_json(
        &client,
        POST,
        format!("{http_url}/api/auth/login"),
        None,
        Some(json!({"username":"admin","password":"changed password"})),
        StatusCode::OK,
    )
    .await;
    sqlx::query("DELETE FROM events WHERE device_id=$1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM devices WHERE id=$1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        uploader
            .upload_events(upload_request(id, vec![], Some(device_token)))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    drop(uploader);
    drop(client);
    stop_http.send(()).unwrap();
    stop_grpc.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        http_task.await.unwrap();
        grpc_task.await.unwrap();
    })
    .await
    .unwrap();
    drop(auth);
    drop(users);
    drop(devices);
    drop(events);
    drop(admins);
    db.cleanup().await;
}
