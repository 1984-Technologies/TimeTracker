use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use arboard::Clipboard;
use qrcode::{QrCode, render::svg};
use serde::Serialize;
use tauri::{
    Manager, RunEvent, State, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tokio::sync::watch;

use super::{
    RegistrationUiState,
    collection::{run_collection, wait_for_shutdown_signal},
    flow::{Status, await_registration},
};
use crate::{
    ClientError,
    adapters::DesktopAdapter,
    registration::{ClientAuthService, DeviceCredentialStore, HttpAuthClient},
};

#[cfg(target_os = "macos")]
thread_local! {
    static MAC_RUNTIME: std::cell::RefCell<Option<crate::adapters::MacOsAdapterRuntime>> = const { std::cell::RefCell::new(None) };
}

struct DesktopState {
    url: String,
    qr_svg: String,
    device_id: String,
    status: watch::Sender<Status>,
    retry: watch::Sender<u64>,
    shutdown: watch::Sender<bool>,
    finished: AtomicBool,
    // Retain clipboard ownership on Linux until another application reads it.
    clipboard: Mutex<Option<Clipboard>>,
}

#[derive(Serialize)]
struct Snapshot {
    #[serde(flatten)]
    status: Status,
    enrollment_url: String,
    qr_svg: String,
    device_id: String,
}

#[tauri::command]
fn registration_snapshot(state: State<'_, DesktopState>) -> Snapshot {
    Snapshot {
        status: state.status.borrow().clone(),
        enrollment_url: state.url.clone(),
        qr_svg: state.qr_svg.clone(),
        device_id: state.device_id.clone(),
    }
}

#[tauri::command]
fn retry_registration(state: State<'_, DesktopState>) {
    if !state.finished.load(Ordering::SeqCst) {
        state
            .retry
            .send_modify(|version| *version = version.wrapping_add(1));
    }
}

#[tauri::command]
fn open_enrollment_link(state: State<'_, DesktopState>) -> Result<(), &'static str> {
    open::that_detached(&state.url)
        .map_err(|_| "Could not open the browser. Copy the link instead.")
}

#[tauri::command]
fn copy_enrollment_link(state: State<'_, DesktopState>) -> Result<(), &'static str> {
    let mut clipboard = state
        .clipboard
        .lock()
        .map_err(|_| "Clipboard unavailable. Select and copy the link below.")?;
    if clipboard.is_none() {
        *clipboard = Some(
            Clipboard::new()
                .map_err(|_| "Clipboard unavailable. Select and copy the link below.")?,
        );
    }
    clipboard
        .as_mut()
        .unwrap()
        .set_text(state.url.clone())
        .map_err(|_| "Could not copy the link. Select and copy it below.")
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main")
        && let Err(error) = window
            .show()
            .and_then(|_| window.unminimize())
            .and_then(|_| window.set_focus())
    {
        eprintln!("Could not reopen TimeTracker: {error}");
    }
}

fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open-client", "Open TimeTracker", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit-client", "Quit TimeTracker", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::Io(std::io::Error::other("Missing TimeTracker tray icon")))?;
    TrayIconBuilder::with_id("timetracker")
        .icon(icon)
        .tooltip("TimeTracker")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open-client" => show_main_window(app),
            // ExitRequested keeps the runtime alive until collection has drained.
            "quit-client" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

async fn create_adapters(
    app: &tauri::AppHandle,
) -> Result<Vec<Box<dyn DesktopAdapter>>, ClientError> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let result = crate::adapters::default_adapters().map(|(runtime, adapters)| {
                // Tauri drives NSApplication on the main thread. Running the monitor's
                // second event loop would replace Tauri's delegate and break shutdown.
                MAC_RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
                adapters
            });
            let _ = tx.send(result);
        })?;
        rx.await?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        crate::adapters::default_adapters()
    }
}

async fn registration_and_collection(
    app: &tauri::AppHandle,
    auth: Arc<ClientAuthService>,
    mut retry: watch::Receiver<u64>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), ClientError> {
    let state = app.state::<DesktopState>();
    let approved = await_registration(
        || async {
            if !auth.check_registration().await?.registered {
                return Ok(false);
            }
            auth.access_token().await?;
            Ok(true)
        },
        &state.status,
        &mut retry,
        &mut shutdown,
    )
    .await;
    if !approved || *shutdown.borrow() {
        return Ok(());
    }
    let adapters = create_adapters(app).await?;
    if *shutdown.borrow() {
        return Ok(());
    }
    state.status.send_replace(Status::Registered);
    run_collection(
        adapters,
        auth,
        async move {
            let _ = shutdown.wait_for(|stopping| *stopping).await;
            Ok(())
        },
        || {
            state.status.send_replace(Status::Stopping);
        },
    )
    .await
}

pub fn run() -> Result<(), ClientError> {
    let runtime = tokio::runtime::Runtime::new()?;
    tauri::async_runtime::set(runtime.handle().clone());
    let auth = Arc::new(ClientAuthService::new(
        HttpAuthClient::new(
            std::env::var("TIMETRACKER_HTTP_URL")
                .unwrap_or_else(|_| "http://localhost:8239".into()),
        ),
        DeviceCredentialStore.load_or_create()?,
    ));
    let registration = RegistrationUiState {
        auth,
        frontend_url: std::env::var("TIMETRACKER_FRONTEND_URL")
            .unwrap_or_else(|_| "http://localhost:5173".into()),
    };
    let url = registration.enrollment_url()?;
    let qr_svg = QrCode::new(url.as_bytes())?
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .build();
    let (status, _) = watch::channel(Status::Checking);
    let (retry, retry_rx) = watch::channel(0);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let state = DesktopState {
        url,
        qr_svg,
        device_id: registration.auth.device_id().0.to_string(),
        status,
        retry,
        shutdown,
        finished: AtomicBool::new(false),
        clipboard: Mutex::new(None),
    };
    let app = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            registration_snapshot,
            retry_registration,
            open_enrollment_link,
            copy_enrollment_link
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    eprintln!("Could not hide TimeTracker to the tray: {error}");
                }
            }
        })
        .setup(move |app| {
            install_tray(app)?;
            let handle = app.handle().clone();
            let flow_handle = handle.clone();
            let signal_handle = handle.clone();
            let signal_task = tauri::async_runtime::spawn(async move {
                let _ = wait_for_shutdown_signal().await;
                signal_handle
                    .state::<DesktopState>()
                    .shutdown
                    .send_replace(true);
            });
            let flow = tauri::async_runtime::spawn(async move {
                registration_and_collection(&flow_handle, registration.auth, retry_rx, shutdown_rx)
                    .await
            });
            tauri::async_runtime::spawn(async move {
                let failed = !matches!(flow.await, Ok(Ok(())));
                signal_task.abort();
                let state = handle.state::<DesktopState>();
                state.status.send_replace(Status::Stopped { failed });
                state.finished.store(true, Ordering::SeqCst);
                if *state.shutdown.borrow() {
                    handle.exit(i32::from(failed));
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())?;
    let exit_code = app.run_return(|app, event| match event {
        RunEvent::ExitRequested { api, .. } => {
            let state = app.state::<DesktopState>();
            if !state.finished.load(Ordering::SeqCst) {
                api.prevent_exit();
                state.status.send_replace(Status::Stopping);
                state.shutdown.send_replace(true);
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main_window(app),
        _ => {}
    });
    #[cfg(target_os = "macos")]
    MAC_RUNTIME.with(|slot| *slot.borrow_mut() = None);
    if exit_code != 0 {
        return Err(std::io::Error::other("Client stopped with an error").into());
    }
    Ok(())
}
