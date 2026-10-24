#[cfg(target_os = "macos")]
use std::io;

#[cfg(target_os = "macos")]
use timetracker_client::{adapters::AdapterOutput, supervisor::CollectionSupervisor};
#[cfg(target_os = "macos")]
use tokio::sync::{mpsc, oneshot};

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    let mut supervisor = CollectionSupervisor::for_current_desktop();
    let (output_tx, mut output_rx) = mpsc::channel(512);

    if let Err(error) = supervisor.start(output_tx).await {
        eprintln!("[ERROR] failed to start collection: {error}");
        return;
    }

    let (stop_tx, mut stop_rx) = oneshot::channel();
    std::thread::spawn(move || {
        println!("\nНажми Enter для остановки всех адаптеров.");
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
        let _ = stop_tx.send(());
    });

    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            output = output_rx.recv() => {
                match output {
                    Some(AdapterOutput::Observation(observation)) => {
                        println!("[OBS] {observation:?}");
                    }
                    Some(AdapterOutput::Status {
                        adapter_id,
                        observation_type,
                        status,
                    }) => {
                        println!("[STATUS] {adapter_id} ({observation_type:?}): {status:?}");
                    }
                    None => break,
                }
            }
        }
    }

    if let Err(error) = supervisor.stop().await {
        eprintln!("[ERROR] failed to stop collection cleanly: {error}");
    }
    println!("Проверка завершена.");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macos_adapter_smoke is only supported on macOS");
}
