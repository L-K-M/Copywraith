FROM rust:1.83-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock* ./
COPY crates/ crates/
COPY server/ server/

# Build only the server (skip src-tauri which needs Tauri dependencies)
RUN cargo build --release --package copywraith-server

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/copywraith-server /usr/local/bin/copywraith-server

ENV COPYWRAITH_DATA_DIR=/data
ENV PORT=3742

EXPOSE 3742

VOLUME ["/data"]

CMD ["copywraith-server"]
