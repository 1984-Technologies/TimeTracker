use crate::{
    adapters::{AdapterOutput, default_adapters, windows},
    supervisor::CollectionSupervisor,
};
use std::{
    collections::HashSet,
    sync::mpsc as std_mpsc,
    thread,
    time::{Duration, Instant},
};
use timetracker_model::{AdapterStatus, ObservationKind};
use tokio::sync::mpsc;
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn focus_test_window(window: HWND) {
    // SAFETY: the test thread owns this window. Temporarily sharing the current
    // foreground thread's input queue lets the test activate its window even
    // when a shell notification was foreground at launch. Always detach again.
    unsafe {
        let current = GetCurrentThreadId();
        let foreground = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let attached = foreground != 0
            && foreground != current
            && AttachThreadInput(current, foreground, 1) != 0;
        BringWindowToTop(window);
        SetForegroundWindow(window);
        if attached {
            AttachThreadInput(current, foreground, 0);
        }
    }
}

enum Command {
    Exercise,
    Stop,
}

struct TestWindow {
    commands: std_mpsc::Sender<Command>,
    worker: Option<thread::JoinHandle<()>>,
}

impl TestWindow {
    fn new() -> Self {
        let (commands, receiver) = std_mpsc::channel();
        let (ready, started) = std_mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            // SAFETY: the built-in STATIC class owns no Rust data; this thread
            // owns the window and pumps its messages until destruction.
            unsafe {
                let window = CreateWindowExW(
                    0,
                    wide("STATIC").as_ptr(),
                    wide("TimeTracker adapter test — initial").as_ptr(),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    100,
                    100,
                    480,
                    220,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
                assert!(!window.is_null(), "could not create test window");
                focus_test_window(window);
                ready.send(()).unwrap();
                let mut message = std::mem::zeroed();
                let mut exercised = false;
                let mut next_focus_attempt = Instant::now();
                loop {
                    while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    if !exercised && Instant::now() >= next_focus_attempt {
                        if GetForegroundWindow() != window {
                            focus_test_window(window);
                        }
                        next_focus_attempt = Instant::now() + Duration::from_millis(250);
                    }
                    match receiver.try_recv() {
                        Ok(Command::Stop) | Err(std_mpsc::TryRecvError::Disconnected) => break,
                        Ok(Command::Exercise) => {
                            exercised = true;
                            SetWindowTextW(
                                window,
                                wide("TimeTracker adapter test — changed заголовок").as_ptr(),
                            );
                            focus_test_window(window);
                            // Input is sent only while our own disposable window
                            // is foreground. F24 does not enter any text.
                            if GetForegroundWindow() == window {
                                let mut old = POINT::default();
                                let mut rect = RECT::default();
                                GetCursorPos(&mut old);
                                GetWindowRect(window, &mut rect);
                                SetCursorPos(
                                    (rect.left + rect.right) / 2,
                                    (rect.top + rect.bottom) / 2,
                                );
                                let key = |flags| INPUT {
                                    r#type: INPUT_KEYBOARD,
                                    Anonymous: INPUT_0 {
                                        ki: KEYBDINPUT {
                                            wVk: VK_F24,
                                            dwFlags: flags,
                                            ..Default::default()
                                        },
                                    },
                                };
                                let mouse = |flags| INPUT {
                                    r#type: INPUT_MOUSE,
                                    Anonymous: INPUT_0 {
                                        mi: MOUSEINPUT {
                                            dwFlags: flags,
                                            ..Default::default()
                                        },
                                    },
                                };
                                let inputs = [
                                    key(0),
                                    key(KEYEVENTF_KEYUP),
                                    mouse(MOUSEEVENTF_LEFTDOWN),
                                    mouse(MOUSEEVENTF_LEFTUP),
                                ];
                                SendInput(
                                    inputs.len() as u32,
                                    inputs.as_ptr(),
                                    std::mem::size_of::<INPUT>() as i32,
                                );
                                SetCursorPos(old.x, old.y);
                            }
                        }
                        Err(std_mpsc::TryRecvError::Empty) => {}
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                DestroyWindow(window);
            }
        });
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        Self {
            commands,
            worker: Some(worker),
        }
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

/// Opens a disposable test window and briefly injects F24 and a mouse click into it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an unlocked interactive Windows session"]
async fn interactive_windows_session() {
    let window = TestWindow::new();
    let (output, mut events) = mpsc::channel(4096);
    let mut adapters = default_adapters().unwrap();
    adapters[1] = Box::new(windows::IdleAdapter::new(Duration::from_secs(2)));
    let supervisor = CollectionSupervisor::start(adapters, output).await;
    let result = tokio::time::timeout(Duration::from_secs(45), async {
        let mut available = HashSet::new();
        let mut exercised = false;
        let mut initial = false;
        let mut changed = false;
        let mut idle = false;
        let mut active_again = false;
        let mut keyboard = false;
        let mut clicked = false;
        let mut moved = false;
        while let Some(event) = events.recv().await {
            match event {
                AdapterOutput::Status {
                    status: AdapterStatus::Available,
                    adapter_id,
                    ..
                } => {
                    available.insert(adapter_id);
                }
                AdapterOutput::Status {
                    status, adapter_id, ..
                } => panic!("{adapter_id}: {status:?}"),
                AdapterOutput::Observation(event) => {
                    if !matches!(event.kind, ObservationKind::MouseMoved { .. }) {
                        eprintln!("{:?}", event.kind);
                    }
                    match event.kind {
                        ObservationKind::FocusedApplicationChanged { app_id, title } => {
                            if title.as_deref() == Some("TimeTracker adapter test — initial") {
                                assert!(app_id.ends_with(".exe"));
                                initial = true;
                            }
                            changed |= title.as_deref()
                                == Some("TimeTracker adapter test — changed заголовок");
                        }
                        ObservationKind::IdleChanged { idle: value } => {
                            idle |= value;
                            active_again |= exercised && idle && !value;
                        }
                        ObservationKind::KeyboardButtonPressed { key } => {
                            keyboard |= exercised && key == VK_F24.to_string();
                        }
                        ObservationKind::MouseClicked { button, x, y } => {
                            clicked |= exercised && button == "Left" && x.is_some() && y.is_some();
                        }
                        ObservationKind::MouseMoved { .. } => moved |= exercised,
                        _ => {}
                    }
                }
            }
            if initial && idle && available.len() == 5 && !exercised {
                window.commands.send(Command::Exercise).unwrap();
                exercised = true;
            }
            if initial && changed && idle && active_again && keyboard && clicked && moved {
                return;
            }
        }
        panic!("adapter output closed before all observations arrived");
    })
    .await;
    tokio::time::timeout(Duration::from_secs(2), supervisor.stop())
        .await
        .unwrap()
        .unwrap();
    result.expect("missing focus/title, idle transition, keyboard, click or movement observation");
}
