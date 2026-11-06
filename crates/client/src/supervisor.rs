use timetracker_model::AdapterStatus;
use tokio::{
    sync::{mpsc, watch},
    task::{JoinError, JoinHandle},
};

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
        adapters: Vec<Box<dyn DesktopAdapter>>,
        output: mpsc::Sender<AdapterOutput>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let mut tasks = Vec::with_capacity(adapters.len());

        for (index, mut adapter) in adapters.into_iter().enumerate() {
            let adapter_id = adapter.id();
            let observation_type = adapter.observation_type();
            let task_output = output.clone();
            let task_shutdown = shutdown_rx.clone();

            tasks.push(tokio::spawn(async move {
                let status = adapter.probe().await;
                if task_output
                    .send(AdapterOutput::Status {
                        adapter_id,
                        observation_type,
                        status: status.clone(),
                    })
                    .await
                    .is_err()
                {
                    return (index, adapter);
                }

                if status != AdapterStatus::Available || *task_shutdown.borrow() {
                    return (index, adapter);
                }

                if let Err(error) = adapter.run(task_output.clone(), task_shutdown).await {
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

        Self {
            shutdown: Some(shutdown_tx),
            tasks,
        }
    }

    pub async fn stop(self) -> Result<(), ClientError> {
        let Self { shutdown, tasks } = self;

        if let Some(shutdown) = shutdown {
            shutdown.send_replace(true);
        }

        let mut first_join_error: Option<JoinError> = None;
        for (expected_index, task) in tasks.into_iter().enumerate() {
            match task.await {
                Ok((adapter_index, adapter)) => {
                    debug_assert_eq!(adapter_index, expected_index);
                    drop(adapter);
                }
                Err(error) => {
                    if first_join_error.is_none() {
                        first_join_error = Some(error);
                    }
                }
            }
        }

        if let Some(error) = first_join_error {
            return Err(Box::new(error));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
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

    async fn wait_for_run_count(run_count: &AtomicUsize, expected: usize) {
        for _ in 0..100 {
            if run_count.load(Ordering::SeqCst) == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(run_count.load(Ordering::SeqCst), expected);
    }

    #[tokio::test]
    async fn starts_available_adapters_and_stops_them() {
        let run_count = Arc::new(AtomicUsize::new(0));
        let (output_tx, mut output_rx) = mpsc::channel(8);
        let supervisor = CollectionSupervisor::start(
            vec![fake_adapter(
                AdapterStatus::Available,
                run_count.clone(),
                false,
            )],
            output_tx,
        )
        .await;

        let status = output_rx.recv().await.unwrap();
        assert!(matches!(
            status,
            AdapterOutput::Status {
                status: AdapterStatus::Available,
                ..
            }
        ));
        wait_for_run_count(&run_count, 1).await;
        supervisor.stop().await.unwrap();
        assert_eq!(run_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unavailable_adapters_report_status_and_are_not_run() {
        let run_count = Arc::new(AtomicUsize::new(0));
        let (output_tx, mut output_rx) = mpsc::channel(8);
        let supervisor = CollectionSupervisor::start(
            vec![fake_adapter(
                AdapterStatus::Unavailable {
                    reason: "test permission missing".to_owned(),
                },
                run_count.clone(),
                false,
            )],
            output_tx,
        )
        .await;

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
        let (output_tx, mut output_rx) = mpsc::channel(8);
        let supervisor = CollectionSupervisor::start(
            vec![fake_adapter(
                AdapterStatus::Available,
                run_count.clone(),
                true,
            )],
            output_tx,
        )
        .await;

        let available = output_rx.recv().await.unwrap();
        assert!(matches!(
            available,
            AdapterOutput::Status {
                status: AdapterStatus::Available,
                ..
            }
        ));
        let failed = output_rx.recv().await.unwrap();
        assert!(matches!(
            failed,
            AdapterOutput::Status {
                status: AdapterStatus::Unavailable { .. },
                ..
            }
        ));
        assert_eq!(run_count.load(Ordering::SeqCst), 1);
        supervisor.stop().await.unwrap();
    }
}
