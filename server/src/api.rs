use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::{Modify, OpenApi, ToSchema};

use copywraith_core::api_types::*;
use copywraith_core::models::ContentType;

use crate::crypto;
use crate::AppState;

type AppRouter = Router<Arc<AppState>>;

pub fn router() -> AppRouter {
    Router::new()
        // Auth endpoints (no password required)
        .route("/auth/status", get(auth_status))
        .route("/auth/setup", post(auth_setup))
        .route("/auth/unlock", post(auth_unlock))
        // Auth endpoints (password required)
        .route("/auth/change-password", post(auth_change_password))
        .route("/auth/lock", post(auth_lock))
        // Data endpoints
        .route("/health", get(health))
        .route("/entries", post(create_entry))
        .route("/entries", get(list_entries))
        .route("/entries/{id}", get(get_entry))
        .route("/entries/{id}", patch(update_entry))
        .route("/entries/{id}", delete(delete_entry))
        .route("/entries/{id}/blob", get(get_blob))
}

#[derive(OpenApi)]
#[openapi(
    paths(
        auth_status,
        auth_setup,
        auth_unlock,
        auth_change_password,
        auth_lock,
        health,
        create_entry,
        list_entries,
        get_entry,
        update_entry,
        delete_entry,
        get_blob
    ),
    components(
        schemas(
            AuthStatusResponse,
            SetupRequest,
            UnlockRequest,
            ChangePasswordRequest,
            ErrorResponse,
            HealthResponse,
            CreateEntryRequest,
            UpdateEntryRequest,
            ListEntriesParams,
            EntryResponse,
            ListEntriesResponse,
            CreateEntryResponse,
            copywraith_core::models::ClipboardEntry,
            copywraith_core::models::ContentType
        )
    ),
    tags(
        (name = "auth", description = "Password setup, unlock, and lock endpoints"),
        (name = "entries", description = "Clipboard entry CRUD and blob download endpoints"),
        (name = "system", description = "Health and status endpoints")
    ),
    modifiers(&ApiServerAddon)
)]
pub struct ApiDoc;

struct ApiServerAddon;

impl Modify for ApiServerAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi.servers = Some(vec![utoipa::openapi::Server::new("/")]);

        let components = openapi
            .components
            .get_or_insert_with(utoipa::openapi::Components::new);
        components.add_security_scheme(
            "bearer_auth",
            utoipa::openapi::security::SecurityScheme::Http(
                utoipa::openapi::security::HttpBuilder::new()
                    .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                    .bearer_format("Password")
                    .build(),
            ),
        );
    }
}

pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

// ---------------------------------------------------------------------------
// Auth endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
struct AuthStatusResponse {
    initialized: bool,
    unlocked: bool,
}

#[derive(Deserialize, ToSchema)]
struct SetupRequest {
    password: String,
}

#[derive(Deserialize, ToSchema)]
struct UnlockRequest {
    password: String,
}

#[derive(Deserialize, ToSchema)]
struct ChangePasswordRequest {
    old_password: String,
    new_password: String,
}

#[utoipa::path(
    get,
    path = "/api/auth/status",
    tag = "auth",
    responses(
        (status = 200, description = "Returns auth initialization and unlock status", body = AuthStatusResponse)
    )
)]
async fn auth_status(State(state): State<Arc<AppState>>) -> Json<AuthStatusResponse> {
    let crypto = state.crypto.lock().unwrap();
    Json(AuthStatusResponse {
        initialized: crypto.is_initialized(),
        unlocked: crypto.is_unlocked(),
    })
}

#[utoipa::path(
    post,
    path = "/api/auth/setup",
    tag = "auth",
    request_body = SetupRequest,
    responses(
        (status = 200, description = "Initializes password protection and encrypts existing data"),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn auth_setup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetupRequest>,
) -> Result<StatusCode, AppError> {
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let mut crypto = state.crypto.lock().unwrap();
    if crypto.is_initialized() {
        return Err(AppError::BadRequest(
            "Password already configured".to_string(),
        ));
    }

    crypto.setup_password(&req.password)?;

    // Encrypt any existing unencrypted entries
    let dek = crypto
        .get_dek()
        .ok_or_else(|| anyhow::anyhow!("DEK not available after setup"))?;
    drop(crypto); // release crypto lock before touching storage

    migrate_existing_data(&state, &dek)?;

    Ok(StatusCode::OK)
}

#[utoipa::path(
    post,
    path = "/api/auth/unlock",
    tag = "auth",
    request_body = UnlockRequest,
    responses(
        (status = 200, description = "Unlocks encrypted data for this process"),
        (status = 400, description = "No password configured", body = ErrorResponse),
        (status = 401, description = "Wrong password", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn auth_unlock(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UnlockRequest>,
) -> Result<StatusCode, AppError> {
    let mut crypto = state.crypto.lock().unwrap();
    if !crypto.is_initialized() {
        return Err(AppError::BadRequest("No password configured".to_string()));
    }

    let ok = crypto.verify_and_unlock(&req.password)?;
    if ok {
        Ok(StatusCode::OK)
    } else {
        Err(AppError::Unauthorized)
    }
}

#[utoipa::path(
    post,
    path = "/api/auth/change-password",
    tag = "auth",
    security(
        ("bearer_auth" = [])
    ),
    request_body = ChangePasswordRequest,
    responses(
        (status = 200, description = "Changes password while keeping encrypted data intact"),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Password setup required", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn auth_change_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<StatusCode, AppError> {
    ensure_authorized(state.as_ref(), &headers)?;

    if req.new_password.len() < 8 {
        return Err(AppError::BadRequest(
            "New password must be at least 8 characters".to_string(),
        ));
    }

    let mut crypto = state.crypto.lock().unwrap();
    crypto.change_password(&req.old_password, &req.new_password)?;
    Ok(StatusCode::OK)
}

#[utoipa::path(
    post,
    path = "/api/auth/lock",
    tag = "auth",
    security(
        ("bearer_auth" = [])
    ),
    responses(
        (status = 200, description = "Locks encrypted data in memory"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Password setup required", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn auth_lock(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    ensure_authorized(state.as_ref(), &headers)?;
    let mut crypto = state.crypto.lock().unwrap();
    crypto.lock();
    Ok(StatusCode::OK)
}

// ---------------------------------------------------------------------------
// Data endpoints
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/health",
    tag = "system",
    responses(
        (status = 200, description = "Server health and version. Includes entry count only when authorized.", body = HealthResponse)
    )
)]
async fn health(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Json<HealthResponse> {
    // Only include entry count when the caller is authenticated
    let entries_count = if is_authorized(state.as_ref(), &headers) {
        Some(state.storage.count_entries().unwrap_or(0))
    } else {
        None
    };
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        entries_count,
    })
}

#[utoipa::path(
    post,
    path = "/api/entries",
    tag = "entries",
    security(
        ("bearer_auth" = [])
    ),
    request_body = CreateEntryRequest,
    responses(
        (status = 201, description = "Created a new entry", body = CreateEntryResponse),
        (status = 200, description = "Entry already existed (deduplicated by hash)", body = CreateEntryResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Password setup required", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn create_entry(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateEntryRequest>,
) -> Result<(StatusCode, Json<CreateEntryResponse>), AppError> {
    ensure_authorized(state.as_ref(), &headers)?;

    let dek = get_dek(&state);

    let (entry, created) = state.storage.create_entry(
        req.content_type,
        req.text_content.as_deref(),
        req.blob_base64.as_deref(),
        req.source_app.as_deref(),
        req.starred,
        &req.content_hash,
        dek.as_ref(),
    )?;

    let blob_url = entry
        .blob_hash
        .as_ref()
        .map(|_| format!("/api/entries/{}/blob", entry.id));

    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };

    Ok((
        status,
        Json(CreateEntryResponse {
            entry: EntryResponse {
                blob_url,
                entry: mask_sensitive_entry(entry),
            },
            created,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/entries",
    tag = "entries",
    security(
        ("bearer_auth" = [])
    ),
    params(ListEntriesParams),
    responses(
        (status = 200, description = "List entries with pagination and filtering", body = ListEntriesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Password setup required", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn list_entries(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<ListEntriesParams>,
) -> Result<Json<ListEntriesResponse>, AppError> {
    ensure_authorized(state.as_ref(), &headers)?;

    let dek = get_dek(&state);
    let limit = copywraith_core::api_types::clamp_limit(params.limit);

    let (entries, total) = state.storage.list_entries(
        limit,
        params.offset,
        params.content_type,
        params.starred_only,
        params.search.as_deref(),
        dek.as_ref(),
    )?;

    let has_more = (params.offset + limit) < total as u32;
    let entries = entries
        .into_iter()
        .map(|e| {
            let blob_url = e
                .blob_hash
                .as_ref()
                .map(|_| format!("/api/entries/{}/blob", e.id));
            EntryResponse {
                blob_url,
                entry: mask_sensitive_entry(e),
            }
        })
        .collect();

    Ok(Json(ListEntriesResponse {
        entries,
        total,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/entries/{id}",
    tag = "entries",
    security(
        ("bearer_auth" = [])
    ),
    params(
        ("id" = String, Path, description = "Entry ID")
    ),
    responses(
        (status = 200, description = "Returns one entry", body = EntryResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Password setup required", body = ErrorResponse),
        (status = 404, description = "Entry not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn get_entry(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<EntryResponse>, AppError> {
    ensure_authorized(state.as_ref(), &headers)?;

    let dek = get_dek(&state);
    let entry = state
        .storage
        .get_entry(&id, dek.as_ref())?
        .ok_or(AppError::NotFound)?;

    let blob_url = entry
        .blob_hash
        .as_ref()
        .map(|_| format!("/api/entries/{}/blob", entry.id));

    Ok(Json(EntryResponse {
        blob_url,
        entry: mask_sensitive_entry(entry),
    }))
}

#[utoipa::path(
    patch,
    path = "/api/entries/{id}",
    tag = "entries",
    security(
        ("bearer_auth" = [])
    ),
    params(
        ("id" = String, Path, description = "Entry ID")
    ),
    request_body = UpdateEntryRequest,
    responses(
        (status = 200, description = "Updated entry", body = EntryResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Password setup required", body = ErrorResponse),
        (status = 404, description = "Entry not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn update_entry(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateEntryRequest>,
) -> Result<Json<EntryResponse>, AppError> {
    ensure_authorized(state.as_ref(), &headers)?;

    if let Some(starred) = req.starred {
        state.storage.update_entry_starred(&id, starred)?;
    }

    let dek = get_dek(&state);
    let entry = state
        .storage
        .get_entry(&id, dek.as_ref())?
        .ok_or(AppError::NotFound)?;

    let blob_url = entry
        .blob_hash
        .as_ref()
        .map(|_| format!("/api/entries/{}/blob", entry.id));

    Ok(Json(EntryResponse {
        blob_url,
        entry: mask_sensitive_entry(entry),
    }))
}

#[utoipa::path(
    delete,
    path = "/api/entries/{id}",
    tag = "entries",
    security(
        ("bearer_auth" = [])
    ),
    params(
        ("id" = String, Path, description = "Entry ID")
    ),
    responses(
        (status = 204, description = "Entry deleted"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Password setup required", body = ErrorResponse),
        (status = 404, description = "Entry not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn delete_entry(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    ensure_authorized(state.as_ref(), &headers)?;

    let deleted = state.storage.delete_entry(&id)?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound)
    }
}

#[utoipa::path(
    get,
    path = "/api/entries/{id}/blob",
    tag = "entries",
    security(
        ("bearer_auth" = [])
    ),
    params(
        ("id" = String, Path, description = "Entry ID")
    ),
    responses(
        (status = 200, description = "Raw blob bytes"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Password setup required", body = ErrorResponse),
        (status = 404, description = "Entry/blob not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn get_blob(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    ensure_authorized(state.as_ref(), &headers)?;

    let dek = get_dek(&state);
    let entry = state
        .storage
        .get_entry(&id, dek.as_ref())?
        .ok_or(AppError::NotFound)?;

    let hash = entry.blob_hash.ok_or(AppError::NotFound)?;
    let raw_data = state.storage.get_blob(&hash)?.ok_or(AppError::NotFound)?;

    // Decrypt blob if encryption is active
    let data = if let Some(ref dek) = dek {
        crypto::decrypt_blob(dek, &raw_data)?
    } else {
        raw_data
    };

    let content_type = match entry.content_type {
        ContentType::Image => {
            let format = copywraith_core::content::detect_image_format(&data);
            match format {
                Some("png") => "image/png",
                Some("jpeg") => "image/jpeg",
                Some("gif") => "image/gif",
                Some("webp") => "image/webp",
                _ => "application/octet-stream",
            }
        }
        _ => "application/octet-stream",
    };

    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, content_type)],
        data,
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum AppError {
    Unauthorized,
    NotFound,
    BadRequest(String),
    SetupRequired,
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found".to_string()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::SetupRequired => (
                StatusCode::FORBIDDEN,
                "Password not configured. Set up a password first.".to_string(),
            ),
            AppError::Internal(err) => {
                tracing::error!("Internal error: {:?}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };

        (status, Json(ErrorResponse { error: message })).into_response()
    }
}

// ---------------------------------------------------------------------------
// Auth helpers
// ---------------------------------------------------------------------------

/// Extract password from Bearer token and verify.
/// Password must be configured; if not, returns 403 (setup required).
fn ensure_authorized(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let mut crypto = state.crypto.lock().unwrap();

    if !crypto.is_initialized() {
        return Err(AppError::SetupRequired);
    }

    let password = extract_bearer(headers).ok_or(AppError::Unauthorized)?;

    let ok = crypto
        .verify_and_unlock(password)
        .map_err(AppError::Internal)?;
    if !ok {
        return Err(AppError::Unauthorized);
    }

    Ok(())
}

/// Non-failing auth check: returns true only when password is configured,
/// a valid Bearer token is present, and the password verifies.
fn is_authorized(state: &AppState, headers: &HeaderMap) -> bool {
    let mut crypto = state.crypto.lock().unwrap();
    if !crypto.is_initialized() {
        return false;
    }
    let password = match extract_bearer(headers) {
        Some(p) => p,
        None => return false,
    };
    crypto.verify_and_unlock(password).unwrap_or(false)
}

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| {
            h.strip_prefix("Bearer ")
                .or_else(|| h.strip_prefix("bearer "))
        })
}

/// Get the DEK. Returns `Some` after `ensure_authorized` has succeeded.
fn get_dek(state: &AppState) -> Option<[u8; 32]> {
    let crypto = state.crypto.lock().unwrap();
    crypto.get_dek()
}

/// Mask `text_content` on sensitive entries so the server never sends
/// the full secret over the wire. Shows first 3 characters + bullets.
fn mask_sensitive_entry(
    mut entry: copywraith_core::models::ClipboardEntry,
) -> copywraith_core::models::ClipboardEntry {
    if entry.sensitive {
        entry.text_content = entry
            .text_content
            .map(|text| copywraith_core::content::mask_sensitive(&text, 60));
    }
    entry
}

/// Encrypt all existing unencrypted entries and blobs in place.
fn migrate_existing_data(state: &AppState, dek: &[u8; 32]) -> anyhow::Result<()> {
    state.storage.encrypt_all_entries(dek)?;
    state.storage.encrypt_all_blobs(dek)?;
    tracing::info!("Migrated existing data to encrypted storage");
    Ok(())
}
