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


## Local authentication demo

Start the database service and backend with
```sh
export JWT_SECRET="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
sudo --preserve-env=JWT_SECRET docker compose up -d db
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/app_db \
INITIAL_ADMIN_USERNAME=admin INITIAL_ADMIN_PASSWORD=change-me-local-only \
JWT_SECRET="$JWT_SECRET" DASHBOARD_ORIGIN=http://localhost:5173 \
HTTP_BIND_ADDR=127.0.0.1:8239 GRPC_BIND_ADDR=127.0.0.1:50051 \
cargo run -p timetracker-backend
```

`sudo` must preserve `JWT_SECRET` because Compose validates the backend configuration even
when starting only `db`. Restarting an existing PostgreSQL volume does not recreate a
manually dropped database. If needed, recreate the empty database once before starting
the backend:

```sh
sudo --preserve-env=JWT_SECRET docker compose exec -T db createdb -U postgres app_db
```

Run frontend with
```sh
npm --prefix frontend ci
npm --prefix frontend run dev:real
```
Dashboard can be accessed at `http://localhost:5173`

Run client:
```sh
cargo run -p timetracker-client --features desktop-ui
```
