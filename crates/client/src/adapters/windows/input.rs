use std::{
    cell::RefCell,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use async_trait::async_trait;
use timetracker_model::{AdapterStatus, ObservationKind, ObservationType};
use tokio::sync::{mpsc, oneshot, watch};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::*,
};

use super::{desktop_available, observation, send, stopped, unavailable};
use crate::{
    ClientError,
    adapters::{AdapterOutput, DesktopAdapter},
};

// The callback runs on the installing thread. Only event categories and timestamps
// leave it: no key values, typed text, button identities or coordinates are stored.
struct CallbackEvents {
    kind: ObservationType,
    sender: mpsc::Sender<AdapterOutput>,
    overflow: Arc<AtomicBool>,
}

thread_local! {
    static EVENTS: RefCell<Option<CallbackEvents>> = const { RefCell::new(None) };
}

fn classify(kind: ObservationType, message: u32) -> Option<ObservationKind> {
    match (kind, message) {
        (ObservationType::Keyboard, WM_KEYDOWN | WM_SYSKEYDOWN) => {
            Some(ObservationKind::KeyboardButtonPressed {})
        }
        (
            ObservationType::MouseClick,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN,
        ) => Some(ObservationKind::MouseClicked {}),
        (ObservationType::MouseMove, WM_MOUSEMOVE) => Some(ObservationKind::MouseMoved {}),
        _ => None,
    }
}

unsafe extern "system" fn callback(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0 {
        EVENTS.with(|events| {
            if let Some(events) = events.borrow().as_ref()
                && let Some(kind) = classify(events.kind, message as u32)
                && let Err(mpsc::error::TrySendError::Full(_)) =
                    events.sender.try_send(observation(kind))
            {
                events.overflow.store(true, Ordering::Relaxed);
            }
        });
    }
    // SAFETY: forwarding the original parameters keeps input delivery unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, message, data) }
}

struct Listener {
    stop: Arc<AtomicBool>,
    overflow: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Listener {
    async fn start(
        kind: ObservationType,
        output: mpsc::Sender<AdapterOutput>,
    ) -> Result<Self, ClientError> {
        desktop_available()?;
        let stop = Arc::new(AtomicBool::new(false));
        let overflow = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_overflow = overflow.clone();
        let (ready_tx, ready_rx) = oneshot::channel();
        let worker = thread::Builder::new()
            .name(format!("timetracker-{kind:?}"))
            .spawn(move || {
                EVENTS.with(|events| {
                    *events.borrow_mut() = Some(CallbackEvents {
                        kind,
                        sender: output,
                        overflow: thread_overflow,
                    })
                });
                // SAFETY: callback lives for the whole process; hooks and the message
                // pump belong to this thread and are removed before thread exit.
                unsafe {
                    let hook_type = if kind == ObservationType::Keyboard {
                        WH_KEYBOARD_LL
                    } else {
                        WH_MOUSE_LL
                    };
                    let hook = SetWindowsHookExW(
                        hook_type,
                        Some(callback),
                        GetModuleHandleW(std::ptr::null()),
                        0,
                    );
                    if hook.is_null() {
                        let _ = ready_tx.send(Err(io::Error::last_os_error()));
                        return;
                    }
                    let _ = ready_tx.send(Ok(()));
                    let mut message = std::mem::zeroed();
                    while !thread_stop.load(Ordering::Relaxed) {
                        // Bound each batch so continuous input cannot delay shutdown.
                        for _ in 0..64 {
                            if PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE)
                                == 0
                            {
                                break;
                            }
                            TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                    UnhookWindowsHookEx(hook);
                }
                EVENTS.with(|events| *events.borrow_mut() = None);
            })?;
        let listener = Self {
            stop,
            overflow,
            worker: Some(worker),
        };
        ready_rx.await??;
        Ok(listener)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

async fn run(
    id: &'static str,
    kind: ObservationType,
    output: mpsc::Sender<AdapterOutput>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), ClientError> {
    if *shutdown.borrow() || output.is_closed() {
        return Ok(());
    }
    let (sender, mut events) = mpsc::channel(4096);
    let listener = match Listener::start(kind, sender).await {
        Ok(listener) => listener,
        Err(error) => {
            send(
                &output,
                &mut shutdown,
                AdapterOutput::Status {
                    adapter_id: id,
                    observation_type: kind,
                    status: unavailable(error),
                },
            )
            .await;
            return Ok(());
        }
    };
    let mut status = AdapterStatus::Available;
    if !send(
        &output,
        &mut shutdown,
        AdapterOutput::Status {
            adapter_id: id,
            observation_type: kind,
            status: status.clone(),
        },
    )
    .await
    {
        return Ok(());
    }
    let mut health = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            biased;
            _ = stopped(&mut shutdown) => return Ok(()),
            _ = output.closed() => return Ok(()),
            _ = health.tick() => {
                let current = if listener.overflow.swap(false, Ordering::Relaxed) {
                    unavailable("Windows input consumer fell behind; events were lost")
                } else {
                    desktop_available().map_or_else(unavailable, |_| AdapterStatus::Available)
                };
                if current != status {
                    status = current;
                    if !send(&output, &mut shutdown, AdapterOutput::Status { adapter_id: id, observation_type: kind, status: status.clone() }).await { return Ok(()); }
                }
            }
            event = events.recv() => {
                let Some(event) = event else {
                    send(&output, &mut shutdown, AdapterOutput::Status { adapter_id: id, observation_type: kind, status: unavailable("Windows input listener stopped") }).await;
                    return Ok(());
                };
                if status == AdapterStatus::Available && !send(&output, &mut shutdown, event).await { return Ok(()); }
            }
        }
    }
}

macro_rules! input_adapter {
    ($name:ident, $id:literal, $kind:ident) => {
        #[derive(Default)]
        pub struct $name;
        #[async_trait]
        impl DesktopAdapter for $name {
            fn id(&self) -> &'static str {
                $id
            }
            fn observation_type(&self) -> ObservationType {
                ObservationType::$kind
            }
            async fn probe(&self) -> AdapterStatus {
                let (output, _events) = mpsc::channel(1);
                Listener::start(self.observation_type(), output)
                    .await
                    .map_or_else(unavailable, |_| AdapterStatus::Available)
            }
            async fn run(
                &mut self,
                output: mpsc::Sender<AdapterOutput>,
                shutdown: watch::Receiver<bool>,
            ) -> Result<(), ClientError> {
                run(self.id(), self.observation_type(), output, shutdown).await
            }
        }
    };
}

input_adapter!(KeyboardAdapter, "windows.keyboard", Keyboard);
input_adapter!(MouseClickAdapter, "windows.mouse-click", MouseClick);
input_adapter!(MouseMoveAdapter, "windows.mouse-move", MouseMove);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_press_and_movement_messages_are_observations() {
        assert!(matches!(
            classify(ObservationType::Keyboard, WM_SYSKEYDOWN),
            Some(ObservationKind::KeyboardButtonPressed {})
        ));
        assert!(classify(ObservationType::Keyboard, WM_KEYUP).is_none());
        assert!(matches!(
            classify(ObservationType::MouseClick, WM_XBUTTONDOWN),
            Some(ObservationKind::MouseClicked {})
        ));
        assert!(classify(ObservationType::MouseClick, WM_LBUTTONUP).is_none());
        assert!(matches!(
            classify(ObservationType::MouseMove, WM_MOUSEMOVE),
            Some(ObservationKind::MouseMoved {})
        ));
        assert!(classify(ObservationType::MouseMove, WM_MOUSEWHEEL).is_none());
    }
}
