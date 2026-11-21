CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS devices (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id)
);
CREATE TABLE IF NOT EXISTS events (
    id UUID NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id),
    device_id UUID NOT NULL REFERENCES devices(id),
    occurred_at TIMESTAMPTZ NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    payload JSONB NOT NULL,
    action JSONB,
    PRIMARY KEY (device_id, id)
);
CREATE INDEX IF NOT EXISTS events_user_time ON events(user_id, occurred_at, id);
