#[cfg(target_os = "macos")]
use std::{io, thread};

#[cfg(target_os = "macos")]
use timetracker_client::{
    adapters::{AdapterOutput, MacOsAdapterRuntime},
    supervisor::CollectionSupervisor,
};
#[cfg(target_os = "macos")]
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    let mut runtime = match MacOsAdapterRuntime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("[ERROR] failed to initialize macOS adapter runtime: {error}");
            return;
        }
    };

    let adapters = match runtime.default_adapters() {
        Ok(adapters) => adapters,
        Err(error) => {
            eprintln!("[ERROR] failed to create adapters: {error}");
            return;
        }
    };

    let (output_tx, mut output_rx) = mpsc::channel(512);
    let supervisor = CollectionSupervisor::start(adapters, output_tx).await;

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

    let shutdown_handle = runtime.shutdown_handle();
    thread::spawn(move || {
        println!("\nPress Enter to stop all adapters.");
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
        let _ = shutdown_handle.request_shutdown();
    });

    runtime.run();

    if let Err(error) = supervisor.stop().await {
        eprintln!("[ERROR] failed to stop collection cleanly: {error}");
    }
    if let Err(error) = output_task.await {
        eprintln!("[ERROR] output task failed: {error}");
    }

    println!("Smoke test finished.");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macos_adapter_smoke is only supported on macOS");
}
