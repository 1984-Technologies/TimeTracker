#[cfg(target_os = "macos")]
use std::{io, sync::mpsc as std_mpsc, thread, time::Duration};

#[cfg(target_os = "macos")]
use nsworkspace::{Event, Monitor, NotificationListener};
#[cfg(target_os = "macos")]
use timetracker_client::{
    adapters::{AdapterOutput, default_adapters},
    supervisor::CollectionSupervisor,
};
#[cfg(target_os = "macos")]
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    let Some((monitor, workspace_events, stop_monitor_tx)) = Monitor::new() else {
        eprintln!("NSWorkspace monitor must be created on the main thread");
        return;
    };

    monitor.subscribe(NotificationListener::DidActivateApplication);
    let initial_app_id = monitor.get_active_application();
    let (focused_app_tx, focused_app_rx) = mpsc::unbounded_channel();

    let event_forwarder = thread::spawn(move || {
        loop {
            if focused_app_tx.is_closed() {
                break;
            }

            match workspace_events.recv_timeout(Duration::from_millis(100)) {
                Ok(Event::DidActivateApplication(app_id)) => {
                    if focused_app_tx.send(app_id).is_err() {
                        break;
                    }
                }
                Ok(Event::Error(error)) => eprintln!("[NSWorkspace ERROR] {error:?}"),
                Ok(_) => {}
                Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let (output_tx, mut output_rx) = mpsc::channel(512);
    let supervisor =
        CollectionSupervisor::start(default_adapters(focused_app_rx, initial_app_id), output_tx)
            .await;

    let output_task = tokio::spawn(async move {
        while let Some(output) = output_rx.recv().await {
            match output {
                AdapterOutput::Observation(observation) => {
                    println!("[OBS] {observation:?}");
                }
                AdapterOutput::Status {
                    adapter_id,
                    observation_type,
                    status,
                } => {
                    println!("[STATUS] {adapter_id} ({observation_type:?}): {status:?}");
                }
            }
        }
    });

    let stop_sender = stop_monitor_tx.clone();
    thread::spawn(move || {
        println!("\nPress Enter to stop all adapters.");
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
        let _ = stop_sender.send(());
    });

    monitor.run();

    if let Err(error) = supervisor.stop().await {
        eprintln!("[ERROR] failed to stop collection cleanly: {error}");
    }
    let _ = output_task.await;
    let _ = event_forwarder.join();
    println!("Smoke test finished.");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macos_adapter_smoke is only supported on macOS");
}
