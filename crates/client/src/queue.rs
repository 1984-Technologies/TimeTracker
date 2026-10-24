use timetracker_model::{Event, EventId};

use crate::ClientError;
use std::task::Poll;
use std::future::poll_fn;
use std::collections::HashMap;

#[derive(Clone)]
pub struct EventQueue {
    events: HashMap<EventId, Event>
}

impl EventQueue {
    pub fn new() -> EventQueue {
        EventQueue {
            events: HashMap::new()
        }
    }

    pub async fn push(&mut self, _event: Event) -> Result<(), ClientError> {
        self.events.insert(_event.id, _event);
        Ok(())
    }

    pub async fn wait_for_events(&self) -> Result<(), ClientError> {        
        poll_fn(|_| {
            if self.events.len() > 0 {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }).await;
        Ok(())
    }

    pub async fn pending(&self, _limit: usize) -> Result<Vec<Event>, ClientError> {
        Ok(self.events.values().take(_limit).map(|event| event.clone()).collect())
    }

    pub async fn acknowledge(&mut self, _ids: &[EventId]) -> Result<(), ClientError> {
        for id in _ids {
            self.events.remove(id);
        };
        Ok(())
    }
}
