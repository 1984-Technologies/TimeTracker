# TimeTracker 

Nothing special. Just another time tracker written in Rust.

## Roadmap
![roadmap of the project](assets/TimeTracker-Roadmap.jpg)

## Configuration (Environment Variables)

The backend service is configured via environment variables:

| Variable | Description | Example |
| :--- | :--- | :--- |
| `DATABASE_URL` | PostgreSQL connection string | `postgres://postgres:postgres@localhost:5432/timetracker` |
| `GRPC_BIND_ADDR` | Socket address for gRPC Metrics Server | `0.0.0.0:50051` (or `127.0.0.1:50051`) |
| `HTTP_BIND_ADDR` | Socket address for HTTP Dashboard API | `0.0.0.0:8080` (or `127.0.0.1:8080`) |

### Example `.env` file:
```env
DATABASE_URL=postgres://postgres:postgres@localhost:5432/timetracker
GRPC_BIND_ADDR=0.0.0.0:50051
HTTP_BIND_ADDR=0.0.0.0:8080
```

The client reads `GRPC_SERVER_ADDR` for the upload server address (default:
`127.0.0.1:50051`).

## Windows desktop adapters

`timetracker_client::adapters::windows::default_adapters()` returns five
`DesktopAdapter` implementations for the focused application/title, idle state,
keyboard presses, mouse clicks and mouse movement. The module and its Win32
dependency are compiled only on Windows.

Run the adapters in the interactive user's Windows session. Focus is sampled
every 500 ms; `app_id` is the lowercase full executable path and `title` is
optional. Changes to the title of the same application produce new observations.
Idle is sampled every second using the session's last-input timer, with a default
threshold of 60 seconds; use `IdleAdapter::new(Duration)` to configure it.
Polling adapters publish the initial state and subsequent changes.

Input uses low-level Windows hooks on dedicated message-pump threads. It reports
press/click/movement categories without recording key values, typed text or
coordinates, matching the current observation model. Adapter status reports API
failures, inaccessible desktops/processes and input queue overflow. Polling
adapters recover automatically when sampling becomes available again. Hooks are
released when their adapter stops. Shutdown also interrupts a blocked output send.
Signals are limited to the accessible desktop/session; services, secure desktops
and protected processes may be unavailable. Focus polling can miss windows that
appear for less than one polling interval.

The client entry point and collection supervisor currently contain placeholders.
The standalone smoke example runs the adapters directly without a backend:

```powershell
cargo run -p timetracker-client --example windows_adapter_smoke -- 20
```

Switch windows, change a window title, type, click and move the mouse. The example
uses a three-second idle threshold, prints observations/statuses, then stops.

```powershell
cargo test -p timetracker-client --lib
cargo test -p timetracker-client --lib interactive_windows_session -- --ignored --nocapture
```

The second command needs an unlocked interactive Windows session. Leave the
desktop untouched while it opens a disposable window, changes its Unicode title,
injects F24 and a mouse click into that window, checks all five observation types,
and closes the window. It is ignored in unattended test runs. Building the client
requires the protobuf compiler version expected by `protobuf-codegen` on `PATH`.
