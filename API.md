# Copywraith API

This document describes the server API and how to access interactive API docs.

## OpenAPI and Swagger UI

Start the server first:

```bash
cargo run -p copywraith-server
```

Then open:

- Swagger UI: `http://localhost:3742/swagger-ui/`
- OpenAPI JSON: `http://localhost:3742/api-docs/openapi.json`

Notes:

- Base API path is `/api`.
- In Swagger UI, click **Authorize** and enter your server password as a bearer token when testing protected endpoints.
- Swagger UI assets are loaded from `unpkg.com`; if you are offline, use the raw OpenAPI JSON URL.

## Authentication model

Copywraith uses a single server password.

- Auth setup/unlock endpoints do not require bearer auth.
- Most `/api/entries*` endpoints and some `/api/auth/*` endpoints require:

```http
Authorization: Bearer <password>
```

If no password is configured yet, protected endpoints return `403` until password setup is completed.

## Endpoint groups

### System

- `GET /api/health`

### Auth

- `GET /api/auth/status`
- `POST /api/auth/setup`
- `POST /api/auth/unlock`
- `POST /api/auth/change-password`
- `POST /api/auth/lock`

### Entries

- `POST /api/entries`
- `GET /api/entries`
- `GET /api/entries/{id}`
- `PATCH /api/entries/{id}`
- `DELETE /api/entries/{id}`
- `GET /api/entries/{id}/blob`

## Behavior notes

- Entries are deduplicated by `content_hash`.
- Sensitive text is masked in API responses.
- Blob payloads are fetched separately with `/api/entries/{id}/blob`.
- `GET /api/entries` supports pagination and filtering via query params (`limit`, `offset`, `content_type`, `starred_only`, `search`).
- For stable sync pagination on mutable datasets, it also supports cursor params `before_updated_at` + `before_id` (descending by `(updated_at, id)`).
