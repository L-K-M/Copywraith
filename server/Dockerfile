FROM node:22-bookworm-slim AS ui-builder

WORKDIR /build/server/ui

COPY server/ui/package*.json ./
RUN npm ci

# The UI build writes dist/Third-Party-Notices.txt with the shared generator
# and font license texts from the repository root.
COPY scripts/bundled-modules.mjs scripts/third-party-notices.mjs /build/scripts/
COPY licenses/fonts/ /build/licenses/fonts/
COPY server/ui/ ./
RUN npm run build

FROM rust:1.98-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev python3 && rm -rf /var/lib/apt/lists/*

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
RUN cargo build --release --locked --package copywraith-server

# Third-party notices for the image: the server's Rust crates, then the UI
# bundle's npm packages and fonts, plus the Rust standard library's notices.
COPY scripts/rust-notices.py scripts/
COPY licenses/ licenses/
COPY --from=ui-builder /build/server/ui/dist/Third-Party-Notices.txt /tmp/ui-notices.txt
RUN python3 scripts/rust-notices.py -p copywraith-server --target host --title "Copywraith server" \
      --overrides licenses/rust-overrides.json --append /tmp/ui-notices.txt \
      --out /notices/Third-Party-Notices.txt --rust-std /notices/Rust-Licenses.html \
 && test -s /notices/Third-Party-Notices.txt && test -s /notices/Rust-Licenses.html

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/copywraith-server /usr/local/bin/copywraith-server
COPY --from=ui-builder /build/server/ui/dist /ui/dist
# Debian packages keep their notices in /usr/share/doc/*/copyright.
COPY --from=builder /notices/ /usr/share/doc/copywraith-server/
COPY LICENSE /usr/share/doc/copywraith-server/LICENSE

ENV COPYWRAITH_DATA_DIR=/data
ENV PORT=3742
ENV COPYWRAITH_HOST=0.0.0.0
ENV COPYWRAITH_UI_DIR=/ui/dist

EXPOSE 3742

VOLUME ["/data"]

CMD ["copywraith-server"]
