use std::sync::Arc;
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

	let (_shutdown_sender, shutdown_receiver) = watch::channel(false);
	let mut shutdown_receiver1 = shutdown_receiver.clone();
	let supervisor_handle = tokio::spawn(async move {
		let collection_supervisor = CollectionSupervisor::start(adapters, send).await;
		let _ = shutdown_receiver1.wait_for(|val| *val == true).await;
		let _ = collection_supervisor.stop().await;
	});

	let event_service1 = event_service.clone();
	let record_outputs_handle = tokio::spawn(async move {
		let _ = event_service1.record_outputs(recv).await;
	});

	let event_service2 = event_service.clone();
	let upload_loop_handle = tokio::spawn(async move {
		let _ = event_service2.upload_loop(shutdown_receiver).await;
	});

	let _ = supervisor_handle.await;
	let _ = record_outputs_handle.await;
	let _ = upload_loop_handle.await;

	todo!("Not implemented: receive and handle shutdown")
}