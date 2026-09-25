use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use copywraith_core::api_types::{CreateEntryRequest, EntryResponse, ListEntriesResponse};
use copywraith_core::content::{bytes_to_base64, hash_bytes};
use copywraith_core::models::{ClipboardEntry, ClipboardFlavors, ContentType};
use serde::Serialize;

use crate::{models::Settings, storage::LocalStorage};

/// Where the most recent sync attempt ended up, as shown in the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Checking,
    Disabled,
    Online,
    /// No configured server could be reached.
    Unreachable,
    /// A server answered but refused the configured password.
    Unauthorized,
    /// A server answered with an error status or an unreadable reply.
    Error,
}

impl SyncState {
    /// States that mean sync is not working and the loop should back off.
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Unreachable | Self::Unauthorized | Self::Error)
    }
}

/// Why a server that did answer refused or failed a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rejection {
    /// HTTP 401: the password is wrong or missing.
    Unauthorized,
    /// HTTP 403: the Copywraith server answers this until a password is set up.
    Forbidden,
    /// Any other non-success status.
    Status(reqwest::StatusCode),
    /// A success status whose body is not a Copywraith response.
    UnreadableResponse,
}

impl Rejection {
    fn from_status(status: reqwest::StatusCode) -> Self {
        match status {
            reqwest::StatusCode::UNAUTHORIZED => Self::Unauthorized,
            reqwest::StatusCode::FORBIDDEN => Self::Forbidden,
            other => Self::Status(other),
        }
    }

    /// Credential problems fail every request the same way. Retrying the rest
    /// of a batch cannot succeed, and each attempt costs the server a full
    /// Argon2id password verification.
    fn is_credential_problem(self) -> bool {
        matches!(self, Self::Unauthorized | Self::Forbidden)
    }

    /// Whether this rejection explains a failure better than `earlier`, one
    /// from another endpoint. A credential refusal does, because it is what
    /// the user must fix and what pauses a push batch; otherwise the first
    /// endpoint's answer stands.
    fn outranks(self, earlier: Self) -> bool {
        self.is_credential_problem() && !earlier.is_credential_problem()
    }

    /// The more telling of this rejection and an `earlier` one, if any.
    fn outrank(self, earlier: Option<Self>) -> Self {
        match earlier {
            Some(earlier) if !self.outranks(earlier) => earlier,
            _ => self,
        }
    }
}

/// How pushing one entry ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PushOutcome {
    Synced,
    Rejected(Rejection),
    /// No configured endpoint accepted a connection.
    Unreachable,
    /// A connection was made but the request did not complete, for example
    /// because a large upload timed out. Specific to this entry.
    Failed,
}

/// How fetching one page of remote entries ended.
enum PageFetch {
    Page(FetchEntriesResult),
    Rejected {
        endpoint: ServerEndpoint,
        rejection: Rejection,
    },
    Unreachable,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncEndpointStatus {
    pub state: SyncState,
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
            state: SyncState::Disabled,
            role: None,
            url: None,
            message: Some("No server URL is configured in Settings.".to_string()),
            checked_at: Some(now_rfc3339()),
        }
    }

    pub fn unreachable_endpoint(endpoint: &ServerEndpoint, message: impl Into<String>) -> Self {
        Self {
            state: SyncState::Unreachable,
            role: Some(endpoint.role.as_str().to_string()),
            url: Some(endpoint.url.clone()),
            message: Some(message.into()),
            checked_at: Some(now_rfc3339()),
        }
    }

    fn rejected(endpoint: &ServerEndpoint, rejection: Rejection) -> Self {
        let (state, message) = match rejection {
            Rejection::Unauthorized => (
                SyncState::Unauthorized,
                "The server rejected the password. Check it in Settings.".to_string(),
            ),
            Rejection::Forbidden => (
                SyncState::Unauthorized,
                "The server refused access (HTTP 403). A new server needs a password set up in its admin page first.".to_string(),
            ),
            Rejection::Status(status) => (
                SyncState::Error,
                format!("The server answered with HTTP {status}."),
            ),
            Rejection::UnreadableResponse => (
                SyncState::Error,
                "The server's reply could not be read. Check that the URL points to a Copywraith server.".to_string(),
            ),
        };

        Self {
            state,
            role: Some(endpoint.role.as_str().to_string()),
            url: Some(endpoint.url.clone()),
            message: Some(message),
            checked_at: Some(now_rfc3339()),
        }
    }

    fn online(endpoint: &ServerEndpoint) -> Self {
        Self {
            state: SyncState::Online,
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
        state: SyncState::Checking,
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

        if entries.is_empty() {
            return;
        }

        // Resolve the endpoint configuration once for the whole batch. Reading
        // it per entry costs a database lock and seven queries each time, which
        // is significant when a first-run push has hundreds of entries queued.
        let settings = storage.get_settings();
        let server_urls = configured_server_urls(&settings);
        if server_urls.is_empty() {
            return; // No server configured, skip sync
        }

        for entry in entries {
            match self
                .push_entry(&entry, storage, &server_urls, &settings.api_key)
                .await
            {
                PushOutcome::Rejected(rejection) if rejection.is_credential_problem() => {
                    log::warn!("Server rejected the sync credentials; pausing this push batch");
                    return;
                }
                PushOutcome::Unreachable => {
                    // Every remaining entry would wait out the same connect
                    // timeout on every endpoint before failing too.
                    log::debug!("No sync server reachable; pausing this push batch");
                    return;
                }
                PushOutcome::Synced | PushOutcome::Rejected(_) | PushOutcome::Failed => {}
            }
        }
    }

    pub async fn sync_entry(&self, entry: &ClipboardEntry, storage: &LocalStorage) {
        let settings = storage.get_settings();
        let server_urls = configured_server_urls(&settings);
        if server_urls.is_empty() {
            return; // No server configured, skip sync
        }

        self.push_entry(entry, storage, &server_urls, &settings.api_key)
            .await;
    }

    async fn push_entry(
        &self,
        entry: &ClipboardEntry,
        storage: &LocalStorage,
        server_urls: &[ServerEndpoint],
        api_key: &str,
    ) -> PushOutcome {
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

        let outcome = self
            .push_entry_with_fallback(server_urls, api_key, &req, &entry.id)
            .await;

        if outcome == PushOutcome::Synced {
            if let Err(e) = storage.mark_synced(&entry.id) {
                log::error!("Failed to mark entry as synced: {}", e);
            }
        }

        outcome
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
            let fetch_result = match self
                .fetch_entries_page_with_fallback(
                    &server_urls,
                    &api_key,
                    PAGE_SIZE,
                    before_cursor
                        .as_ref()
                        .map(|(updated_at, id)| (updated_at.as_str(), id.as_str())),
                )
                .await
            {
                PageFetch::Page(result) => result,
                PageFetch::Rejected {
                    endpoint,
                    rejection,
                } => {
                    return Ok(PullSyncResult {
                        pulled,
                        endpoint_status: SyncEndpointStatus::rejected(&endpoint, rejection),
                    });
                }
                PageFetch::Unreachable => {
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
                }
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
    ) -> PushOutcome {
        let mut rejection: Option<Rejection> = None;
        let mut connected = false;

        for (index, endpoint) in server_urls.iter().enumerate() {
            let url = format!("{}/api/entries", endpoint.url);
            let mut request = self.http.post(&url).json(req);

            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }

            match request.send().await {
                Ok(response) => {
                    connected = true;
                    if response.status().is_success() {
                        self.note_responding_endpoint(endpoint);
                        return PushOutcome::Synced;
                    }

                    rejection = Some(Rejection::from_status(response.status()).outrank(rejection));

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
                    // A connect failure or a request that could not even be built
                    // never reached a server.
                    connected |= !(e.is_connect() || e.is_builder());
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

        match rejection {
            Some(rejection) => PushOutcome::Rejected(rejection),
            None if connected => PushOutcome::Failed,
            None => PushOutcome::Unreachable,
        }
    }

    async fn fetch_entries_page_with_fallback(
        &self,
        server_urls: &[ServerEndpoint],
        api_key: &str,
        page_size: u32,
        before_cursor: Option<(&str, &str)>,
    ) -> PageFetch {
        let mut first_rejection: Option<(ServerEndpoint, Rejection)> = None;

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

            if !response.status().is_success() {
                let rejection = Rejection::from_status(response.status());
                if first_rejection
                    .as_ref()
                    .is_none_or(|(_, earlier)| rejection.outranks(*earlier))
                {
                    first_rejection = Some((endpoint.clone(), rejection));
                }
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
                    // Only a usable answer counts as the server responding;
                    // a 401 or an HTML error page must not read as "online".
                    self.note_responding_endpoint(endpoint);
                    return PageFetch::Page(FetchEntriesResult {
                        page,
                        endpoint_index: index,
                    });
                }
                Err(e) => {
                    log::warn!(
                        "Failed to parse entries response from {}: {}",
                        endpoint.url,
                        e
                    );
                    first_rejection
                        .get_or_insert_with(|| (endpoint.clone(), Rejection::UnreadableResponse));
                }
            }
        }

        match first_rejection {
            Some((endpoint, rejection)) => PageFetch::Rejected {
                endpoint,
                rejection,
            },
            None => PageFetch::Unreachable,
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

        // One transaction for the row, its starred flag, and its synced flag.
        // Doing these as three separate statements costs three fsyncs per
        // entry, which is the dominant cost of a bulk pull on mobile.
        //
        // The server's id and timestamps are carried through rather than
        // regenerated, so an entry is the same entry on every device and the
        // local list stays in true chronological order.
        storage.insert_remote_entry(
            crate::storage::RemoteEntryIdentity {
                id: &remote.entry.id,
                created_at: remote.entry.created_at,
                updated_at: remote.entry.updated_at,
            },
            remote.entry.content_type,
            &remote_flavors,
            blob_data.as_deref(),
            &content_hash,
            remote.entry.source_app.as_deref(),
            remote.entry.starred,
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const EMPTY_PAGE: &str = r#"{"entries":[],"total":0,"has_more":false}"#;

    /// A minimal HTTP/1.1 server that answers every request with one fixed
    /// response and counts the requests it received.
    async fn fake_server(status: &'static str, body: &'static str) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let counter = counter.clone();
                tokio::spawn(async move {
                    read_request(&mut socket).await;
                    counter.fetch_add(1, Ordering::SeqCst);
                    let response = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        (url, requests)
    }

    /// Consume the request head and body so the client sees a clean response.
    async fn read_request(socket: &mut tokio::net::TcpStream) {
        let mut data = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let read = match socket.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(read) => read,
            };
            data.extend_from_slice(&buffer[..read]);

            let Some(head_end) = data.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&data[..head_end]).to_ascii_lowercase();
            let body_len = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if data.len() >= head_end + 4 + body_len {
                return;
            }
        }
    }

    /// A URL on which nothing is listening.
    async fn closed_port_url() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        url
    }

    fn storage_with_servers(primary: &str, fallback: &str) -> (tempfile::TempDir, LocalStorage) {
        let dir = tempfile::tempdir().unwrap();
        let storage = LocalStorage::new(dir.path()).unwrap();
        storage
            .save_settings(&Settings {
                server_url_primary: primary.to_string(),
                server_url_fallback: fallback.to_string(),
                api_key: "configured password".to_string(),
                ..Settings::default()
            })
            .unwrap();
        (dir, storage)
    }

    fn queue_text(storage: &LocalStorage, text: &str) {
        let flavors = ClipboardFlavors {
            text_plain: Some(text.to_string()),
            ..ClipboardFlavors::default()
        };
        let hash = flavors.payload_hash(ContentType::Text, None);
        storage
            .insert_entry(ContentType::Text, &flavors, None, &hash, None)
            .unwrap()
            .expect("a new entry is queued");
    }

    async fn pull_state(primary: &str, fallback: &str) -> SyncEndpointStatus {
        let (_dir, storage) = storage_with_servers(primary, fallback);
        let client = SyncClient::new(&storage);
        client
            .pull_new_entries(&storage)
            .await
            .unwrap()
            .endpoint_status
    }

    #[tokio::test]
    async fn a_rejected_password_is_reported_instead_of_online() {
        let (url, _) = fake_server("401 Unauthorized", r#"{"error":"Unauthorized"}"#).await;

        let status = pull_state(&url, "").await;

        assert_eq!(status.state, SyncState::Unauthorized);
        assert!(status.message.unwrap().contains("password"));
    }

    #[tokio::test]
    async fn an_unconfigured_server_is_reported_as_needing_setup() {
        let (url, _) = fake_server("403 Forbidden", r#"{"error":"Password not configured"}"#).await;

        let status = pull_state(&url, "").await;

        assert_eq!(status.state, SyncState::Unauthorized);
        assert!(status.message.unwrap().contains("admin page"));
    }

    #[tokio::test]
    async fn a_credential_rejection_outranks_another_endpoints_error() {
        let (failing, _) = fake_server("500 Internal Server Error", "{}").await;
        let (refusing, _) = fake_server("401 Unauthorized", "{}").await;

        for (primary, fallback) in [(&failing, &refusing), (&refusing, &failing)] {
            let status = pull_state(primary, fallback).await;
            assert_eq!(status.state, SyncState::Unauthorized);
            assert_eq!(status.url.as_deref(), Some(refusing.as_str()));
        }
    }

    #[tokio::test]
    async fn a_push_batch_pauses_when_any_endpoint_refuses_the_password() {
        let (failing, failing_requests) = fake_server("500 Internal Server Error", "{}").await;
        let (refusing, refusing_requests) = fake_server("401 Unauthorized", "{}").await;
        let (_dir, storage) = storage_with_servers(&failing, &refusing);
        for text in ["first", "second", "third"] {
            queue_text(&storage, text);
        }

        SyncClient::new(&storage)
            .sync_unsynced_entries(&storage)
            .await;

        assert_eq!(failing_requests.load(Ordering::SeqCst), 1);
        assert_eq!(refusing_requests.load(Ordering::SeqCst), 1);
        // Pausing leaves the whole queue for the next successful sync.
        assert_eq!(storage.get_unsynced_entries().unwrap().len(), 3);
    }

    #[test]
    fn sync_states_serialize_to_the_strings_the_frontend_knows() {
        // Must match KNOWN_STATES in src/lib/util/syncStatusStore.ts, which
        // folds anything else into "unreachable".
        for (state, expected) in [
            (SyncState::Checking, "checking"),
            (SyncState::Disabled, "disabled"),
            (SyncState::Online, "online"),
            (SyncState::Unreachable, "unreachable"),
            (SyncState::Unauthorized, "unauthorized"),
            (SyncState::Error, "error"),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn a_server_error_is_reported_with_its_status() {
        let (url, _) = fake_server("500 Internal Server Error", r#"{"error":"boom"}"#).await;

        let status = pull_state(&url, "").await;

        assert_eq!(status.state, SyncState::Error);
        assert!(status.message.unwrap().contains("500"));
    }

    #[tokio::test]
    async fn a_reply_that_is_not_a_copywraith_response_is_an_error() {
        let (url, _) = fake_server("200 OK", "<html>captive portal</html>").await;

        let status = pull_state(&url, "").await;

        assert_eq!(status.state, SyncState::Error);
    }

    #[tokio::test]
    async fn a_usable_page_is_online() {
        let (url, _) = fake_server("200 OK", EMPTY_PAGE).await;

        let status = pull_state(&url, "").await;

        assert_eq!(status.state, SyncState::Online);
    }

    #[tokio::test]
    async fn a_rejection_outranks_an_unreachable_fallback() {
        let (url, _) = fake_server("401 Unauthorized", "{}").await;
        let closed = closed_port_url().await;

        // The server that answered says why sync fails; the dead endpoint
        // cannot, whichever order they are configured in.
        assert_eq!(
            pull_state(&closed, &url).await.state,
            SyncState::Unauthorized
        );
        assert_eq!(
            pull_state(&url, &closed).await.state,
            SyncState::Unauthorized
        );
    }

    #[tokio::test]
    async fn nothing_listening_is_unreachable() {
        let closed = closed_port_url().await;

        assert_eq!(pull_state(&closed, "").await.state, SyncState::Unreachable);
    }

    #[tokio::test]
    async fn a_push_batch_stops_at_the_first_credential_rejection() {
        let (url, requests) = fake_server("401 Unauthorized", "{}").await;
        let (_dir, storage) = storage_with_servers(&url, "");
        for text in ["first", "second", "third"] {
            queue_text(&storage, text);
        }

        SyncClient::new(&storage)
            .sync_unsynced_entries(&storage)
            .await;

        // Every further request would cost the server an Argon2id run and
        // could not succeed with the same password.
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(storage.get_unsynced_entries().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_push_batch_continues_past_entry_specific_rejections() {
        let (url, requests) = fake_server("413 Payload Too Large", "{}").await;
        let (_dir, storage) = storage_with_servers(&url, "");
        for text in ["first", "second", "third"] {
            queue_text(&storage, text);
        }

        SyncClient::new(&storage)
            .sync_unsynced_entries(&storage)
            .await;

        assert_eq!(requests.load(Ordering::SeqCst), 3);
        assert_eq!(storage.get_unsynced_entries().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_successful_push_marks_the_entry_synced() {
        let (url, requests) = fake_server("201 Created", "{}").await;
        let (_dir, storage) = storage_with_servers(&url, "");
        queue_text(&storage, "accepted");

        SyncClient::new(&storage)
            .sync_unsynced_entries(&storage)
            .await;

        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(storage.get_unsynced_entries().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pushing_with_no_server_listening_is_unreachable() {
        let closed = closed_port_url().await;
        let (_dir, storage) = storage_with_servers(&closed, "");
        queue_text(&storage, "stranded");
        let entry = storage.get_unsynced_entries().unwrap().remove(0);
        let client = SyncClient::new(&storage);
        let endpoints = configured_server_urls(&storage.get_settings());

        let outcome = client.push_entry(&entry, &storage, &endpoints, "pw").await;

        assert_eq!(outcome, PushOutcome::Unreachable);
    }

    #[test]
    fn only_failure_states_back_off() {
        assert!(SyncState::Unreachable.is_failure());
        assert!(SyncState::Unauthorized.is_failure());
        assert!(SyncState::Error.is_failure());
        assert!(!SyncState::Online.is_failure());
        assert!(!SyncState::Disabled.is_failure());
        assert!(!SyncState::Checking.is_failure());
    }
}
