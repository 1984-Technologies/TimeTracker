use sqlx::PgPool;
use time::OffsetDateTime;
use timetracker_backend::{dashboard::TimeRange, repositories::*};
use timetracker_model::*;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires an isolated TEST_DATABASE_URL"]
async fn postgres_persists_idempotently_and_rolls_back_conflicting_batches() {
    let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../schema.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let user = UserId(Uuid::new_v4());
    let device = DeviceId(Uuid::new_v4());
    sqlx::query("INSERT INTO users (id, name) VALUES ($1, 'Integration Test')")
        .bind(user.0)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO devices (id, user_id) VALUES ($1, $2)")
        .bind(device.0)
        .bind(user.0)
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
    sqlx::query("INSERT INTO devices (id, user_id) VALUES ($1, $2)")
        .bind(other_device.0)
        .bind(user.0)
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
