# TimeTracker frontend

React + TypeScript dashboard for the authenticated HTTP API. `npm run dev:real` uses the real backend; `npm run dev` / `npm run dev:mock` use disposable mock data. Both default to the API at `http://localhost:8239`.

## Run with the mock API

```bash
npm install
npm run dev:mock
```

This starts the mock API at `http://localhost:8239` and Vite at `http://localhost:5173` (use this origin to match the default backend CORS setting). The mock login is `admin` / `change-me-local-only`.

## Run with the Rust backend

Start the backend on port `8239`, then run:

```bash
npm run dev:real
```

Both modes read the API origin from their corresponding environment file (`.env.mock` or
`.env.real`). Runtime code accesses it only through `src/config.ts`.

## Build

```bash
npm run build
```

Run analytics, authentication, and enrollment tests with:

```bash
npm test
npm run test:auth
```

The TypeScript definitions in `src/types.ts` mirror the structures in
`crates/backend/src/dashboard.rs`. Enum values use Serde's default externally tagged JSON shape.
The application-activity chart infers intervals between focused-application events, excludes idle
and stopped-collector intervals, and normalizes each stacked bar to one 15-minute bucket.

## Registration and sessions

Sign in with an administrator account. The `/devices` page accepts a pasted link or scans a QR code when `BarcodeDetector`, HTTPS/localhost, and camera access are available. If scanning is unsupported or denied, use the paste field. Links must match this dashboard's origin and `/devices/register` path and contain exactly the device UUID and lowercase SHA-256 hash in the fragment. Loading/scanning a valid link preserves it in the URL through login, logout, expiry, and reload; approval always requires an explicit user choice and button click.

The `/admins` page creates administrators and changes passwords. JWTs remain in memory. Logout, expiry, and rejected tokens abort outstanding requests and clear private query caches. Reload requires login. Private activity requests use the same session cancellation as the rest of the API.

Set `VITE_API_BASE_URL` for a different backend and set its `DASHBOARD_ORIGIN` to the browser origin. The development mock also supports admin/user creation, password changes, device approval/conflicts, registration checks, signed expiring test JWTs, and paginated activity. `MOCK_TOKEN_TTL_SECONDS` can shorten token expiry for local checks.

## Pending dependencies

This branch is based on `main` (`4a66a136`). PR #47 owns the real-backend default, config/typecheck hotfixes, activity device IDs, and paginated activity loader. Those changes are not duplicated here. Integrate #47 before using the paginated activity API with charts/totals; the backend must preserve preceding state actions. The mock serves the current array response for existing activity requests and paginated responses when `limit` or `offset` is supplied, so the dashboard remains usable on main. Client authentication and database repositories are now included in main. Backend authentication endpoints remain a separate dependency for real device approval and uploads. `npm run test:auth` loads browser modules through Vite and tests the frontend and its mock without requiring those unmerged implementations.
