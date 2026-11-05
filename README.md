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
