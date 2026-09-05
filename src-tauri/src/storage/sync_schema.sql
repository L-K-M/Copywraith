CREATE TABLE IF NOT EXISTS sync_peers (server_id TEXT PRIMARY KEY, cursor INTEGER NOT NULL DEFAULT 0, reset_requested INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS sync_profiles (profile TEXT PRIMARY KEY, server_id TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sync_links (
    server_id TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    local_id TEXT,
    content_hash TEXT NOT NULL,
    sequence INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    starred INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT,
    PRIMARY KEY(server_id, remote_id)
);
CREATE INDEX IF NOT EXISTS sync_links_local ON sync_links(local_id, server_id);
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
