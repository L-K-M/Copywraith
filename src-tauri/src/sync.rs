use std::sync::Mutex;

use copywraith_core::api_types::{CreateEntryRequest, EntryResponse, ListEntriesResponse};
use copywraith_core::content::{bytes_to_base64, hash_bytes, hash_text};
use copywraith_core::models::{ClipboardEntry, ContentType};

use crate::storage::LocalStorage;

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
        if settings.server_url.is_empty() {
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

        let url = format!("{}/api/entries", settings.server_url.trim_end_matches('/'));

        let mut request = self.http.post(&url).json(&req);

        if !settings.api_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", settings.api_key));
        }

        match request.send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    if let Err(e) = storage.mark_synced(&entry.id) {
                        log::error!("Failed to mark entry as synced: {}", e);
                    }
                } else {
                    log::warn!(
                        "Server returned {} when syncing entry {}",
                        resp.status(),
                        entry.id
                    );
                }
            }
            Err(e) => {
                log::debug!("Failed to sync entry to server (will retry): {}", e);
            }
        }
    }

    pub async fn pull_new_entries(&self, storage: &LocalStorage) -> anyhow::Result<usize> {
        const PAGE_SIZE: u32 = 100;

        let settings = storage.get_settings();
        if settings.server_url.is_empty() {
            return Ok(0);
        }

        let base_url = settings.server_url.trim_end_matches('/').to_string();
        let api_key = settings.api_key;

        let (initialized, last_seen_server_id) = {
            let state = self.pull_state.lock().unwrap();
            (state.initialized, state.last_seen_server_id.clone())
        };

        let mut offset = 0;
        let mut pulled = 0usize;
        let mut cursor_after_sync: Option<String> = None;

        loop {
            let url = format!(
                "{}/api/entries?limit={}&offset={}",
                base_url, PAGE_SIZE, offset
            );

            let mut request = self.http.get(&url);
            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }

            let response = match request.send().await {
                Ok(resp) => resp,
                Err(e) => {
                    log::debug!("Failed to fetch entries from server: {}", e);
                    return Ok(pulled);
                }
            };

            if !response.status().is_success() {
                log::warn!("Server returned {} when pulling entries", response.status());
                return Ok(pulled);
            }

            let page: ListEntriesResponse = response.json().await?;

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
                    .ingest_remote_entry(&base_url, &api_key, remote, storage)
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

        Ok(pulled)
    }

    async fn ingest_remote_entry(
        &self,
        base_url: &str,
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
                    let data = self.fetch_blob_data(base_url, api_key, remote).await?;
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
            let data = self.fetch_blob_data(base_url, api_key, remote).await?;
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
        base_url: &str,
        api_key: &str,
        remote: &EntryResponse,
    ) -> anyhow::Result<Vec<u8>> {
        let blob_url = remote
            .blob_url
            .as_deref()
            .map(|url| resolve_url(base_url, url))
            .unwrap_or_else(|| {
                format!(
                    "{}/api/entries/{}/blob",
                    base_url.trim_end_matches('/'),
                    remote.entry.id
                )
            });

        let mut request = self.http.get(&blob_url);
        if !api_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "Server returned {} when downloading blob for {}",
                response.status(),
                remote.entry.id
            );
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }
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
