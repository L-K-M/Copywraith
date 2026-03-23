use copywraith_core::api_types::CreateEntryRequest;
use copywraith_core::content::{bytes_to_base64, hash_text};
use copywraith_core::models::ClipboardEntry;

use crate::storage::LocalStorage;

pub struct SyncClient {
    http: reqwest::Client,
}

impl SyncClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
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
}
