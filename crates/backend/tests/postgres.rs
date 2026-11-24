use sqlx::PgPool;
use time::OffsetDateTime;
use timetracker_backend::{BackendError, dashboard::TimeRange, repositories::*};
use timetracker_model::auth::DeviceApprovalRequest;
use timetracker_model::*;
use tokio::sync::Mutex;
use uuid::Uuid;

static DB_TEST_LOCK: Mutex<()> = Mutex::const_new(());

fn sample_hash(hex_char: char) -> String {
    std::iter::repeat_n(hex_char, 64).collect()
}

async fn connect_and_init() -> PgPool {
    let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../schema.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../schema.sql"))
        .execute(&pool)
        .await
        .unwrap();
    pool
}

#[tokio::test]
#[ignore = "requires an isolated TEST_DATABASE_URL"]
async fn postgres_persists_idempotently_and_rolls_back_conflicting_batches() {
    let _guard = DB_TEST_LOCK.lock().await;
    let pool = connect_and_init().await;
    let user = UserId(Uuid::new_v4());
    let device = DeviceId(Uuid::new_v4());
    sqlx::query("INSERT INTO users (id, name) VALUES ($1, 'Integration Test')")
        .bind(user.0)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO devices (id, user_id, credential_hash) VALUES ($1, $2, $3)")
        .bind(device.0)
        .bind(user.0)
        .bind(sample_hash('a'))
        .execute(&pool)
        .await
        .unwrap();
    let events = PostgresEventRepository(pool.clone());
    let event = Event {
        id: EventId(Uuid::new_v4()),
        device_id: device,
        occurred_at: OffsetDateTime::UNIX_EPOCH,
        kind: EventKind::Heartbeat,
    };
    assert_eq!(
        PostgresDeviceRepository(pool.clone())
            .owner_of(device)
            .await
            .unwrap(),
        Some(user)
    );
    assert_eq!(
        events
            .insert_batch(user, std::slice::from_ref(&event))
            .await
            .unwrap(),
        [event.id]
    );
    let received_at: OffsetDateTime =
        sqlx::query_scalar("SELECT received_at FROM events WHERE device_id = $1 AND id = $2")
            .bind(device.0)
            .bind(event.id.0)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(received_at > event.occurred_at);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(
        events
            .insert_batch(user, std::slice::from_ref(&event))
            .await
            .unwrap(),
        [event.id]
    );
    let retried_received_at: OffsetDateTime =
        sqlx::query_scalar("SELECT received_at FROM events WHERE device_id = $1 AND id = $2")
            .bind(device.0)
            .bind(event.id.0)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(retried_received_at, received_at);
    let mut conflict = event.clone();
    conflict.kind = EventKind::CollectorStopped;
    let mut new_event = event.clone();
    new_event.id = EventId(Uuid::new_v4());
    assert!(
        events
            .insert_batch(user, &[new_event, conflict])
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM events WHERE user_id = $1")
        .bind(user.0)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let actions = PostgresUserActionRepository(pool.clone());
    assert_eq!(
        actions
            .user_actions(
                user,
                TimeRange {
                    start: event.occurred_at - time::Duration::seconds(1),
                    end: event.occurred_at + time::Duration::seconds(1)
                }
            )
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        PostgresUserRepository(pool.clone())
            .find_user(user)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Integration Test"
    );
    assert!(
        actions
            .preceding_actions(user, event.occurred_at)
            .await
            .unwrap()
            .is_empty()
    );
    let preceding = actions
        .preceding_actions(user, event.occurred_at + time::Duration::seconds(1))
        .await
        .unwrap();
    assert_eq!(preceding.len(), 1);
    assert_eq!(preceding[0].id, event.id);
    let other_device = DeviceId(Uuid::new_v4());
    sqlx::query("INSERT INTO devices (id, user_id, credential_hash) VALUES ($1, $2, $3)")
        .bind(other_device.0)
        .bind(user.0)
        .bind(sample_hash('b'))
        .execute(&pool)
        .await
        .unwrap();
    let other_event = Event {
        device_id: other_device,
        ..event.clone()
    };
    assert_eq!(
        events.insert_batch(user, &[other_event]).await.unwrap(),
        [event.id]
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM events WHERE user_id = $1 AND id = $2")
            .bind(user.0)
            .bind(event.id.0)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2);
    sqlx::query("DELETE FROM events WHERE user_id = $1")
        .bind(user.0)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM devices WHERE user_id = $1")
        .bind(user.0)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user.0)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires an isolated TEST_DATABASE_URL"]
async fn postgres_users_and_device_approvals_enforce_ownership_and_conflicts() {
    let _guard = DB_TEST_LOCK.lock().await;
    let pool = connect_and_init().await;
    let users = PostgresUserRepository::new(pool.clone());
    let devices = PostgresDeviceRepository::new(pool.clone());

    let user1 = users.create_user("Alice").await.unwrap();
    let user2 = users.create_user("Bob").await.unwrap();
    assert_eq!(
        users.find_user(user1.id).await.unwrap().unwrap().name,
        "Alice"
    );

    let device_id = DeviceId(Uuid::new_v4());
    let hash_a = sample_hash('a');
    let hash_b = sample_hash('b');

    let missing_user = devices
        .create_approved(DeviceApprovalRequest {
            device_id,
            user_id: UserId(Uuid::new_v4()),
            credential_hash: hash_a.clone(),
        })
        .await;
    assert!(matches!(missing_user, Err(BackendError::NotFound(_))));

    let invalid_hash = devices
        .create_approved(DeviceApprovalRequest {
            device_id,
            user_id: user1.id,
            credential_hash: "not-a-sha256-hex".into(),
        })
        .await;
    assert!(matches!(invalid_hash, Err(BackendError::Validation(_))));

    let created = devices
        .create_approved(DeviceApprovalRequest {
            device_id,
            user_id: user1.id,
            credential_hash: hash_a.clone(),
        })
        .await
        .unwrap();
    assert_eq!(created.device_id, device_id);
    assert_eq!(created.user_id, user1.id);
    assert_eq!(created.credential_hash, hash_a);

    assert_eq!(
        devices.find(device_id).await.unwrap(),
        Some(created.clone())
    );
    assert_eq!(devices.owner_of(device_id).await.unwrap(), Some(user1.id));
    assert!(devices.list().await.unwrap().contains(&created));

    let repeated = devices
        .create_approved(DeviceApprovalRequest {
            device_id,
            user_id: user1.id,
            credential_hash: hash_a.clone(),
        })
        .await
        .unwrap();
    assert_eq!(repeated, created);

    let hash_conflict = devices
        .create_approved(DeviceApprovalRequest {
            device_id,
            user_id: user1.id,
            credential_hash: hash_b,
        })
        .await;
    assert!(matches!(hash_conflict, Err(BackendError::Conflict(_))));

    let owner_conflict = devices
        .create_approved(DeviceApprovalRequest {
            device_id,
            user_id: user2.id,
            credential_hash: hash_a,
        })
        .await;
    assert!(matches!(owner_conflict, Err(BackendError::Conflict(_))));

    sqlx::query("DELETE FROM devices WHERE id = $1")
        .bind(device_id.0)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
        .bind(user1.id.0)
        .bind(user2.id.0)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires an isolated TEST_DATABASE_URL"]
async fn postgres_admin_accounts_crud_and_concurrent_seeding() {
    let _guard = DB_TEST_LOCK.lock().await;
    let pool = connect_and_init().await;
    let admins = PostgresAdminAccountRepository::new(pool.clone());

    sqlx::query("DELETE FROM admin_accounts")
        .execute(&pool)
        .await
        .unwrap();

    let mut handles = Vec::new();
    for i in 0..8 {
        let repo = PostgresAdminAccountRepository::new(pool.clone());
        let username = format!("initial-admin-{i}");
        handles.push(tokio::spawn(async move {
            repo.ensure_initial_account(&username, "initial-hash").await
        }));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    let seeded = admins.list().await.unwrap();
    assert_eq!(seeded.len(), 1);
    let initial = seeded[0].clone();
    assert_eq!(initial.password_hash, "initial-hash");

    admins
        .change_password(initial.id, "updated-hash")
        .await
        .unwrap();
    admins
        .ensure_initial_account("should-not-be-created", "other-hash")
        .await
        .unwrap();

    let after_reseed = admins.list().await.unwrap();
    assert_eq!(after_reseed.len(), 1);
    assert_eq!(after_reseed[0].id, initial.id);
    assert_eq!(after_reseed[0].username, initial.username);
    assert_eq!(after_reseed[0].password_hash, "updated-hash");

    let second = admins.create("second-admin", "hash-2").await.unwrap();
    assert_eq!(
        admins.find_by_id(second.id).await.unwrap(),
        Some(second.clone())
    );
    assert_eq!(
        admins.find_by_username("second-admin").await.unwrap(),
        Some(second.clone())
    );
    assert!(
        admins
            .find_by_username("does-not-exist")
            .await
            .unwrap()
            .is_none()
    );

    let duplicate = admins.create("second-admin", "hash-3").await;
    assert!(matches!(duplicate, Err(BackendError::Conflict(_))));

    let unknown_pw = admins.change_password(Uuid::new_v4(), "hash-4").await;
    assert!(matches!(unknown_pw, Err(BackendError::NotFound(_))));

    sqlx::query("DELETE FROM admin_accounts")
        .execute(&pool)
        .await
        .unwrap();
}
