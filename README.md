# TimeTracker demo

TimeTracker is a small Rust client/server demo. A Linux client sends a startup event and periodic
heartbeats over gRPC. The server receives them, keeps the current session's events in memory, and
serves a React dashboard over HTTP.

## Run the server

Docker Compose builds the Rust backend and the web dashboard into one container:

```bash
docker compose up --build -d
```

Open <http://localhost:8080>. The gRPC client endpoint is exposed on port `50051`.

The demo intentionally uses in-memory storage, so collected events are cleared when the container
restarts. `HTTP_PORT` and `GRPC_PORT` can be set before starting Compose to change the published
ports.

## Run the desktop client

Build and start the client on the laptop that should report online status:

```bash
cargo build --release -p timetracker-client
./target/release/timetracker-client \
  --grpc-address http://YOUR_SERVER:50051 \
  --heartbeat-seconds 30
```

The client stores a stable device ID under `$XDG_STATE_HOME/timetracker/device-id`, or under
`$HOME/.local/state/timetracker/device-id` when `XDG_STATE_HOME` is unset. It retries while the
server is unavailable. Set `TIMETRACKER_HEARTBEAT_SECONDS` to change the default 30-second interval.
`TIMETRACKER_SERVER` remains available as an environment-variable alternative to
`--grpc-address`. Run the client with `--help` to see all CLI options. On macOS the client starts
the desktop observation adapters; on other operating systems it sends collector lifecycle events
and heartbeats.

## Local development

```bash
cargo test --workspace --all-targets
cargo run -p timetracker-backend
```

In a second terminal:

```bash
TIMETRACKER_HEARTBEAT_SECONDS=2 cargo run -p timetracker-client
```

The dashboard API is available at `http://localhost:8080`; the production container also serves the
built frontend at that address.
