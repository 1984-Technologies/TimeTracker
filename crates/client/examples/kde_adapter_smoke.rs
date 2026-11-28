#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<(), timetracker_client::ClientError> {
    use std::time::Duration;
    use timetracker_client::adapters::{
        AdapterOutput, default_adapters,
        kde::{FocusedApplicationAdapter, IdleAdapter},
    };
    use tokio::{
        sync::{mpsc, watch},
        task::JoinSet,
    };

    let mut seconds = 10u64;
    let mut collect_titles = false;
    for arg in std::env::args().skip(1) {
        if arg == "--titles" {
            collect_titles = true;
        } else {
            seconds = arg.parse::<u64>()?;
        }
    }
    if seconds == 0 {
        return Err("smoke duration must be positive".into());
    }

    let (output, mut events) = mpsc::channel(256);
    let (stop, shutdown) = watch::channel(false);
    let mut tasks = JoinSet::new();
    let mut adapters = default_adapters()?;
    if adapters.is_empty() {
        return Err("no adapters detected for the current desktop session".into());
    }
    if collect_titles {
        adapters[0] = Box::new(FocusedApplicationAdapter::new(true));
    }
    // Short idle threshold so IdleChanged transitions are visible during the smoke test.
    adapters[1] = Box::new(IdleAdapter::new(Duration::from_secs(2)));

    for mut adapter in adapters {
        let output = output.clone();
        let shutdown = shutdown.clone();
        tasks.spawn(async move { adapter.run(output, shutdown).await });
    }
    drop(output);

    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = events.recv() => match event {
                Some(AdapterOutput::Observation(observation)) => println!("[OBS] {observation:?}"),
                Some(AdapterOutput::Status { adapter_id, observation_type, status }) => {
                    println!("[STATUS] {adapter_id} ({observation_type:?}): {status:?}")
                }
                None => break,
            }
        }
    }

    stop.send(true)?;
    while let Some(result) = tasks.join_next().await {
        result??;
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("kde_adapter_smoke requires Linux and a running KDE Plasma Wayland session");
}
