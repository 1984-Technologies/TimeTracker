use std::time::Duration;

use async_trait::async_trait;
use time::OffsetDateTime;
use timetracker_model::{AdapterStatus, Observation, ObservationType};
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
    time::{Instant, MissedTickBehavior, interval_at},
};

use super::{AdapterOutput, DesktopAdapter, activity_window::ActivityWindow};

use crate::ClientError;

const CHILD_CHANNEL_CAPACITY: usize = 1024;

pub const DEFAULT_ACTIVITY_WINDOW_DURATION: Duration = Duration::from_secs(10);

pub struct MouseKeyboardActivityAdapter {
    children: Vec<Box<dyn DesktopAdapter + Sync>>,
    window_duration: Duration,
}

impl MouseKeyboardActivityAdapter {
    pub fn new(
        keyboard: Box<dyn DesktopAdapter + Sync>,
        mouse_click: Box<dyn DesktopAdapter + Sync>,
        mouse_move: Box<dyn DesktopAdapter + Sync>,
        window_duration: Duration,
    ) -> Self {
        assert!(
            !window_duration.is_zero(),
            "activity window duration must be greater than zero"
        );

        Self {
            children: vec![keyboard, mouse_click, mouse_move],
            window_duration,
        }
    }
}

#[async_trait]
impl DesktopAdapter for MouseKeyboardActivityAdapter {
    fn id(&self) -> &'static str {
        "input-activity.meta-adapter"
    }

    fn observation_type(&self) -> ObservationType {
        ObservationType::MouseKeyboardActivity
    }

    async fn probe(&self) -> AdapterStatus {
        let mut unavailable = Vec::new();

        for child in &self.children {
            if let AdapterStatus::Unavailable { reason } = child.probe().await {
                unavailable.push(format!(
                    "{} ({:?}): {}",
                    child.id(),
                    child.observation_type(),
                    reason
                ));
            }
        }

        if unavailable.is_empty() {
            AdapterStatus::Available
        } else {
            AdapterStatus::Unavailable {
                reason: format!(
                    "Some input adapters are unavailable: {}",
                    unavailable.join("; ")
                ),
            }
        }
    }

    async fn run(
        &mut self,
        output: mpsc::Sender<AdapterOutput>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        if *shutdown.borrow() || shutdown.has_changed().is_err() || output.is_closed() {
            return Ok(());
        }

        let (child_shutdown_tx, child_shutdown_rx) = watch::channel(false);
        let (child_output_tx, mut child_output_rx) = mpsc::channel(CHILD_CHANNEL_CAPACITY);

        let mut tasks = JoinSet::new();

        let children = std::mem::take(&mut self.children);
        let child_count = children.len();

        for (index, mut child) in children.into_iter().enumerate() {
            let child_id = child.id();
            let observation_type = child.observation_type();

            let task_output = child_output_tx.clone();
            let task_shutdown = child_shutdown_rx.clone();

            tasks.spawn(async move {
                let result = child.run(task_output.clone(), task_shutdown.clone()).await;

                let shutdown_requested = *task_shutdown.borrow();

                let failure_reason = match result {
                    Err(error) => Some(format!("adapter run failed: {error}")),
                    Ok(()) if !shutdown_requested => {
                        Some("adapter stopped unexpectedly".to_owned())
                    }
                    Ok(()) => None,
                };

                if let Some(reason) = failure_reason {
                    let _ = task_output
                        .send(AdapterOutput::Status {
                            adapter_id: child_id,
                            observation_type,
                            status: AdapterStatus::Unavailable { reason },
                        })
                        .await;
                }

                (index, child)
            });
        }

        drop(child_output_tx);

        let mut restored_children = Vec::with_capacity(child_count);
        let mut first_join_error = None;

        let mut window = ActivityWindow::default();
        let mut window_started = Instant::now();

        let mut ticker = interval_at(window_started + self.window_duration, self.window_duration);

        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut stopping_children = false;
        let mut can_send = true;
        let mut child_failure_reason: Option<String> = None;

        loop {
            if stopping_children {
                child_shutdown_tx.send_replace(true);

                while let Some(event) = child_output_rx.recv().await {
                    if can_send {
                        match handle_child_output(event, &mut window, &output).await {
                            ChildOutputResult::Continue => {}

                            ChildOutputResult::ChildUnavailable(reason) => {
                                if child_failure_reason.is_none() {
                                    child_failure_reason = Some(reason);
                                }
                            }

                            ChildOutputResult::OutputClosed => {
                                can_send = false;
                            }
                        }
                    }
                }

                break;
            }

            tokio::select! {
                biased;

                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        stopping_children = true;
                    }
                }

                _ = output.closed(), if can_send => {
                    can_send = false;
                    stopping_children = true;
                }

                completed = tasks.join_next(), if !tasks.is_empty() => {
                    match completed {
                        Some(Ok((index, child))) => {
                            restored_children.push((index, child));
                        }
                        Some(Err(error)) => {
                            if first_join_error.is_none() {
                                first_join_error = Some(error);
                            }
                        }
                        None => {}
                    }

                    stopping_children = true;
                }

                event = child_output_rx.recv() => {
                    match event {
                        Some(event) => {
                            match handle_child_output(
                                event,
                                &mut window,
                                &output,
                            ).await {
                                ChildOutputResult::Continue => {}

                                ChildOutputResult::ChildUnavailable(reason) => {
                                    child_failure_reason = Some(reason);
                                    stopping_children = true;
                                }

                                ChildOutputResult::OutputClosed => {
                                    can_send = false;
                                    stopping_children = true;
                                }
                            }
                        }

                        None => {
                            break;
                        }
                    }
                }

                _ = ticker.tick() => {
                    let duration = window_started.elapsed();
                    let summary = window.summary_and_reset(duration);

                    window_started = Instant::now();

                    if output
                        .send(AdapterOutput::Observation(Observation {
                            occurred_at: OffsetDateTime::now_utc(),
                            kind: summary,
                        }))
                        .await
                        .is_err()
                    {
                        can_send = false;
                        stopping_children = true;
                    }
                }
            }
        }

        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((index, child)) => {
                    restored_children.push((index, child));
                }

                Err(error) => {
                    if first_join_error.is_none() {
                        first_join_error = Some(error);
                    }
                }
            }
        }

        restored_children.sort_by_key(|(index, _)| *index);

        self.children = restored_children
            .into_iter()
            .map(|(_, child)| child)
            .collect();

        if can_send && !window.is_empty() {
            let summary = window.summary_and_reset(window_started.elapsed());

            let _ = output
                .send(AdapterOutput::Observation(Observation {
                    occurred_at: OffsetDateTime::now_utc(),
                    kind: summary,
                }))
                .await;
        }

        if let Some(error) = first_join_error {
            return Err(Box::new(error));
        }

        if let Some(reason) = child_failure_reason {
            return Err(reason.into());
        }

        Ok(())
    }
}

enum ChildOutputResult {
    Continue,
    ChildUnavailable(String),
    OutputClosed,
}

async fn handle_child_output(
    event: AdapterOutput,
    window: &mut ActivityWindow,
    output: &mpsc::Sender<AdapterOutput>,
) -> ChildOutputResult {
    match event {
        AdapterOutput::Observation(observation) => {
            window.record(&observation.kind);
            ChildOutputResult::Continue
        }

        AdapterOutput::Status {
            adapter_id,
            observation_type,
            status,
        } => {
            let failure_reason = match &status {
                AdapterStatus::Available => None,
                AdapterStatus::Unavailable { reason } => Some(format!(
                    "Child adapter {adapter_id} ({observation_type:?}) became unavailable: {reason}"
                )),
            };

            if output
                .send(AdapterOutput::Status {
                    adapter_id,
                    observation_type,
                    status,
                })
                .await
                .is_err()
            {
                return ChildOutputResult::OutputClosed;
            }

            match failure_reason {
                Some(reason) => ChildOutputResult::ChildUnavailable(reason),
                None => ChildOutputResult::Continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::sync::{mpsc, watch};

    use timetracker_model::ObservationKind;

    struct FakeAdapter {
        id: &'static str,
        observation_type: ObservationType,
        probe_status: AdapterStatus,
        events: Vec<ObservationKind>,
        started: mpsc::UnboundedSender<&'static str>,
        exit_unexpectedly: bool,
    }

    #[async_trait]
    impl DesktopAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            self.id
        }

        fn observation_type(&self) -> ObservationType {
            self.observation_type
        }

        async fn probe(&self) -> AdapterStatus {
            self.probe_status.clone()
        }

        async fn run(
            &mut self,
            output: mpsc::Sender<AdapterOutput>,
            mut shutdown: watch::Receiver<bool>,
        ) -> Result<(), ClientError> {
            for kind in self.events.drain(..) {
                let observation = AdapterOutput::Observation(Observation {
                    occurred_at: OffsetDateTime::now_utc(),
                    kind,
                });

                if output.send(observation).await.is_err() {
                    return Ok(());
                }
            }

            let _ = self.started.send(self.id);

            if self.exit_unexpectedly {
                return Ok(());
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

    struct PanickingAdapter;

    #[async_trait]
    impl DesktopAdapter for PanickingAdapter {
        fn id(&self) -> &'static str {
            "test.panicking"
        }

        fn observation_type(&self) -> ObservationType {
            ObservationType::Keyboard
        }

        async fn probe(&self) -> AdapterStatus {
            AdapterStatus::Available
        }

        async fn run(
            &mut self,
            _output: mpsc::Sender<AdapterOutput>,
            _shutdown: watch::Receiver<bool>,
        ) -> Result<(), ClientError> {
            panic!("synthetic child panic");
        }
    }

    fn fake_adapter(
        id: &'static str,
        observation_type: ObservationType,
        events: Vec<ObservationKind>,
        probe_status: AdapterStatus,
        started: mpsc::UnboundedSender<&'static str>,
        exit_unexpectedly: bool,
    ) -> Box<dyn DesktopAdapter + Sync> {
        Box::new(FakeAdapter {
            id,
            observation_type,
            probe_status,
            events,
            started,
            exit_unexpectedly,
        })
    }

    fn make_adapter(
        window_duration: Duration,
        started: mpsc::UnboundedSender<&'static str>,
        keyboard_status: AdapterStatus,
        keyboard_exits_unexpectedly: bool,
    ) -> MouseKeyboardActivityAdapter {
        let keyboard = fake_adapter(
            "test.keyboard",
            ObservationType::Keyboard,
            vec![
                ObservationKind::KeyboardButtonPressed {
                    key: "A".to_owned(),
                },
                ObservationKind::KeyboardButtonPressed {
                    key: "B".to_owned(),
                },
            ],
            keyboard_status,
            started.clone(),
            keyboard_exits_unexpectedly,
        );

        let mouse_click = fake_adapter(
            "test.mouse-click",
            ObservationType::MouseClick,
            vec![ObservationKind::MouseClicked {
                button: "Left".to_owned(),
                x: Some(100),
                y: Some(200),
            }],
            AdapterStatus::Available,
            started.clone(),
            false,
        );

        let mouse_move = fake_adapter(
            "test.mouse-move",
            ObservationType::MouseMove,
            vec![
                ObservationKind::MouseMoved { x: 10, y: 20 },
                ObservationKind::MouseMoved { x: 20, y: 30 },
                ObservationKind::MouseMoved { x: 30, y: 40 },
            ],
            AdapterStatus::Available,
            started,
            false,
        );

        MouseKeyboardActivityAdapter::new(keyboard, mouse_click, mouse_move, window_duration)
    }

    async fn wait_for_children(started: &mut mpsc::UnboundedReceiver<&'static str>) {
        let mut ids = Vec::new();

        for _ in 0..3 {
            let id = tokio::time::timeout(Duration::from_secs(2), started.recv())
                .await
                .expect("child adapter did not start")
                .expect("started channel was closed");

            ids.push(id);
        }

        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"test.keyboard"));
        assert!(ids.contains(&"test.mouse-click"));
        assert!(ids.contains(&"test.mouse-move"));
    }

    fn check_summary(
        event: Option<AdapterOutput>,
        expected_keys: u64,
        expected_clicks: u64,
        expected_moves: u64,
    ) -> u64 {
        let Some(AdapterOutput::Observation(observation)) = event else {
            panic!("expected an activity summary observation");
        };

        let ObservationKind::MouseKeyboardActivitySummary {
            keypress_count,
            mouse_click_count,
            mouse_move_count,
            window_duration_ms,
        } = observation.kind
        else {
            panic!("unexpected observation kind");
        };

        assert_eq!(keypress_count, expected_keys);
        assert_eq!(mouse_click_count, expected_clicks);
        assert_eq!(mouse_move_count, expected_moves);

        window_duration_ms
    }

    #[tokio::test]
    async fn probe_reports_unavailable_child() {
        let (started_tx, _started_rx) = mpsc::unbounded_channel();

        let adapter = make_adapter(
            Duration::from_secs(10),
            started_tx,
            AdapterStatus::Unavailable {
                reason: "permission denied".to_owned(),
            },
            false,
        );

        let status = adapter.probe().await;

        assert!(matches!(
            status,
            AdapterStatus::Unavailable { ref reason }
                if reason.contains("permission denied")
        ));
    }

    #[tokio::test]
    async fn periodically_emits_aggregated_counts() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();

        let mut adapter = make_adapter(
            Duration::from_millis(200),
            started_tx,
            AdapterStatus::Available,
            false,
        );

        let (output_tx, mut output_rx) = mpsc::channel(16);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let task = tokio::spawn(async move { adapter.run(output_tx, shutdown_rx).await });

        wait_for_children(&mut started_rx).await;

        let event = tokio::time::timeout(Duration::from_secs(2), output_rx.recv())
            .await
            .expect("periodic summary was not emitted");

        let duration = check_summary(event, 2, 1, 3);
        assert!(duration >= 200);

        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("meta-adapter did not stop")
            .unwrap()
            .unwrap();

        assert!(output_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn shutdown_flushes_remaining_activity() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();

        let mut adapter = make_adapter(
            Duration::from_secs(30),
            started_tx,
            AdapterStatus::Available,
            false,
        );

        let (output_tx, mut output_rx) = mpsc::channel(16);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let task = tokio::spawn(async move { adapter.run(output_tx, shutdown_rx).await });

        wait_for_children(&mut started_rx).await;

        shutdown_tx.send(true).unwrap();

        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("meta-adapter did not finish shutdown")
            .unwrap()
            .unwrap();

        check_summary(output_rx.recv().await, 2, 1, 3);

        assert!(output_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn unexpected_child_exit_is_reported() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();

        let mut adapter = make_adapter(
            Duration::from_secs(30),
            started_tx,
            AdapterStatus::Available,
            true,
        );

        let (output_tx, mut output_rx) = mpsc::channel(16);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);

        let task = tokio::spawn(async move { adapter.run(output_tx, shutdown_rx).await });

        wait_for_children(&mut started_rx).await;

        let mut unavailable_reported = false;

        while let Some(event) = tokio::time::timeout(Duration::from_secs(2), output_rx.recv())
            .await
            .expect("expected an unavailable status")
        {
            if let AdapterOutput::Status {
                adapter_id,
                status: AdapterStatus::Unavailable { reason },
                ..
            } = event
            {
                assert_eq!(adapter_id, "test.keyboard");
                assert!(reason.contains("stopped unexpectedly"));
                unavailable_reported = true;
                break;
            }
        }

        assert!(unavailable_reported);

        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("meta-adapter did not terminate")
            .unwrap();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn panicking_child_stops_other_children() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();

        let keyboard: Box<dyn DesktopAdapter + Sync> = Box::new(PanickingAdapter);

        let mouse_click = fake_adapter(
            "test.mouse-click",
            ObservationType::MouseClick,
            vec![],
            AdapterStatus::Available,
            started_tx.clone(),
            false,
        );

        let mouse_move = fake_adapter(
            "test.mouse-move",
            ObservationType::MouseMove,
            vec![],
            AdapterStatus::Available,
            started_tx,
            false,
        );

        let mut adapter = MouseKeyboardActivityAdapter::new(
            keyboard,
            mouse_click,
            mouse_move,
            Duration::from_secs(30),
        );

        let (output_tx, _output_rx) = mpsc::channel(16);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);

        let task = tokio::spawn(async move { adapter.run(output_tx, shutdown_rx).await });

        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(2), started_rx.recv())
                .await
                .expect("child adapter did not start")
                .expect("started channel closed");
        }

        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("meta-adapter did not stop after child panic")
            .expect("meta-adapter task panicked");

        assert!(result.is_err());
    }
}
