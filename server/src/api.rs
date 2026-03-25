use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

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

// ---------------------------------------------------------------------------
// Auth endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AuthStatusResponse {
    initialized: bool,
    unlocked: bool,
}

#[derive(Deserialize)]
struct SetupRequest {
    password: String,
}

#[derive(Deserialize)]
struct UnlockRequest {
    password: String,
}

#[derive(Deserialize)]
struct ChangePasswordRequest {
    old_password: String,
    new_password: String,
}

async fn auth_status(State(state): State<Arc<AppState>>) -> Json<AuthStatusResponse> {
    let crypto = state.crypto.lock().unwrap();
    Json(AuthStatusResponse {
        initialized: crypto.is_initialized(),
        unlocked: crypto.is_unlocked(),
    })
}

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
    let dek = crypto.get_dek().ok_or_else(|| {
        anyhow::anyhow!("DEK not available after setup")
    })?;
    drop(crypto); // release crypto lock before touching storage

    migrate_existing_data(&state, &dek)?;

    Ok(StatusCode::OK)
}

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

async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let entries_count = state.storage.count_entries().unwrap_or(0);
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        entries_count,
    })
}

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
            entry: EntryResponse { blob_url, entry },
            created,
        }),
    ))
}

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
            EntryResponse { blob_url, entry: e }
        })
        .collect();

    Ok(Json(ListEntriesResponse {
        entries,
        total,
        has_more,
    }))
}

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

    Ok(Json(EntryResponse { blob_url, entry }))
}

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

    Ok(Json(EntryResponse { blob_url, entry }))
}

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

/// Encrypt all existing unencrypted entries and blobs in place.
fn migrate_existing_data(state: &AppState, dek: &[u8; 32]) -> anyhow::Result<()> {
    state.storage.encrypt_all_entries(dek)?;
    state.storage.encrypt_all_blobs(dek)?;
    tracing::info!("Migrated existing data to encrypted storage");
    Ok(())
}
