CREATE TABLE sync_clock (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), sequence INTEGER NOT NULL);
INSERT INTO sync_clock VALUES (1, 0);
CREATE TABLE sync_generations (
    id TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    deleted INTEGER NOT NULL DEFAULT 0,
    sequence INTEGER NOT NULL UNIQUE
);
CREATE TABLE sync_heads (content_hash TEXT PRIMARY KEY, generation_id TEXT NOT NULL);
CREATE TABLE sync_receipts (
    operation_id TEXT PRIMARY KEY,
    fingerprint TEXT,
    kind TEXT NOT NULL,
    receipt TEXT NOT NULL
);

-- Adopt existing server IDs without rewriting payloads or client IDs.
INSERT INTO sync_generations (id, content_hash, sequence)
    SELECT id, content_hash, ROW_NUMBER() OVER (ORDER BY id) FROM entries;
INSERT INTO sync_heads SELECT content_hash, id FROM entries;
UPDATE sync_clock SET sequence = (SELECT COALESCE(MAX(sequence), 0) FROM sync_generations);

-- Every writer, including the legacy admin API, participates in the same feed.
CREATE TRIGGER entries_sync_insert AFTER INSERT ON entries BEGIN
    UPDATE sync_clock SET sequence = sequence + 1;
    INSERT INTO sync_generations (id, content_hash, sequence)
        SELECT new.id, new.content_hash, sequence FROM sync_clock;
    INSERT INTO sync_heads VALUES (new.content_hash, new.id)
        ON CONFLICT(content_hash) DO UPDATE SET generation_id = excluded.generation_id;
END;
CREATE TRIGGER entries_sync_update AFTER UPDATE ON entries BEGIN
    UPDATE sync_clock SET sequence = sequence + 1;
    UPDATE sync_generations SET sequence = (SELECT sequence FROM sync_clock) WHERE id = new.id;
END;
CREATE TRIGGER entries_sync_delete AFTER DELETE ON entries BEGIN
    UPDATE sync_clock SET sequence = sequence + 1;
    UPDATE sync_generations SET deleted = 1, sequence = (SELECT sequence FROM sync_clock) WHERE id = old.id;
END;
