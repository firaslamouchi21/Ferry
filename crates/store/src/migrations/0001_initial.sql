CREATE TABLE roster_entries (
    peer_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    signing_key TEXT NOT NULL,
    sealing_key TEXT NOT NULL,
    paired_at_millis INTEGER NOT NULL
);

CREATE TABLE outbox_entries (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL,
    peer_id TEXT NOT NULL REFERENCES roster_entries (peer_id),
    enqueued_at_millis INTEGER NOT NULL,
    outbox_expires_at_monotonic INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_attempted_at_millis INTEGER
);

CREATE INDEX idx_outbox_entries_peer_id ON outbox_entries (peer_id);
CREATE INDEX idx_outbox_entries_item_id ON outbox_entries (item_id);

CREATE TABLE inbox_items (
    id TEXT PRIMARY KEY,
    peer_id TEXT NOT NULL REFERENCES roster_entries (peer_id),
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    hash TEXT NOT NULL,
    payload_ref TEXT,
    received_at_millis INTEGER NOT NULL,
    delivered_at_millis INTEGER,
    expires_at_monotonic INTEGER,
    expires_at_wall_estimate_millis INTEGER,
    is_burn_after_read INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_inbox_items_peer_id ON inbox_items (peer_id);
CREATE INDEX idx_inbox_items_state ON inbox_items (state);

CREATE TABLE audit_events (
    id TEXT PRIMARY KEY,
    actor TEXT NOT NULL,
    kind TEXT NOT NULL,
    item_id TEXT,
    occurred_at_millis INTEGER NOT NULL,
    outcome TEXT NOT NULL
);

CREATE INDEX idx_audit_events_occurred_at_millis ON audit_events (occurred_at_millis);

CREATE TRIGGER audit_events_no_update
BEFORE UPDATE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit_events is append-only: update is not permitted');
END;

CREATE TRIGGER audit_events_no_delete
BEFORE DELETE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit_events is append-only: delete is not permitted');
END;
