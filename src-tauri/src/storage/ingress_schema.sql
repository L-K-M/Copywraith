-- Local knowledge order is independent of server sequences and wall clocks.
CREATE TABLE IF NOT EXISTS ingress_clock (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    value INTEGER NOT NULL CHECK(typeof(value) = 'integer' AND value >= 0),
    backfilled INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO ingress_clock(singleton, value) VALUES (1, 0);
CREATE TABLE IF NOT EXISTS ingress_tombstones (
    server_id TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    first_known INTEGER NOT NULL,
    PRIMARY KEY(server_id, remote_id)
);
CREATE INDEX IF NOT EXISTS ingress_tombstones_hash ON ingress_tombstones(content_hash, first_known);
CREATE TABLE IF NOT EXISTS ingress_deletions (
    operation_id TEXT PRIMARY KEY,
    server_id TEXT NOT NULL,
    local_id TEXT NOT NULL,
    incarnation INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    remote_id TEXT,
    created_clock INTEGER NOT NULL,
    resolved_clock INTEGER
);
CREATE INDEX IF NOT EXISTS ingress_deletions_hash ON ingress_deletions(content_hash, created_clock);
CREATE TABLE IF NOT EXISTS ingress_retirements(content_hash TEXT PRIMARY KEY, clock INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS ingress_registrations (
    registration_id TEXT PRIMARY KEY,
    domain TEXT NOT NULL,
    epoch INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ingress_revocations(registration_id TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS ingress_baselines(registration_id TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS ingress_receipts (
    registration_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    envelope TEXT,
    outcome TEXT NOT NULL,
    reason TEXT,
    content_hash TEXT NOT NULL,
    CHECK((reason IS NULL AND envelope IS NULL) OR (reason IS NOT NULL AND envelope IS NOT NULL)),
    PRIMARY KEY(registration_id, sequence)
);
CREATE TABLE IF NOT EXISTS ingress_observations (
    domain TEXT NOT NULL,
    stamp TEXT NOT NULL,
    payload_fingerprint TEXT NOT NULL,
    registration_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    PRIMARY KEY(domain, stamp)
);
CREATE TABLE IF NOT EXISTS ingress_capture_authority (
    local_id TEXT NOT NULL,
    incarnation INTEGER NOT NULL,
    registration_id TEXT NOT NULL,
    PRIMARY KEY(local_id, incarnation)
);

-- Record first knowledge once, including tombstones first encountered as inserts.
CREATE TRIGGER IF NOT EXISTS ingress_deleted_link_insert AFTER INSERT ON sync_links
WHEN new.deleted = 1 AND NOT EXISTS(SELECT 1 FROM ingress_tombstones WHERE server_id = new.server_id AND remote_id = new.remote_id)
BEGIN
    UPDATE ingress_clock SET value = value + 1;
    INSERT INTO ingress_tombstones SELECT new.server_id, new.remote_id, new.content_hash, value FROM ingress_clock;
END;
CREATE TRIGGER IF NOT EXISTS ingress_deleted_link_update AFTER UPDATE OF deleted ON sync_links
WHEN new.deleted = 1 AND NOT EXISTS(SELECT 1 FROM ingress_tombstones WHERE server_id = new.server_id AND remote_id = new.remote_id)
BEGIN
    UPDATE ingress_clock SET value = value + 1;
    INSERT INTO ingress_tombstones SELECT new.server_id, new.remote_id, new.content_hash, value FROM ingress_clock;
END;
CREATE TRIGGER IF NOT EXISTS ingress_delete_created AFTER INSERT ON sync_operation_provenance
WHEN new.kind = 'delete'
BEGIN
    UPDATE ingress_clock SET value = value + 1;
    INSERT INTO ingress_deletions SELECT new.operation_id, new.server_id, new.local_id, new.incarnation, new.content_hash, new.remote_id, value,
        CASE WHEN new.resolved = 1 THEN value END FROM ingress_clock;
END;
CREATE TRIGGER IF NOT EXISTS ingress_delete_resolved AFTER UPDATE OF resolved ON sync_operation_provenance
WHEN new.kind = 'delete' AND new.resolved = 1 AND EXISTS(SELECT 1 FROM ingress_deletions WHERE operation_id = new.operation_id AND resolved_clock IS NULL)
BEGIN
    UPDATE ingress_clock SET value = value + 1;
    UPDATE ingress_deletions SET resolved_clock = (SELECT value FROM ingress_clock), remote_id = new.remote_id WHERE operation_id = new.operation_id;
END;
-- Retire never-prepared captures too; replacement of an incarnation retires its intent.
CREATE TRIGGER IF NOT EXISTS ingress_entry_deleted AFTER DELETE ON entries
BEGIN
    UPDATE ingress_clock SET value = value + 1;
    INSERT INTO ingress_retirements SELECT old.content_hash, value FROM ingress_clock WHERE 1
        ON CONFLICT(content_hash) DO UPDATE SET clock = excluded.clock;
END;
CREATE TRIGGER IF NOT EXISTS ingress_incarnation_retired AFTER UPDATE OF sync_incarnation ON entries
WHEN new.sync_incarnation != old.sync_incarnation
BEGIN
    UPDATE ingress_clock SET value = value + 1;
    INSERT INTO ingress_retirements SELECT old.content_hash, value FROM ingress_clock WHERE 1
        ON CONFLICT(content_hash) DO UPDATE SET clock = excluded.clock;
END;
