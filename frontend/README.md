# TimeTracker frontend

React + TypeScript dashboard for the HTTP dashboard API.

## Run with the mock API

```bash
npm install
npm run dev:mock
```

This starts the mock API at `http://localhost:8239` and Vite at `http://127.0.0.1:5173`.

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

Run the analytics tests with:

```bash
npm test
```

The TypeScript definitions in `src/types.ts` mirror the structures in
`crates/backend/src/dashboard.rs`. Enum values use Serde's default externally tagged JSON shape.
The application-activity chart infers intervals between focused-application events, excludes idle
and stopped-collector intervals, and normalizes each stacked bar to one 15-minute bucket.
