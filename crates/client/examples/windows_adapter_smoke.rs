//! Run from an interactive Windows session. No backend or supervisor is needed.
#[cfg(windows)]
#[tokio::main]
async fn main() -> Result<(), timetracker_client::ClientError> {
    use std::time::Duration;
    use timetracker_client::adapters::{AdapterOutput, windows};
    use tokio::sync::{mpsc, watch};

    let seconds = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "20".into())
        .parse::<u64>()?;
    let (output, mut events) = mpsc::channel(4096);
    let (shutdown, stop) = watch::channel(false);
    let mut tasks = Vec::new();
    let mut adapters = windows::default_adapters();
    // Short threshold makes active -> idle -> active easy to verify interactively.
    adapters[1] = Box::new(windows::IdleAdapter::new(Duration::from_secs(3)));
    for mut adapter in adapters {
        println!("{} probe: {:?}", adapter.id(), adapter.probe().await);
        let output = output.clone();
        let stop = stop.clone();
        tasks.push(tokio::spawn(async move { adapter.run(output, stop).await }));
    }
    drop(output);
    println!(
        "Switch windows, change a title, type, click and move the mouse; then leave input idle for 3 seconds."
    );
    let timeout = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            _ = &mut timeout => break,
            event = events.recv() => match event {
                Some(AdapterOutput::Status { adapter_id, status, .. }) => println!("{adapter_id}: {status:?}"),
                Some(AdapterOutput::Observation(event)) => println!("{} {:?}", event.occurred_at, event.kind),
                None => break,
            }
        }
    }
    shutdown.send(true)?;
    for task in tasks {
        tokio::time::timeout(Duration::from_secs(2), task).await???;
    }
    println!("All Windows adapters stopped cleanly.");
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This example requires an interactive Windows session.");
}
