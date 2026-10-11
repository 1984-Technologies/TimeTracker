use timetracker_model::{Event, EventId};

use crate::ClientError;

pub struct EventQueue;

impl EventQueue {
    pub async fn push(&self, _event: Event) -> Result<(), ClientError> {
        todo!()
    }

    pub async fn wait_for_events(&self) -> Result<(), ClientError> {
        todo!()
    }

    pub async fn pending(&self, _limit: usize) -> Result<Vec<Event>, ClientError> {
        todo!()
    }

    pub async fn acknowledge(&self, _ids: &[EventId]) -> Result<(), ClientError> {
        todo!()
    }
}
