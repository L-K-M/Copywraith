use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use copywraith_core::api_types::{CreateEntryRequest, EntryResponse, ListEntriesResponse};
use copywraith_core::content::{bytes_to_base64, hash_bytes};
use copywraith_core::models::{ClipboardEntry, ClipboardFlavors, ContentType};
use serde::Serialize;

use crate::{models::Settings, storage::LocalStorage};

#[derive(Debug, Clone, Serialize)]
pub struct SyncEndpointStatus {
    pub state: String,
    pub role: Option<String>,
    pub url: Option<String>,
    pub message: Option<String>,
    pub checked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullSyncResult {
    pub pulled: usize,
    pub endpoint_status: SyncEndpointStatus,
}

struct FetchEntriesResult {
    page: ListEntriesResponse,
    endpoint_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointRole {
    Local,
    Vpn,
}

impl EndpointRole {
    fn as_str(self) -> &'static str {
        match self {
            EndpointRole::Local => "local",
            EndpointRole::Vpn => "vpn",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerEndpoint {
    role: EndpointRole,
    url: String,
}

impl SyncEndpointStatus {
    fn disabled() -> Self {
        Self {
            state: "disabled".to_string(),
            role: None,
            url: None,
            message: Some("No server URL is configured in Settings.".to_string()),
            checked_at: Some(now_rfc3339()),
        }
    }

    pub fn unreachable_endpoint(endpoint: &ServerEndpoint, message: impl Into<String>) -> Self {
        Self {
            state: "unreachable".to_string(),
            role: Some(endpoint.role.as_str().to_string()),
            url: Some(endpoint.url.clone()),
            message: Some(message.into()),
            checked_at: Some(now_rfc3339()),
        }
    }

    fn online(endpoint: &ServerEndpoint) -> Self {
        Self {
            state: "online".to_string(),
            role: Some(endpoint.role.as_str().to_string()),
            url: Some(endpoint.url.clone()),
            message: Some("Last sync check completed successfully.".to_string()),
            checked_at: Some(now_rfc3339()),
        }
    }
}

fn checking_status_for_endpoint(
    endpoint: Option<&ServerEndpoint>,
    message: impl Into<String>,
) -> SyncEndpointStatus {
    SyncEndpointStatus {
        state: "checking".to_string(),
        role: endpoint.map(|endpoint| endpoint.role.as_str().to_string()),
        url: endpoint.map(|endpoint| endpoint.url.clone()),
        message: Some(message.into()),
        checked_at: Some(now_rfc3339()),
    }
}

pub fn checking_status(storage: &LocalStorage, message: impl Into<String>) -> SyncEndpointStatus {
    let settings = storage.get_settings();
    let server_urls = configured_server_urls(&settings);
    checking_status_for_endpoint(server_urls.first(), message)
}

pub fn first_configured_endpoint(storage: &LocalStorage) -> Option<ServerEndpoint> {
    let settings = storage.get_settings();
    configured_server_urls(&settings).into_iter().next()
}

pub fn checking_status_for_configured_endpoint(
    endpoint: Option<&ServerEndpoint>,
    message: impl Into<String>,
) -> SyncEndpointStatus {
    checking_status_for_endpoint(endpoint, message)
}

struct PullState {
    initialized: bool,
    /// Newest `(updated_at, id)` we have fully pulled. Entries at or below this
    /// key are considered already synced. Comparing the full key (rather than a
    /// single id) keeps the cursor stable when an entry's `updated_at` changes.
    watermark: Option<(DateTime<Utc>, String)>,
}

#[derive(Debug, Clone)]
struct EndpointHeartbeat {
    endpoint: ServerEndpoint,
    observed_at: Instant,
}

pub struct SyncClient {
    http: reqwest::Client,
    pull_state: Mutex<PullState>,
    last_responding_endpoint: Mutex<Option<EndpointHeartbeat>>,
}

impl SyncClient {
    pub fn new(storage: &LocalStorage) -> Self {
        // Restore persisted watermark so we don't re-scan the entire server on
        // restart. A missing/legacy/unparseable watermark falls back to a full
        // (re)sync, which is safe because ingestion is idempotent.
        let watermark = storage.get_sync_watermark().and_then(|(updated_at, id)| {
            DateTime::parse_from_rfc3339(&updated_at)
                .ok()
                .map(|dt| (dt.with_timezone(&Utc), id))
        });
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            pull_state: Mutex::new(PullState {
                initialized: watermark.is_some(),
                watermark,
            }),
            last_responding_endpoint: Mutex::new(None),
        }
    }

    fn note_responding_endpoint(&self, endpoint: &ServerEndpoint) {
        let mut status = self.last_responding_endpoint.lock().unwrap();
        *status = Some(EndpointHeartbeat {
            endpoint: endpoint.clone(),
            observed_at: Instant::now(),
        });
    }

    pub fn reset_pull_cursor(&self, storage: &LocalStorage) {
        {
            let mut state = self.pull_state.lock().unwrap();
            state.initialized = false;
            state.watermark = None;
        }

        if let Err(e) = storage.clear_sync_watermark() {
            log::warn!("Failed to clear sync watermark: {}", e);
        }
    }

    fn recent_responding_status(&self) -> Option<SyncEndpointStatus> {
        const MAX_STATUS_AGE: Duration = Duration::from_secs(30);

        let status = self.last_responding_endpoint.lock().unwrap();
        let heartbeat = status.as_ref()?;
        if heartbeat.observed_at.elapsed() > MAX_STATUS_AGE {
            return None;
        }

        Some(SyncEndpointStatus::online(&heartbeat.endpoint))
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

        let flavors = entry.resolved_flavors();

        let content_hash = flavors.payload_hash(entry.content_type, entry.blob_hash.as_deref());

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
            text_content: flavors.to_legacy_text_content(entry.content_type),
            flavors: if flavors.is_empty() {
                None
            } else {
                Some(flavors.clone())
            },
            blob_base64,
            source_app: entry.source_app.clone(),
            starred: Some(entry.starred),
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

        let (initialized, watermark) = {
            let state = self.pull_state.lock().unwrap();
            (state.initialized, state.watermark.clone())
        };

        let mut before_cursor: Option<(String, String)> = None;
        let mut pulled = 0usize;
        // Newest (updated_at, id) observed this pass. Promoted to the watermark
        // once the pass finishes without a blocking ingest error.
        let mut newest_seen: Option<(DateTime<Utc>, String)> = None;
        let mut had_ingest_error = false;
        let mut active_endpoint: Option<ServerEndpoint> = None;

        loop {
            let Some(fetch_result) = self
                .fetch_entries_page_with_fallback(
                    &server_urls,
                    &api_key,
                    PAGE_SIZE,
                    before_cursor
                        .as_ref()
                        .map(|(updated_at, id)| (updated_at.as_str(), id.as_str())),
                )
                .await?
            else {
                let endpoint_status = self.recent_responding_status().unwrap_or_else(|| {
                    let attempted = server_urls
                        .first()
                        .expect("server_urls is non-empty after sync config check");
                    SyncEndpointStatus::unreachable_endpoint(
                        attempted,
                        "No configured server endpoint responded while pulling entries.",
                    )
                });

                return Ok(PullSyncResult {
                    pulled,
                    endpoint_status,
                });
            };

            let page = fetch_result.page;
            let used_index = fetch_result.endpoint_index;

            if used_index > 0 {
                server_urls.swap(0, used_index);
            }

            if active_endpoint.is_none() {
                active_endpoint = Some(server_urls[0].clone());
            }

            if page.entries.is_empty() {
                break;
            }

            // The first entry of the first page is the newest the server has.
            if newest_seen.is_none() {
                let first = &page.entries[0].entry;
                newest_seen = Some((first.updated_at, first.id.clone()));
            }

            let mut reached_watermark = false;

            for remote in &page.entries {
                if initialized {
                    if let Some((wm_updated_at, wm_id)) = watermark.as_ref() {
                        let entry_key = (remote.entry.updated_at, remote.entry.id.as_str());
                        if entry_key <= (*wm_updated_at, wm_id.as_str()) {
                            // We've reached entries we already pulled. Because the
                            // page is sorted by (updated_at DESC, id DESC), every
                            // remaining entry is also at or below the watermark.
                            reached_watermark = true;
                            break;
                        }
                    }
                }

                match self
                    .ingest_remote_entry(&server_urls, &api_key, remote, storage)
                    .await
                {
                    Ok(true) => pulled += 1,
                    Ok(false) => {}
                    Err(e) => {
                        log::warn!("Failed to ingest remote entry {}: {}", remote.entry.id, e);
                        had_ingest_error = true;
                    }
                }
            }

            if reached_watermark || !page.has_more {
                break;
            }

            let Some(last_entry) = page.entries.last() else {
                break;
            };

            before_cursor = Some((
                last_entry.entry.updated_at.to_rfc3339(),
                last_entry.entry.id.clone(),
            ));
        }

        // Advance the watermark to the newest entry we saw, but only when the
        // pass had no blocking ingest error (so a transient failure is retried
        // next time) and only forward (never move the watermark backwards, e.g.
        // if the previous newest entry was deleted on the server).
        if let Some((updated_at, id)) = newest_seen.filter(|_| !had_ingest_error) {
            let advanced = {
                let mut state = self.pull_state.lock().unwrap();
                let should_advance = match state.watermark.as_ref() {
                    Some((wm_updated_at, wm_id)) => {
                        (updated_at, id.as_str()) > (*wm_updated_at, wm_id.as_str())
                    }
                    None => true,
                };
                if should_advance {
                    state.watermark = Some((updated_at, id.clone()));
                }
                state.initialized = true;
                should_advance
            };

            // Persist outside the in-memory lock so it survives app restarts.
            if advanced {
                if let Err(e) = storage.save_sync_watermark(&updated_at.to_rfc3339(), &id) {
                    log::warn!("Failed to persist sync watermark: {}", e);
                }
            }
        }

        let endpoint_status = active_endpoint
            .as_ref()
            .map(SyncEndpointStatus::online)
            .or_else(|| self.recent_responding_status())
            .unwrap_or_else(|| {
                let attempted = server_urls
                    .first()
                    .expect("server_urls is non-empty after sync config check");
                SyncEndpointStatus::unreachable_endpoint(
                    attempted,
                    "No configured server endpoint responded while pulling entries.",
                )
            });

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
                Ok(response) => {
                    self.note_responding_endpoint(endpoint);

                    if response.status().is_success() {
                        return true;
                    }

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
        before_cursor: Option<(&str, &str)>,
    ) -> anyhow::Result<Option<FetchEntriesResult>> {
        for (index, endpoint) in server_urls.iter().enumerate() {
            let mut url = match reqwest::Url::parse(&format!("{}/api/entries", endpoint.url)) {
                Ok(url) => url,
                Err(e) => {
                    log::warn!("Invalid server URL {}: {}", endpoint.url, e);
                    continue;
                }
            };

            {
                let mut query = url.query_pairs_mut();
                query.append_pair("limit", &page_size.to_string());
                query.append_pair("offset", "0");
                // Native sync needs the original payload. The server masks
                // sensitive entries by default for presentation clients.
                query.append_pair("include_sensitive", "true");
                if let Some((before_updated_at, before_id)) = before_cursor {
                    query.append_pair("before_updated_at", before_updated_at);
                    query.append_pair("before_id", before_id);
                }
            }

            let mut request = self.http.get(url);
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

            self.note_responding_endpoint(endpoint);

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
                Ok(page) => {
                    return Ok(Some(FetchEntriesResult {
                        page,
                        endpoint_index: index,
                    }))
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse entries response from {}: {}",
                        endpoint.url,
                        e
                    );
                }
            }
        }

        Ok(None)
    }

    async fn ingest_remote_entry(
        &self,
        server_urls: &[ServerEndpoint],
        api_key: &str,
        remote: &EntryResponse,
        storage: &LocalStorage,
    ) -> anyhow::Result<bool> {
        let mut blob_data: Option<Vec<u8>> = None;
        let remote_flavors = resolved_remote_flavors(&remote.entry);

        let content_hash = if let Some(hash) = remote.entry.blob_hash.as_deref() {
            remote_flavors.payload_hash(remote.entry.content_type, Some(hash))
        } else if remote.entry.content_type == ContentType::Image {
            let Some(data) = self.fetch_blob_data(server_urls, api_key, remote).await? else {
                return Ok(false);
            };
            if data.is_empty() {
                return Ok(false);
            }
            let hash = hash_bytes(&data);
            blob_data = Some(data);
            remote_flavors.payload_hash(ContentType::Image, Some(&hash))
        } else {
            remote_flavors.payload_hash(remote.entry.content_type, None)
        };

        if storage.has_content_hash(&content_hash)? {
            return storage
                .apply_remote_star_state_by_content_hash(&content_hash, remote.entry.starred);
        }

        if remote.entry.blob_hash.is_some() && blob_data.is_none() {
            let Some(data) = self.fetch_blob_data(server_urls, api_key, remote).await? else {
                return Ok(false);
            };
            if data.is_empty() {
                return Ok(false);
            }

            let actual_hash = hash_bytes(&data);
            if let Some(expected_hash) = remote.entry.blob_hash.as_deref() {
                if actual_hash != expected_hash {
                    log::warn!(
                        "Skipping remote blob entry {} due to hash mismatch",
                        remote.entry.id
                    );
                    return Ok(false);
                }
            } else if remote.entry.content_type == ContentType::Image && actual_hash != content_hash
            {
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
            &remote_flavors,
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

    /// Download a remote entry's blob.
    ///
    /// Returns `Ok(Some(bytes))` on success, `Ok(None)` when the blob is
    /// definitively unavailable (a reachable server answered with a non-success
    /// status, e.g. the blob was deleted) so the caller can skip the entry
    /// without blocking the sync watermark, and `Err` only for transient
    /// failures (no server reachable / read error) that are worth retrying.
    async fn fetch_blob_data(
        &self,
        server_urls: &[ServerEndpoint],
        api_key: &str,
        remote: &EntryResponse,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let mut last_error: Option<anyhow::Error> = None;
        let mut saw_definitive_unavailable = false;

        for (index, endpoint) in server_urls.iter().enumerate() {
            let blob_url = remote
                .blob_url
                .as_deref()
                .map(|url| resolve_url(&endpoint.url, url))
                .unwrap_or_else(|| {
                    format!("{}/api/entries/{}/blob", endpoint.url, remote.entry.id)
                });

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
                // A reachable server answered but the blob is not available.
                // Treat this as a definitive (non-retryable) miss.
                saw_definitive_unavailable = true;
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
                Ok(bytes) => return Ok(Some(bytes.to_vec())),
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

        // Every reachable server returned a non-success status: the blob is gone.
        // Skip the entry instead of pinning the watermark forever.
        if saw_definitive_unavailable {
            log::warn!(
                "Blob for {} is unavailable on all reachable servers; skipping entry",
                remote.entry.id
            );
            return Ok(None);
        }

        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("Failed to download blob for {}", remote.entry.id)))
    }
}

fn now_rfc3339() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    chrono::DateTime::<chrono::Utc>::from(UNIX_EPOCH + duration).to_rfc3339()
}

fn resolved_remote_flavors(entry: &ClipboardEntry) -> ClipboardFlavors {
    entry
        .flavors
        .clone()
        .merge_legacy(entry.content_type, entry.text_content.as_deref())
}

fn configured_server_urls(settings: &Settings) -> Vec<ServerEndpoint> {
    let mut urls: Vec<ServerEndpoint> = Vec::new();

    for (raw, role) in [
        (&settings.server_url_primary, EndpointRole::Local),
        (&settings.server_url_fallback, EndpointRole::Vpn),
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
