use std::sync::Mutex;

use copywraith_core::api_types::{CreateEntryRequest, EntryResponse, ListEntriesResponse};
use copywraith_core::content::{bytes_to_base64, hash_bytes, hash_text};
use copywraith_core::models::{ClipboardEntry, ContentType};
use serde::Serialize;

use crate::{models::Settings, storage::LocalStorage};

#[derive(Debug, Clone, Serialize)]
pub struct SyncEndpointStatus {
    pub state: String,
    pub role: Option<String>,
    pub url: Option<String>,
}

pub struct PullSyncResult {
    pub pulled: usize,
    pub endpoint_status: SyncEndpointStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointRole {
    Primary,
    Fallback,
}

impl EndpointRole {
    fn as_str(self) -> &'static str {
        match self {
            EndpointRole::Primary => "primary",
            EndpointRole::Fallback => "fallback",
        }
    }
}

#[derive(Debug, Clone)]
struct ServerEndpoint {
    role: EndpointRole,
    url: String,
}

impl SyncEndpointStatus {
    fn disabled() -> Self {
        Self {
            state: "disabled".to_string(),
            role: None,
            url: None,
        }
    }

    fn unreachable() -> Self {
        Self {
            state: "unreachable".to_string(),
            role: None,
            url: None,
        }
    }

    fn online(endpoint: &ServerEndpoint) -> Self {
        Self {
            state: "online".to_string(),
            role: Some(endpoint.role.as_str().to_string()),
            url: Some(endpoint.url.clone()),
        }
    }
}

struct PullState {
    initialized: bool,
    last_seen_server_id: Option<String>,
}

pub struct SyncClient {
    http: reqwest::Client,
    pull_state: Mutex<PullState>,
}

impl SyncClient {
    pub fn new(storage: &LocalStorage) -> Self {
        // Restore persisted sync cursor so we don't re-scan the entire server on restart
        let persisted_cursor = storage.get_sync_cursor();
        Self {
            http: reqwest::Client::new(),
            pull_state: Mutex::new(PullState {
                initialized: persisted_cursor.is_some(),
                last_seen_server_id: persisted_cursor,
            }),
        }
    }

    pub async fn sync_unsynced_entries(&self, storage: &LocalStorage) {
        let entries = match storage.get_unsynced_entries() {
            Ok(entries) => entries,
            Err(e) => {
                log::error!("Failed to read unsynced entries: {}", e);
                return;
            }
        };

        for entry in entries {
            self.sync_entry(&entry, storage).await;
        }
    }

    pub async fn sync_entry(&self, entry: &ClipboardEntry, storage: &LocalStorage) {
        let settings = storage.get_settings();
        let server_urls = configured_server_urls(&settings);
        if server_urls.is_empty() {
            return; // No server configured, skip sync
        }

        let content_hash = if let Some(ref text) = entry.text_content {
            hash_text(text)
        } else if let Some(ref hash) = entry.blob_hash {
            hash.clone()
        } else {
            return;
        };

        let blob_base64 = if let Some(ref hash) = entry.blob_hash {
            storage
                .get_blob(hash)
                .ok()
                .flatten()
                .map(|data| bytes_to_base64(&data))
        } else {
            None
        };

        let req = CreateEntryRequest {
            content_type: entry.content_type,
            text_content: entry.text_content.clone(),
            blob_base64,
            source_app: entry.source_app.clone(),
            content_hash,
        };

        let synced = self
            .push_entry_with_fallback(&server_urls, &settings.api_key, &req, &entry.id)
            .await;

        if synced {
            if let Err(e) = storage.mark_synced(&entry.id) {
                log::error!("Failed to mark entry as synced: {}", e);
            }
        }
    }

    pub async fn pull_new_entries(&self, storage: &LocalStorage) -> anyhow::Result<PullSyncResult> {
        const PAGE_SIZE: u32 = 100;

        let settings = storage.get_settings();
        let mut server_urls = configured_server_urls(&settings);
        if server_urls.is_empty() {
            return Ok(PullSyncResult {
                pulled: 0,
                endpoint_status: SyncEndpointStatus::disabled(),
            });
        }

        let api_key = settings.api_key;

        let (initialized, last_seen_server_id) = {
            let state = self.pull_state.lock().unwrap();
            (state.initialized, state.last_seen_server_id.clone())
        };

        let mut offset = 0;
        let mut pulled = 0usize;
        let mut cursor_after_sync: Option<String> = None;
        let mut active_endpoint: Option<ServerEndpoint> = None;

        loop {
            let Some((page, used_index)) = self
                .fetch_entries_page_with_fallback(&server_urls, &api_key, PAGE_SIZE, offset)
                .await?
            else {
                return Ok(PullSyncResult {
                    pulled,
                    endpoint_status: SyncEndpointStatus::unreachable(),
                });
            };

            if used_index > 0 {
                server_urls.swap(0, used_index);
            }

            if active_endpoint.is_none() {
                active_endpoint = Some(server_urls[0].clone());
            }

            if page.entries.is_empty() {
                break;
            }

            if cursor_after_sync.is_none() {
                cursor_after_sync = Some(page.entries[0].entry.id.clone());
            }

            let mut reached_cursor = false;

            for remote in &page.entries {
                if initialized && last_seen_server_id.as_deref() == Some(remote.entry.id.as_str()) {
                    reached_cursor = true;
                    break;
                }

                match self
                    .ingest_remote_entry(&server_urls, &api_key, remote, storage)
                    .await
                {
                    Ok(true) => pulled += 1,
                    Ok(false) => {}
                    Err(e) => {
                        log::warn!("Failed to ingest remote entry {}: {}", remote.entry.id, e);
                    }
                }
            }

            if reached_cursor || !page.has_more {
                break;
            }

            offset += PAGE_SIZE;
        }

        if let Some(cursor) = cursor_after_sync {
            let mut state = self.pull_state.lock().unwrap();
            state.last_seen_server_id = Some(cursor.clone());
            state.initialized = true;
            // Persist cursor so it survives app restarts
            if let Err(e) = storage.save_sync_cursor(&cursor) {
                log::warn!("Failed to persist sync cursor: {}", e);
            }
        }

        let endpoint_status = active_endpoint
            .as_ref()
            .map(SyncEndpointStatus::online)
            .unwrap_or_else(SyncEndpointStatus::unreachable);

        Ok(PullSyncResult {
            pulled,
            endpoint_status,
        })
    }

    async fn push_entry_with_fallback(
        &self,
        server_urls: &[ServerEndpoint],
        api_key: &str,
        req: &CreateEntryRequest,
        entry_id: &str,
    ) -> bool {
        for (index, endpoint) in server_urls.iter().enumerate() {
            let url = format!("{}/api/entries", endpoint.url);
            let mut request = self.http.post(&url).json(req);

            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }

            match request.send().await {
                Ok(response) if response.status().is_success() => return true,
                Ok(response) => {
                    if index + 1 < server_urls.len() {
                        log::debug!(
                            "Server {} returned {} when syncing entry {}; trying fallback",
                            endpoint.url,
                            response.status(),
                            entry_id
                        );
                    } else {
                        log::warn!(
                            "Server {} returned {} when syncing entry {}",
                            endpoint.url,
                            response.status(),
                            entry_id
                        );
                    }
                }
                Err(e) => {
                    if index + 1 < server_urls.len() {
                        log::debug!(
                            "Failed syncing entry {} via {}: {} (trying fallback)",
                            entry_id,
                            endpoint.url,
                            e
                        );
                    } else {
                        log::debug!(
                            "Failed syncing entry {} via {} (will retry): {}",
                            entry_id,
                            endpoint.url,
                            e
                        );
                    }
                }
            }
        }

        false
    }

    async fn fetch_entries_page_with_fallback(
        &self,
        server_urls: &[ServerEndpoint],
        api_key: &str,
        page_size: u32,
        offset: u32,
    ) -> anyhow::Result<Option<(ListEntriesResponse, usize)>> {
        let mut parse_error: Option<anyhow::Error> = None;

        for (index, endpoint) in server_urls.iter().enumerate() {
            let url = format!(
                "{}/api/entries?limit={}&offset={}",
                endpoint.url, page_size, offset
            );

            let mut request = self.http.get(&url);
            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(e) => {
                    if index + 1 < server_urls.len() {
                        log::debug!(
                            "Failed to fetch entries from {}: {} (trying fallback)",
                            endpoint.url,
                            e
                        );
                    } else {
                        log::debug!("Failed to fetch entries from {}: {}", endpoint.url, e);
                    }
                    continue;
                }
            };

            if !response.status().is_success() {
                if index + 1 < server_urls.len() {
                    log::debug!(
                        "Server {} returned {} when pulling entries; trying fallback",
                        endpoint.url,
                        response.status()
                    );
                } else {
                    log::warn!(
                        "Server {} returned {} when pulling entries",
                        endpoint.url,
                        response.status()
                    );
                }
                continue;
            }

            match response.json::<ListEntriesResponse>().await {
                Ok(page) => return Ok(Some((page, index))),
                Err(e) => {
                    let error_message = e.to_string();
                    parse_error = Some(e.into());
                    if index + 1 < server_urls.len() {
                        log::warn!(
                            "Failed to parse entries response from {}: {} (trying fallback)",
                            endpoint.url,
                            error_message
                        );
                    }
                }
            }
        }

        if let Some(err) = parse_error {
            Err(err)
        } else {
            Ok(None)
        }
    }

    async fn ingest_remote_entry(
        &self,
        server_urls: &[ServerEndpoint],
        api_key: &str,
        remote: &EntryResponse,
        storage: &LocalStorage,
    ) -> anyhow::Result<bool> {
        let mut blob_data: Option<Vec<u8>> = None;

        let content_hash = match remote.entry.content_type {
            ContentType::Image => {
                if let Some(hash) = remote.entry.blob_hash.clone() {
                    hash
                } else {
                    let data = self.fetch_blob_data(server_urls, api_key, remote).await?;
                    if data.is_empty() {
                        return Ok(false);
                    }
                    let hash = hash_bytes(&data);
                    blob_data = Some(data);
                    hash
                }
            }
            _ => {
                let Some(text) = remote.entry.text_content.as_ref() else {
                    return Ok(false);
                };
                hash_text(text)
            }
        };

        if storage.has_content_hash(&content_hash)? {
            return Ok(false);
        }

        if remote.entry.content_type == ContentType::Image && blob_data.is_none() {
            let data = self.fetch_blob_data(server_urls, api_key, remote).await?;
            if data.is_empty() {
                return Ok(false);
            }

            let actual_hash = hash_bytes(&data);
            if actual_hash != content_hash {
                log::warn!(
                    "Skipping remote image {} due to hash mismatch",
                    remote.entry.id
                );
                return Ok(false);
            }

            blob_data = Some(data);
        }

        let inserted = storage.insert_entry(
            remote.entry.content_type,
            remote.entry.text_content.as_deref(),
            blob_data.as_deref(),
            &content_hash,
            remote.entry.source_app.as_deref(),
        )?;

        let Some(local_entry) = inserted else {
            return Ok(false);
        };

        if remote.entry.starred {
            if let Err(e) = storage.set_starred(&local_entry.id, true) {
                log::warn!(
                    "Failed to apply starred flag for pulled entry {}: {}",
                    local_entry.id,
                    e
                );
            }
        }

        if let Err(e) = storage.mark_synced(&local_entry.id) {
            log::warn!(
                "Failed to mark pulled entry {} as synced: {}",
                local_entry.id,
                e
            );
        }

        Ok(true)
    }

    async fn fetch_blob_data(
        &self,
        server_urls: &[ServerEndpoint],
        api_key: &str,
        remote: &EntryResponse,
    ) -> anyhow::Result<Vec<u8>> {
        let mut last_error: Option<anyhow::Error> = None;

        for (index, endpoint) in server_urls.iter().enumerate() {
            let blob_url = remote
                .blob_url
                .as_deref()
                .map(|url| resolve_url(&endpoint.url, url))
                .unwrap_or_else(|| format!("{}/api/entries/{}/blob", endpoint.url, remote.entry.id));

            let mut request = self.http.get(&blob_url);
            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }

            let response = match request.send().await {
                Ok(response) => response,
                Err(e) => {
                    last_error = Some(anyhow::anyhow!(
                        "Failed to download blob for {} from {}: {}",
                        remote.entry.id,
                        endpoint.url,
                        e
                    ));

                    if index + 1 < server_urls.len() {
                        log::debug!(
                            "Failed to download blob for {} via {}: {} (trying fallback)",
                            remote.entry.id,
                            endpoint.url,
                            e
                        );
                    }
                    continue;
                }
            };

            if !response.status().is_success() {
                last_error = Some(anyhow::anyhow!(
                    "Server {} returned {} when downloading blob for {}",
                    endpoint.url,
                    response.status(),
                    remote.entry.id
                ));

                if index + 1 < server_urls.len() {
                    log::debug!(
                        "Server {} returned {} for blob {}; trying fallback",
                        endpoint.url,
                        response.status(),
                        remote.entry.id
                    );
                }
                continue;
            }

            match response.bytes().await {
                Ok(bytes) => return Ok(bytes.to_vec()),
                Err(e) => {
                    let error_message = e.to_string();
                    last_error = Some(e.into());
                    if index + 1 < server_urls.len() {
                        log::debug!(
                            "Failed reading blob bytes for {} from {}: {} (trying fallback)",
                            remote.entry.id,
                            endpoint.url,
                            error_message
                        );
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            anyhow::anyhow!("Failed to download blob for {}", remote.entry.id)
        }))
    }
}

fn configured_server_urls(settings: &Settings) -> Vec<ServerEndpoint> {
    let mut urls: Vec<ServerEndpoint> = Vec::new();

    for (raw, role) in [
        (&settings.server_url_primary, EndpointRole::Primary),
        (&settings.server_url_fallback, EndpointRole::Fallback),
    ] {
        let normalized = raw.trim().trim_end_matches('/');
        if normalized.is_empty() {
            continue;
        }

        if urls.iter().any(|existing| existing.url == normalized) {
            continue;
        }

        urls.push(ServerEndpoint {
            role,
            url: normalized.to_string(),
        });
    }

    urls
}

fn resolve_url(base_url: &str, maybe_relative: &str) -> String {
    if maybe_relative.starts_with("http://") || maybe_relative.starts_with("https://") {
        maybe_relative.to_string()
    } else {
        format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            maybe_relative.trim_start_matches('/'),
        )
    }
}
