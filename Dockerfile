FROM node:22-bookworm-slim AS frontend-builder
WORKDIR /src/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:bookworm AS backend-builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
RUN cargo build --locked --release -p timetracker-backend

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=backend-builder /src/target/release/timetracker-backend /usr/local/bin/timetracker-backend
COPY --from=frontend-builder /src/frontend/dist/ /app/frontend/

ENV GRPC_BIND_ADDR=0.0.0.0:50051 \
    HTTP_BIND_ADDR=0.0.0.0:8080 \
    STATIC_DIR=/app/frontend
EXPOSE 50051 8080
USER 65534:65534
ENTRYPOINT ["timetracker-backend"]
