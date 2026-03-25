FROM node:20-bookworm-slim AS ui-builder

WORKDIR /build/server/ui

COPY server/ui/package*.json ./
RUN npm ci

COPY server/ui/ ./
RUN npm run build

FROM rust:1.85-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock* ./
COPY crates/ crates/
COPY server/ server/

# Cargo resolves all workspace members before building a single package.
# Include the Tauri manifest + lib entrypoint so workspace loading succeeds
# without copying the full desktop source tree into the build context.
COPY src-tauri/Cargo.toml src-tauri/Cargo.toml
COPY src-tauri/src/lib.rs src-tauri/src/lib.rs

# Build only the server (skip src-tauri which needs Tauri dependencies)
RUN cargo build --release --package copywraith-server

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/copywraith-server /usr/local/bin/copywraith-server
COPY --from=ui-builder /build/server/ui/dist /ui/dist

ENV COPYWRAITH_DATA_DIR=/data
ENV PORT=3742
ENV COPYWRAITH_UI_DIR=/ui/dist

EXPOSE 3742

VOLUME ["/data"]

CMD ["copywraith-server"]
