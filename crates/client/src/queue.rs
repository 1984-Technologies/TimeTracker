use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;
use timetracker_model::{Event, EventId};
use tokio::sync::Notify;

use crate::ClientError;

pub struct EventQueue {
    queue: Mutex<VecDeque<Event>>,
    notify: Notify,
}

impl EventQueue {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
        }
    }

    fn lock_queue(&self) -> Result<std::sync::MutexGuard<'_, VecDeque<Event>>, ClientError> {
        self.queue
            .lock()
            .map_err(|e| format!("Event queue lock poisoned: {}", e).into())
    }

    pub async fn push(&self, _event: Event) -> Result<(), ClientError> {
        {
            let mut queue = self.lock_queue()?;
            queue.push_back(_event);
        }
        self.notify.notify_one();
        Ok(())
    }

    pub async fn wait_for_events(&self) -> Result<(), ClientError> {
        loop {
            let notify = self.notify.notified();
            tokio::pin!(notify);
            notify.as_mut().enable();
            {
                let queue = self.lock_queue()?;
                if !queue.is_empty() {
                    return Ok(());
                }
            }
            notify.await;
        }
    }

    pub async fn pending(&self, _limit: usize) -> Result<Vec<Event>, ClientError> {
        if _limit == 0 {
            return Ok(Vec::new());
        }
        let queue = self.lock_queue()?;
        let events = queue.iter().take(_limit).cloned().collect();
        Ok(events)
    }

    pub async fn acknowledge(&self, _ids: &[EventId]) -> Result<(), ClientError> {
        if _ids.is_empty() {
            return Ok(());
        }
        let ids: HashSet<EventId> = _ids.iter().copied().collect();
        let mut queue = self.lock_queue()?;
        queue.retain(|event| !ids.contains(&event.id));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use time::OffsetDateTime;
    use timetracker_model::{DeviceId, EventKind};
    use uuid::Uuid;

    fn make_event() -> Event {
        Event {
            id: EventId(Uuid::new_v4()),
            device_id: DeviceId(Uuid::new_v4()),
            occurred_at: OffsetDateTime::now_utc(),
            kind: EventKind::Heartbeat,
        }
    }

    #[tokio::test]
    async fn pending_returns_oldest_events_without_removing_them() {
        let queue = EventQueue::new();
        let e1 = make_event();
        let e2 = make_event();
        let e3 = make_event();
        queue.push(e1.clone()).await.unwrap();
        queue.push(e2.clone()).await.unwrap();
        queue.push(e3.clone()).await.unwrap();
        let batch = queue.pending(2).await.unwrap();
        assert_eq!(batch, vec![e1.clone(), e2.clone()]);
        let all = queue.pending(3).await.unwrap();
        assert_eq!(all, vec![e1, e2, e3]);
    }

    #[tokio::test]
    async fn acknowledge_removes_only_confirmed_events() {
        let queue = EventQueue::new();
        let e1 = make_event();
        let e2 = make_event();
        let e3 = make_event();
        queue.push(e1.clone()).await.unwrap();
        queue.push(e2.clone()).await.unwrap();
        queue.push(e3.clone()).await.unwrap();
        let unknown_id = EventId(Uuid::new_v4());
        queue
            .acknowledge(&[e1.id, e3.id, unknown_id])
            .await
            .unwrap();
        let remaining = queue.pending(10).await.unwrap();
        assert_eq!(remaining, vec![e2]);
    }

    #[tokio::test]
    async fn wait_for_events_returns_immediately_if_not_empty() {
        let queue = EventQueue::new();
        queue.push(make_event()).await.unwrap();
        let res = tokio::time::timeout(Duration::from_millis(100), queue.wait_for_events()).await;
        assert!(
            res.is_ok(),
            "wait for events should not block when queue is already non-empty"
        );
    }

    #[tokio::test]
    async fn push_wakes_up_waiting_consumer() {
        let queue = Arc::new(EventQueue::new());
        let queue_clone = Arc::clone(&queue);
        let waiter = tokio::spawn(async move {
            queue_clone.wait_for_events().await.unwrap();
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        queue.push(make_event()).await.unwrap();
        let res = tokio::time::timeout(Duration::from_millis(100), waiter).await;
        assert!(res.is_ok(), "Consumer task was not woken up by push()");
    }
}
