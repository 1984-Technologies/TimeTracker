use std::{io, mem};

use timetracker_model::AdapterStatus;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

pub struct CollectionSupervisor {
    pub(crate) adapters: Vec<Box<dyn DesktopAdapter>>,
    shutdown: Option<watch::Sender<bool>>,
    tasks: Vec<JoinHandle<(usize, Box<dyn DesktopAdapter>)>>,
}

impl CollectionSupervisor {
    pub fn new(adapters: Vec<Box<dyn DesktopAdapter>>) -> Self {
        Self {
            adapters,
            shutdown: None,
            tasks: Vec::new(),
        }
    }

    #[cfg(target_os = "macos")]
    pub fn for_current_desktop() -> Self {
        Self::new(crate::adapters::default_macos_adapters())
    }

    pub async fn start(&mut self, output: mpsc::Sender<AdapterOutput>) -> Result<(), ClientError> {
        if self.shutdown.is_some() {
            return Err(Box::new(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "collection supervisor is already running; call stop before start",
            )));
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        self.shutdown = Some(shutdown_tx);

        let adapters = mem::take(&mut self.adapters);
        for (index, adapter) in adapters.into_iter().enumerate() {
            let adapter_id = adapter.id();
            let observation_type = adapter.observation_type();
            let task_output = output.clone();
            let task_shutdown = shutdown_rx.clone();

            self.tasks.push(tokio::spawn(async move {
                let mut adapter = adapter;
                let status = adapter.probe().await;

                let _ = task_output
                    .send(AdapterOutput::Status {
                        adapter_id,
                        observation_type,
                        status: status.clone(),
                    })
                    .await;

                if status == AdapterStatus::Available
                    && let Err(error) = adapter.run(task_output.clone(), task_shutdown).await
                {
                    let _ = task_output
                        .send(AdapterOutput::Status {
                            adapter_id,
                            observation_type,
                            status: AdapterStatus::Unavailable {
                                reason: format!("adapter run failed: {error}"),
                            },
                        })
                        .await;
                }

                (index, adapter)
            }));
        }

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<(), ClientError> {
        let Some(shutdown) = self.shutdown.take() else {
            return Ok(());
        };
        shutdown.send_replace(true);
        drop(shutdown);

        let tasks = mem::take(&mut self.tasks);
        let mut returned_adapters = Vec::with_capacity(tasks.len());
        let mut first_join_error = None;
        for task in tasks {
            match task.await {
                Ok(adapter) => returned_adapters.push(adapter),
                Err(error) if first_join_error.is_none() => first_join_error = Some(error),
                Err(_) => {}
            }
        }
        returned_adapters.sort_unstable_by_key(|(index, _)| *index);
        self.adapters = returned_adapters
            .into_iter()
            .map(|(_, adapter)| adapter)
            .collect();

        if let Some(error) = first_join_error {
            return Err(Box::new(error));
        }
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.shutdown.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use std::io;
    use timetracker_model::{AdapterStatus, ObservationType};
    use tokio::sync::{mpsc, watch};

    use super::CollectionSupervisor;
    use crate::{
        ClientError,
        adapters::{AdapterOutput, DesktopAdapter},
    };

    struct FakeAdapter {
        id: &'static str,
        probe_status: AdapterStatus,
        run_count: Arc<AtomicUsize>,
        fail_on_run: bool,
    }

    #[async_trait]
    impl DesktopAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            self.id
        }

        fn observation_type(&self) -> ObservationType {
            ObservationType::Idle
        }

        async fn probe(&self) -> AdapterStatus {
            self.probe_status.clone()
        }

        async fn run(
            &mut self,
            _output: mpsc::Sender<AdapterOutput>,
            mut shutdown: watch::Receiver<bool>,
        ) -> Result<(), ClientError> {
            self.run_count.fetch_add(1, Ordering::SeqCst);
            if self.fail_on_run {
                return Err(Box::new(io::Error::other(
                    "synthetic event handler failure",
                )));
            }

            loop {
                if *shutdown.borrow() {
                    return Ok(());
                }
                if shutdown.changed().await.is_err() {
                    return Ok(());
                }
            }
        }
    }

    fn fake_adapter(
        probe_status: AdapterStatus,
        run_count: Arc<AtomicUsize>,
        fail_on_run: bool,
    ) -> Box<dyn DesktopAdapter> {
        Box::new(FakeAdapter {
            id: "test.fake-adapter",
            probe_status,
            run_count,
            fail_on_run,
        })
    }

    #[tokio::test]
    async fn starts_available_adapters_and_stops_them() {
        let run_count = Arc::new(AtomicUsize::new(0));
        let mut supervisor = CollectionSupervisor::new(vec![fake_adapter(
            AdapterStatus::Available,
            run_count.clone(),
            false,
        )]);
        let (output_tx, _output_rx) = mpsc::channel(8);

        supervisor.start(output_tx.clone()).await.unwrap();
        assert!(supervisor.is_running());
        assert!(supervisor.start(output_tx.clone()).await.is_err());

        supervisor.stop().await.unwrap();
        assert!(!supervisor.is_running());
        assert_eq!(run_count.load(Ordering::SeqCst), 1);

        supervisor.start(output_tx).await.unwrap();
        supervisor.stop().await.unwrap();
        assert_eq!(run_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn unavailable_adapters_report_status_and_are_not_run() {
        let run_count = Arc::new(AtomicUsize::new(0));
        let mut supervisor = CollectionSupervisor::new(vec![fake_adapter(
            AdapterStatus::Unavailable {
                reason: "test permission missing".to_owned(),
            },
            run_count.clone(),
            false,
        )]);
        let (output_tx, mut output_rx) = mpsc::channel(8);

        supervisor.start(output_tx).await.unwrap();
        let output = output_rx.recv().await.unwrap();
        assert!(matches!(
            output,
            AdapterOutput::Status {
                status: AdapterStatus::Unavailable { .. },
                ..
            }
        ));
        assert_eq!(run_count.load(Ordering::SeqCst), 0);
        supervisor.stop().await.unwrap();
    }

    #[tokio::test]
    async fn run_errors_are_reported_as_status() {
        let run_count = Arc::new(AtomicUsize::new(0));
        let mut supervisor = CollectionSupervisor::new(vec![fake_adapter(
            AdapterStatus::Available,
            run_count,
            true,
        )]);
        let (output_tx, mut output_rx) = mpsc::channel(8);

        supervisor.start(output_tx).await.unwrap();
        let initial_status = output_rx.recv().await.unwrap();
        assert!(matches!(
            initial_status,
            AdapterOutput::Status {
                status: AdapterStatus::Available,
                ..
            }
        ));
        let error_status = output_rx.recv().await.unwrap();
        assert!(matches!(
            error_status,
            AdapterOutput::Status {
                status: AdapterStatus::Unavailable { reason },
                ..
            } if reason.contains("synthetic event handler failure")
        ));

        supervisor.stop().await.unwrap();
    }
}
