# Copywraith Server

The Copywraith server stores clipboard entries for sync and long-term history. It is a Rust/Axum service backed by SQLite and filesystem blob storage, with a bundled Svelte admin UI.

## Security Model

Do not expose this server directly to the Internet. Run it on a trusted LAN or behind a secure VPN such as Tailscale or Netbird.

The server is single-user and password-protected, but it intentionally does not include public-service protections such as rate limiting or brute-force defense.

On first use, create a password in the admin UI. Once configured:

- Text and blob data are encrypted at rest with AES-256-GCM.
- API requests to protected endpoints require `Authorization: Bearer <password>`.
- The admin UI stores the unlocked password in `sessionStorage`.
- Desktop and Android clients use the same password field in Settings.

If the password is forgotten, deleting `auth.json` from the data directory resets auth, but encrypted data becomes permanently unreadable.

## Prerequisites

- Rust 1.85 or newer; this repository pins the toolchain in `rust-toolchain.toml`.
- Node.js and npm if you need to build the bundled admin UI.
- Docker if you run the containerized server.

## Run With Cargo

From the repository root:

```bash
cargo run -p copywraith-server
```

Defaults:

- API base: `http://localhost:3742/api`
- Admin UI: `http://localhost:3742/`
- Data directory: `./data`

Useful environment variables:

- `COPYWRAITH_DATA_DIR`: data directory, default `./data`
- `PORT`: listen port, default `3742`
- `COPYWRAITH_HOST`: bind host, default `127.0.0.1`; use `0.0.0.0` in Docker
- `COPYWRAITH_UI_DIR`: path to built admin UI assets
- `COPYWRAITH_MAX_BODY_BYTES`: max request body size
- `RUST_LOG`: Rust logging filter

Tip: copy `.env.example` to `.env` and adjust values for local or Docker runs.

## Run With Docker

From the repository root:

```bash
docker compose up --build
```

On systems where Docker requires sudo:

```bash
sudo docker compose up --build
```

The root compose file exposes port `3742` and persists data in `./copywraith-data`.

For repeat deployments, use the helper script:

```bash
./scripts/redeploy-server-docker.sh
```

With sudo Docker:

```bash
USE_SUDO=1 ./scripts/redeploy-server-docker.sh
```

The script tags the image as `copywraith-server:<server-version>` from `server/Cargo.toml` and uses the same tag for build and startup, which helps avoid stale-image confusion.

You can also run from the `server/` directory:

```bash
docker compose up --build
```

## Admin UI

The admin UI lives in `server/ui/` and is a plain Svelte + Vite app. The server serves the built UI from `/`.

Build it manually with:

```bash
cd server/ui
npm install
npm run build
```

The Dockerfile builds and embeds the UI automatically. If the UI has not been built for a cargo run, the server shows a fallback page with build instructions.

## API

Base path: `/api`

- `GET /health`
- `GET /auth/status`
- `POST /auth/setup`
- `POST /auth/unlock`
- `POST /auth/change-password`
- `POST /auth/lock`
- `POST /entries`
- `GET /entries`
- `GET /entries/{id}`
- `PATCH /entries/{id}`
- `DELETE /entries/{id}`
- `GET /entries/{id}/blob`

Interactive docs are served at `/swagger-ui/`, and OpenAPI JSON is served at `/api-docs/openapi.json`.

`GET /entries` supports `limit`, `offset`, `content_type`, `starred_only`, and `search` query parameters.

## Data Storage

Server data is stored under `COPYWRAITH_DATA_DIR`:

- SQLite database for metadata and text entries.
- Blob directory for binary payloads.
- `auth.json` for password verifier and encrypted data key.

Entries are deduplicated by SHA-256 `content_hash`.

## Docker Notes

The server Docker builder image must use Rust 1.85 or newer. If a build fails with ``feature `edition2024` is required``, rebuild with the current Dockerfile:

```bash
docker compose build --no-cache --pull copywraith-server
```

Containers must bind `COPYWRAITH_HOST=0.0.0.0`; both compose files already set this. If `/api/health` from the host fails with connection reset or refused, verify that environment variable first.

The compose files support explicit image tags with `COPYWRAITH_SERVER_IMAGE_REPO` and `COPYWRAITH_SERVER_IMAGE_TAG`. The default tag is kept aligned with the server crate version, currently `0.2.0`.

After bumping the server crate version, run:

```bash
./scripts/sync-version.sh --write
```

## Troubleshooting

- `npm install` fails in `server/ui`: verify network access and npm registry settings.
- Docker build fails with `edition2024`: rebuild with `docker compose build --no-cache --pull copywraith-server` and confirm `server/Dockerfile` uses Rust 1.85 or newer.
- `/api/health` fails after container start: confirm `COPYWRAITH_HOST=0.0.0.0` and redeploy with `./scripts/redeploy-server-docker.sh`.
- Server reports an old version after deploy: redeploy with the helper script and verify the running image tag in script output.
