FROM rust:slim-bookworm

ARG FUZZ_TOOLCHAIN=nightly-2026-11-26
ENV FUZZ_TOOLCHAIN=${FUZZ_TOOLCHAIN}
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential clang ca-certificates pkg-config protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
RUN rustup toolchain install "${FUZZ_TOOLCHAIN}" --profile minimal \
    && cargo install cargo-fuzz --version 0.13.2 --locked

WORKDIR /workspace
CMD ["bash", "fuzz/run.sh"]
