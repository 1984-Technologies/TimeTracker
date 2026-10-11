use tokio::sync::mpsc;

use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

pub struct CollectionSupervisor {
    pub(crate) adapters: Vec<Box<dyn DesktopAdapter>>,
}

impl CollectionSupervisor {
    pub fn new(adapters: Vec<Box<dyn DesktopAdapter>>) -> Self {
        Self { adapters }
    }

    pub async fn start(&mut self, _output: mpsc::Sender<AdapterOutput>) -> Result<(), ClientError> {
        todo!()
    }

    pub async fn stop(&mut self) -> Result<(), ClientError> {
        todo!()
    }
}
