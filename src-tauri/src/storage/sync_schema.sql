CREATE TABLE IF NOT EXISTS sync_peers (server_id TEXT PRIMARY KEY, cursor INTEGER NOT NULL DEFAULT 0, reset_requested INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS sync_profiles (profile TEXT PRIMARY KEY, server_id TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sync_links (
    server_id TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    local_id TEXT,
    local_incarnation INTEGER NOT NULL DEFAULT 1,
    content_hash TEXT NOT NULL,
    sequence INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    starred INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT,
    PRIMARY KEY(server_id, remote_id)
);
CREATE INDEX IF NOT EXISTS sync_links_local ON sync_links(local_id, server_id);
CREATE INDEX IF NOT EXISTS sync_links_hash ON sync_links(content_hash, server_id, sequence DESC);
CREATE TABLE IF NOT EXISTS sync_outbox (
    operation_id TEXT PRIMARY KEY,
    server_id TEXT NOT NULL,
    local_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    kind TEXT NOT NULL,
    request TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS sync_outbox_local ON sync_outbox(local_id, server_id);
CREATE TABLE IF NOT EXISTS sync_blocked (server_id TEXT NOT NULL, local_id TEXT NOT NULL, reason TEXT NOT NULL, PRIMARY KEY(server_id, local_id));
CREATE TABLE IF NOT EXISTS sync_unresolved_deletes (local_id TEXT PRIMARY KEY, content_hash TEXT NOT NULL);

-- Operation identity survives payload removal and cancellation acknowledgment.
CREATE TABLE IF NOT EXISTS sync_operation_provenance (
    operation_id TEXT PRIMARY KEY,
    server_id TEXT NOT NULL,
    local_id TEXT NOT NULL,
    incarnation INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    kind TEXT NOT NULL,
    remote_id TEXT,
    resolved INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS sync_provenance_hash ON sync_operation_provenance(content_hash, server_id, kind, resolved);

-- These observations belong to the capture, never to its later upload attempt.
CREATE TABLE IF NOT EXISTS sync_capture_heads (
    local_id TEXT NOT NULL,
    incarnation INTEGER NOT NULL,
    server_id TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    deleted INTEGER NOT NULL,
    PRIMARY KEY(local_id, incarnation, server_id)
);
CREATE TABLE IF NOT EXISTS sync_capture_predecessors (
    local_id TEXT NOT NULL,
    incarnation INTEGER NOT NULL,
    operation_id TEXT NOT NULL,
    PRIMARY KEY(local_id, incarnation, operation_id)
);
