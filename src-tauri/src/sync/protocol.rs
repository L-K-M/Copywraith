use super::*;
use copywraith_core::sync_protocol::*;
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use std::collections::HashSet;

#[derive(Debug)]
enum RequestFailureKind {
    Operation(SyncRetryPolicy),
    Session,
}

#[derive(Debug)]
struct RequestFailure {
    kind: RequestFailureKind,
    message: String,
}

impl std::fmt::Display for RequestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RequestFailure {}

fn response_failure(status: StatusCode, error: Option<SyncErrorResponse>) -> RequestFailure {
    let code = error.as_ref().map(|error| error.code);
    let message = error
        .map(|error| error.error)
        .unwrap_or_else(|| format!("HTTP {status}"));
    let kind = if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        || matches!(
            code,
            Some(SyncErrorCode::ServerMismatch | SyncErrorCode::InvalidCursor)
        ) {
        RequestFailureKind::Session
    } else if matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE | StatusCode::UNPROCESSABLE_ENTITY
    ) || matches!(
        code,
        Some(SyncErrorCode::OperationReuse | SyncErrorCode::WrongOperationKind)
    ) {
        RequestFailureKind::Operation(SyncRetryPolicy::Manual)
    } else if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
        RequestFailureKind::Operation(SyncRetryPolicy::Automatic)
    } else {
        RequestFailureKind::Session
    };
    RequestFailure { kind, message }
}

struct ProtocolSession {
    server_id: String,
    endpoints: Vec<ServerEndpoint>,
    api_key: String,
}

enum Backend {
    Disabled,
    Legacy,
    Protocol(ProtocolSession),
}

impl SyncClient {
    async fn backend(&self, storage: &LocalStorage) -> anyhow::Result<Backend> {
        let settings = storage.get_settings();
        let configured = configured_server_urls(&settings);
        if configured.is_empty() {
            return Ok(Backend::Disabled);
        }
        let profile = hash_bytes(
            configured
                .iter()
                .map(|e| e.url.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .as_bytes(),
        );
        let mut server = None;
        let mut endpoints = Vec::new();
        let mut legacy = false;
        let mut last_error = "No configured server responded".to_string();
        for endpoint in configured {
            let response = self
                .http
                .get(format!("{}/api/sync", endpoint.url))
                .bearer_auth(&settings.api_key)
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    last_error = error.to_string();
                    continue;
                }
            };
            if response.status() == StatusCode::NOT_FOUND {
                legacy = true;
                continue;
            }
            if !response.status().is_success() {
                last_error = format!("Sync discovery returned {}", response.status());
                continue;
            }
            let info: SyncInfo = response.json().await?;
            anyhow::ensure!(
                info.version == SYNC_PROTOCOL_VERSION,
                "Unsupported server synchronization protocol"
            );
            anyhow::ensure!(
                server.as_ref().is_none_or(|id| id == &info.server_id),
                "Primary and fallback URLs identify different servers; synchronization stopped"
            );
            storage.bind_sync_server(&profile, &info.server_id)?;
            server = Some(info.server_id);
            endpoints.push(endpoint);
        }
        if let Some(server_id) = server {
            return Ok(Backend::Protocol(ProtocolSession {
                server_id,
                endpoints,
                api_key: settings.api_key,
            }));
        }
        if legacy {
            anyhow::ensure!(!storage.has_generation_sync_state()?, "Generation-aware sync state exists; refusing an unsafe legacy downgrade. Upgrade or restore the server.");
            return Ok(Backend::Legacy);
        }
        anyhow::bail!(last_error)
    }

    async fn protocol_request<T: DeserializeOwned>(
        &self,
        session: &ProtocolSession,
        method: Method,
        path: &str,
        body: Option<&SyncMutation>,
    ) -> Result<T, RequestFailure> {
        let mut error = "No verified endpoint responded".to_string();
        for endpoint in &session.endpoints {
            let mut request = self
                .http
                .request(
                    method.clone(),
                    format!("{}/api/sync/{}{path}", endpoint.url, session.server_id),
                )
                .bearer_auth(&session.api_key);
            if let Some(body) = body {
                request = request.json(body);
            }
            match request.send().await {
                Ok(response) if response.status().is_success() => match response.json().await {
                    Ok(value) => return Ok(value),
                    Err(cause) => {
                        error = format!(
                            "Receipt/response unreadable; delivery remains unresolved: {cause}"
                        )
                    }
                },
                Ok(response) => {
                    let status = response.status();
                    let failure =
                        response_failure(status, response.json::<SyncErrorResponse>().await.ok());
                    if !matches!(
                        failure.kind,
                        RequestFailureKind::Operation(SyncRetryPolicy::Automatic)
                    ) {
                        return Err(failure);
                    }
                    error = failure.message;
                }
                Err(cause) => {
                    error = cause.to_string();
                }
            }
        }
        Err(RequestFailure {
            kind: RequestFailureKind::Operation(SyncRetryPolicy::Automatic),
            message: error,
        })
    }

    pub async fn sync_unsynced_entries(&self, storage: &LocalStorage) {
        let _guard = self.protocol_lock.lock().await;
        let result = match self.backend(storage).await {
            Ok(Backend::Disabled) => Ok(()),
            Ok(Backend::Legacy) => {
                self.legacy_sync_unsynced_entries(storage).await;
                Ok(())
            }
            Ok(Backend::Protocol(session)) => {
                let _ = storage.record_sync_session_error(None, None);
                let result = self.push_protocol(storage, &session).await;
                storage
                    .record_sync_session_error(
                        Some(&session.server_id),
                        result
                            .as_ref()
                            .err()
                            .map(|error| error.to_string())
                            .as_deref(),
                    )
                    .and(result)
            }
            Err(error) => {
                let _ = storage.record_sync_session_error(None, Some(&error.to_string()));
                Err(error)
            }
        };
        if let Err(error) = result {
            log::warn!("Push synchronization incomplete: {error}");
        }
    }

    pub async fn sync_entry(&self, _entry: &ClipboardEntry, storage: &LocalStorage) {
        // Read durable current intent, never acknowledge a caller's stale snapshot.
        self.sync_unsynced_entries(storage).await;
    }

    pub async fn pull_new_entries(&self, storage: &LocalStorage) -> anyhow::Result<PullSyncResult> {
        let _guard = self.protocol_lock.lock().await;
        match self.backend(storage).await? {
            Backend::Disabled => Ok(PullSyncResult {
                pulled: 0,
                endpoint_status: SyncEndpointStatus::disabled(),
            }),
            Backend::Legacy => {
                let mut result = self.legacy_pull_new_entries(storage).await?;
                result.endpoint_status.message = Some(
                    "Legacy server: deletion synchronization is unavailable. Upgrade the server."
                        .into(),
                );
                Ok(result)
            }
            Backend::Protocol(session) => {
                let result = self.pull_protocol(storage, &session).await;
                let pulled = std::mem::take(&mut *self.pending_protocol_changes.lock().unwrap());
                let mut endpoint_status = match result {
                    Ok(_) => SyncEndpointStatus::online(&session.endpoints[0]),
                    Err(error) => SyncEndpointStatus::unreachable_endpoint(
                        &session.endpoints[0],
                        format!("Synchronization incomplete: {error}"),
                    ),
                };
                if let Some(warning) = storage.sync_warning(&session.server_id)? {
                    endpoint_status.message = Some(match endpoint_status.message {
                        Some(message) => format!("{message} {warning}"),
                        None => warning,
                    });
                }
                Ok(PullSyncResult {
                    pulled,
                    endpoint_status,
                })
            }
        }
    }

    async fn send_pending(
        &self,
        storage: &LocalStorage,
        session: &ProtocolSession,
        attempted: &mut HashSet<String>,
    ) -> anyhow::Result<()> {
        for pending in storage.scheduled_mutations(&session.server_id)? {
            if !attempted.insert(pending.request.operation_id.clone()) {
                continue;
            }
            let result = self
                .protocol_request(session, Method::POST, "/operations", Some(&pending.request))
                .await;
            let receipt: SyncReceipt = match result {
                Ok(receipt) => receipt,
                Err(RequestFailure {
                    kind: RequestFailureKind::Operation(policy),
                    message,
                }) => {
                    let recovery = match policy {
                        SyncRetryPolicy::Manual => {
                            "Correct the cause, then explicitly retry or delete the local item."
                        }
                        SyncRetryPolicy::Automatic => {
                            "The same operation will retry automatically."
                        }
                    };
                    storage.record_mutation_failure(
                        &pending.request.operation_id,
                        policy,
                        &format!("{message}. {recovery} Prior delivery may have committed."),
                    )?;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let changed = storage.acknowledge_mutation(&pending, &receipt)?;
            *self.pending_protocol_changes.lock().unwrap() += usize::from(changed);
        }
        Ok(())
    }

    async fn push_protocol(
        &self,
        storage: &LocalStorage,
        session: &ProtocolSession,
    ) -> anyhow::Result<()> {
        // Resolve earlier deliveries and deletions before freezing a replacement create.
        let mut attempted = HashSet::new();
        self.send_pending(storage, session, &mut attempted).await?;
        self.pull_protocol(storage, session).await?;
        for candidate in storage.sync_candidates(&session.server_id)? {
            let action = if let Some(id) = &candidate.remote_id {
                SyncAction::Star {
                    generation_id: id.clone(),
                    starred: candidate.entry.starred,
                }
            } else {
                let entry = &candidate.entry;
                let flavors = entry.resolved_flavors();
                let content_hash =
                    flavors.payload_hash(entry.content_type, entry.blob_hash.as_deref());
                let head: SyncHead = self
                    .protocol_request(
                        session,
                        Method::GET,
                        &format!("/heads/{content_hash}"),
                        None,
                    )
                    .await?;
                anyhow::ensure!(
                    head.server_id == session.server_id && head.content_hash == content_hash,
                    "Head identity mismatch"
                );
                if let Some(generation) = &head.generation {
                    if generation.state == GenerationState::Deleted
                        && !storage.capture_can_restore(
                            &session.server_id,
                            &candidate,
                            &generation.id,
                        )?
                    {
                        storage.block_sync_candidate(&session.server_id, &candidate, "This capture predates knowledge of the deletion; copy again after synchronization to restore it.")?;
                        continue;
                    }
                }
                let blob_base64 = match entry.blob_hash.as_deref() {
                    Some(hash) => match storage.get_blob(hash) {
                        Ok(Some(bytes)) if hash_bytes(&bytes) == hash => {
                            Some(bytes_to_base64(&bytes))
                        }
                        _ => {
                            storage.record_candidate_failure(&session.server_id, &candidate, "Capture blob is missing or corrupt. Repair it, then retry synchronization.")?;
                            continue;
                        }
                    },
                    None => None,
                };
                SyncAction::Create {
                    expected: head.generation,
                    payload: CreateEntryRequest {
                        content_type: entry.content_type,
                        text_content: flavors.to_legacy_text_content(entry.content_type),
                        flavors: Some(flavors),
                        blob_base64,
                        source_app: entry.source_app.clone(),
                        starred: Some(entry.starred),
                        content_hash,
                    },
                }
            };
            storage.enqueue_sync_candidate(&session.server_id, &candidate, action)?;
        }
        self.send_pending(storage, session, &mut attempted).await
    }

    async fn download_protocol_blob(
        &self,
        storage: &LocalStorage,
        session: &ProtocolSession,
        change: &SyncChange,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let entry = &change
            .entry
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing live payload"))?
            .entry;
        let Some(hash) = &entry.blob_hash else {
            return Ok(None);
        };
        if let Some(bytes) = storage.get_blob(hash)? {
            anyhow::ensure!(hash_bytes(&bytes) == *hash, "Local blob hash mismatch");
            return Ok(Some(bytes));
        }
        let mut error = "No verified endpoint supplied the blob".to_string();
        for endpoint in &session.endpoints {
            let response = self
                .http
                .get(format!(
                    "{}/api/sync/{}/entries/{}/blob",
                    endpoint.url, session.server_id, entry.id
                ))
                .bearer_auth(&session.api_key)
                .send()
                .await;
            match response {
                Ok(response) if response.status().is_success() => {
                    let bytes = response.bytes().await?.to_vec();
                    anyhow::ensure!(
                        !bytes.is_empty() && hash_bytes(&bytes) == *hash,
                        "Downloaded blob hash mismatch"
                    );
                    return Ok(Some(bytes));
                }
                Ok(response) => error = format!("Blob download returned {}", response.status()),
                Err(cause) => error = cause.to_string(),
            }
        }
        // Retry from the same cursor: a concurrent deletion appears as a tombstone next pass.
        anyhow::bail!(error)
    }

    async fn pull_protocol(
        &self,
        storage: &LocalStorage,
        session: &ProtocolSession,
    ) -> anyhow::Result<usize> {
        let mut cursor = storage.sync_cursor(&session.server_id)?;
        let mut applied = 0;
        loop {
            let page: SyncChanges = self
                .protocol_request(
                    session,
                    Method::GET,
                    &format!("/changes?cursor={cursor}&limit={SYNC_PAGE_SIZE}"),
                    None,
                )
                .await?;
            anyhow::ensure!(
                page.server_id == session.server_id,
                "Change feed server identity mismatch"
            );
            let previous = cursor;
            for change in &page.changes {
                anyhow::ensure!(
                    change.sequence > cursor && change.sequence <= page.cursor,
                    "Invalid change feed ordering"
                );
                let changed = match change.generation.state {
                    GenerationState::Deleted => {
                        anyhow::ensure!(
                            change.entry.is_none(),
                            "Tombstone must not carry a payload"
                        );
                        storage.apply_sync_deletion(&session.server_id, change)?
                    }
                    GenerationState::Live => {
                        let entry = &change
                            .entry
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("Missing live payload"))?
                            .entry;
                        anyhow::ensure!(
                            entry
                                .resolved_flavors()
                                .payload_hash(entry.content_type, entry.blob_hash.as_deref())
                                == change.content_hash,
                            "Change payload hash mismatch"
                        );
                        let blob = self
                            .download_protocol_blob(storage, session, change)
                            .await?;
                        storage.apply_sync_live(&session.server_id, change, blob.as_deref())?
                    }
                };
                applied += usize::from(changed);
                *self.pending_protocol_changes.lock().unwrap() += usize::from(changed);
                cursor = change.sequence;
            }
            if !page.has_more {
                return Ok(applied);
            }
            anyhow::ensure!(cursor > previous, "Change feed made no progress");
        }
    }
}
