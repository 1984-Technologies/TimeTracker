use std::thread::JoinHandle;
use tokio::sync::mpsc;
use tokio::sync::watch;

use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

pub struct CollectionSupervisor {
    shutdown: Option<watch::Sender<bool>>,
    tasks: Vec<JoinHandle<(usize, Box<dyn DesktopAdapter>)>>,
}

impl CollectionSupervisor {
    pub async fn start(
        _adapters: Vec<Box<dyn DesktopAdapter>>,
        _output: mpsc::Sender<AdapterOutput>
    ) -> Self {
        todo!()
    }

    pub async fn stop(self) -> Result<(), ClientError> {
        todo!()
    }
}
