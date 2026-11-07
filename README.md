# TimeTracker 

Nothing special. Just another time tracker written in Rust.

## Building

Install Rust and the Protobuf compiler (`protoc`), and make sure both are available in `PATH`.
The client uses Protobuf 4.34.0-release and requires the matching compiler (protoc 34.0).
Then run `cargo check --workspace --all-targets` and `cargo test --workspace`.

The client reads the gRPC server address from `GRPC_SERVER_ADDR` (default: `localhost:50051`).

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
