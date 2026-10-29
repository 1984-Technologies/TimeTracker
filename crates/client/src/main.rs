use std::sync::Arc;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::sync::watch;

use timetracker_client::{event_service::ClientEventService, identity::InstallationIdentity, queue::EventQueue, supervisor::CollectionSupervisor, upload::GrpcUploadClient};

#[tokio::main]
async fn main() {
	let device_id = InstallationIdentity.load_or_create().await.expect("Could not obtain device id");
	let event_queue = EventQueue::new();
	let upload_client = GrpcUploadClient::new();
	let buffer_capacity = 100;
	let event_service = Arc::new(ClientEventService::new(device_id, Arc::new(event_queue), upload_client));
	let adapters = Vec::new(); // TODO: create adapters

	let (send, recv) = mpsc::channel(buffer_capacity);

	let (shutdown_sender, shutdown_receiver) = watch::channel(false);
	let mut shutdown_receiver1 = shutdown_receiver.clone();
	tokio::spawn(async move {
		let collection_supervisor = CollectionSupervisor::start(adapters, send).await;
		let _ = shutdown_receiver1.wait_for(|val| *val == true).await;
		let _ = collection_supervisor.stop().await;
	});
	let event_service1 = event_service.clone();
	tokio::spawn(async move {
		let _ = event_service1.record_outputs(recv).await;
	});
	let event_service2 = event_service.clone();
	tokio::spawn(async move {
		let _ = event_service2.upload_loop(shutdown_receiver).await;
	});

	match signal::ctrl_c().await {
        Ok(()) => {},
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        },
    }

    let _ = shutdown_sender.send(true);
}