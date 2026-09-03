ALTER TABLE inbox_items ADD COLUMN notify_on_open INTEGER NOT NULL DEFAULT 0;

CREATE TABLE open_receipts (
    item_id TEXT PRIMARY KEY REFERENCES inbox_items (id),
    peer_id TEXT NOT NULL REFERENCES roster_entries (peer_id),
    queued_at_millis INTEGER NOT NULL
);

CREATE INDEX idx_open_receipts_peer_id ON open_receipts (peer_id);
