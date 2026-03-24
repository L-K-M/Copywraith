use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};

use copywraith_core::api_types::*;
use copywraith_core::models::ContentType;

use crate::AppState;

type AppRouter = Router<Arc<AppState>>;

pub fn router() -> AppRouter {
    Router::new()
        .route("/health", get(health))
        .route("/entries", post(create_entry))
        .route("/entries", get(list_entries))
        .route("/entries/{id}", get(get_entry))
        .route("/entries/{id}", patch(update_entry))
        .route("/entries/{id}", delete(delete_entry))
        .route("/entries/{id}/blob", get(get_blob))
}

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

    let (entry, created) = state.storage.create_entry(
        req.content_type,
        req.text_content.as_deref(),
        req.blob_base64.as_deref(),
        req.source_app.as_deref(),
        req.starred,
        &req.content_hash,
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

    let limit = copywraith_core::api_types::clamp_limit(params.limit);
    let (entries, total) = state.storage.list_entries(
        limit,
        params.offset,
        params.content_type,
        params.starred_only,
        params.search.as_deref(),
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

    let entry = state.storage.get_entry(&id)?.ok_or(AppError::NotFound)?;

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

    let entry = state.storage.get_entry(&id)?.ok_or(AppError::NotFound)?;

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

    let entry = state.storage.get_entry(&id)?.ok_or(AppError::NotFound)?;

    let hash = entry.blob_hash.ok_or(AppError::NotFound)?;
    let data = state.storage.get_blob(&hash)?.ok_or(AppError::NotFound)?;

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

// Error handling

#[derive(Debug)]
enum AppError {
    Unauthorized,
    NotFound,
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

fn ensure_authorized(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let Some(expected) = state.admin_api_key.as_deref() else {
        return Ok(());
    };

    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "));

    if token == Some(expected) {
        Ok(())
    } else {
        Err(AppError::Unauthorized)
    }
}
