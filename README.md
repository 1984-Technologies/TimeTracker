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
| `JWT_SECRET` | Required: unpadded base64url encoding of at least 32 cryptographically random bytes | Generate as shown below |
| `DASHBOARD_ORIGIN` | Exact browser origin allowed by CORS; defaults to `http://localhost:5173` | `https://tracker.example.com` |
| `INITIAL_ADMIN_USERNAME` | First admin username; required when the admin table is empty | `admin` |
| `INITIAL_ADMIN_PASSWORD` | First admin password; required when the admin table is empty | Supply through your secret manager |

### Example `.env` file:
```env
DATABASE_URL=postgres://postgres:postgres@localhost:5432/timetracker
GRPC_BIND_ADDR=0.0.0.0:50051
HTTP_BIND_ADDR=0.0.0.0:8080
DASHBOARD_ORIGIN=http://localhost:5173
```

Generate `JWT_SECRET` once, store it securely, and supply it on every backend
start. For example, in PowerShell 7:

```powershell
$bytes = New-Object byte[] 32
[System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
$env:JWT_SECRET = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
```

Use HTTPS for HTTP and TLS for gRPC in production, with TLS terminated by a
trusted reverse proxy when running these listeners without TLS. Keep passwords,
device secrets and bearer tokens out of request logs. The server's tracing does
not log request bodies or authorization headers.

## Authentication and management API

The first startup creates one admin from `INITIAL_ADMIN_USERNAME` and
`INITIAL_ADMIN_PASSWORD`. Subsequent starts preserve existing accounts and their
passwords. Passwords use salted Argon2id hashes. JWTs use HS256 with issuer
`timetracker`, audience `timetracker-api`, and a 900-second lifetime.
`sub` is the principal UUID and `kind` is `admin` or `device`. Each protected
request also checks that the principal still exists.

| Endpoint | Access | Result |
| :--- | :--- | :--- |
| `POST /api/auth/login` | Public, username/password | Admin `TokenResponse` |
| `GET /api/auth/me` | Admin bearer token | Current admin ID and username |
| `GET`, `POST /api/admins` | Admin bearer token | List accounts / create account (`201`) |
| `PATCH /api/admins/{id}` | Admin bearer token | Change password (`204`) |
| `GET /api/devices`, `GET /api/devices/{id}` | Admin bearer token | Device IDs and user assignments |
| `POST /api/devices` | Admin bearer token | Approve device ID, user ID and credential hash |
| `POST /api/devices/register` | Public, device ID/secret | Read-only `{registered}` check |
| `POST /api/devices/login` | Public, device ID/secret | Device `TokenResponse` |
| `POST /users` | Admin bearer token | Create tracked user (`201`) |
| `GET /users`, `GET /users/{id}/actions` | Admin bearer token | Existing user/activity reads |
| `GET /health`, CORS preflight | Public | Health / CORS response |

`TokenResponse` contains `access_token`, `token_type: "Bearer"` and
`expires_in: 900`. Send `Authorization: Bearer <token>` on HTTP requests and
`authorization: Bearer <device token>` in gRPC metadata. For `UploadEvents`, the
request device ID and every event's device ID must match the device JWT subject.

Device secrets are 32 random bytes encoded as unpadded base64url. Approval stores
the lowercase hex SHA-256 of the decoded bytes. Only admins approve devices, and
the assigned user must already exist. Repeating an identical approval succeeds;
changing an approved device's owner or credentials returns `409`. Devices from
older databases cannot authenticate until an admin approves their credential
hash; their existing events are preserved.

Errors use JSON `{code, message}`: invalid input `400`, missing/invalid credentials
`401`, wrong principal kind `403`, missing records `404`, and conflicts `409`.
Both login endpoints share a per-process limit of 20 attempts per 60 seconds,
including successful logins; excess attempts return `429` with `Retry-After`.
Password verification runs off the async executor with at most four concurrent
workers. CORS allows the configured origin, `Authorization`, `Content-Type`, and
GET/POST/PATCH/OPTIONS; preflight does not require a token.

Run the normal checks and the isolated database integration tests:

```powershell
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
$env:TEST_DATABASE_URL = 'postgres://postgres:password@127.0.0.1:5432/timetracker_test'
cargo test -p timetracker-backend --test postgres --locked -- --ignored
```

Use a dedicated test database. The database tests cover authenticated HTTP/gRPC
round trips to the activity API, replay and rollback, cross-device rejection,
the gRPC size limit, concurrent bootstrap/approval, and legacy-device migration.

Authentication also has four coverage-guided fuzz targets and stable-Rust seed
replay. See [fuzz/README.md](fuzz/README.md) for local runs, failure reproduction,
and the independent JWT claims oracle. CI runs each target for 30 seconds with
AddressSanitizer and saves failure artifacts.
