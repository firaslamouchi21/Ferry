CREATE TABLE outbound_items (
    id TEXT PRIMARY KEY,
    peer_id TEXT NOT NULL REFERENCES roster_entries (peer_id),
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    hash TEXT NOT NULL,
    ttl_secs INTEGER NOT NULL,
    is_burn_after_read INTEGER NOT NULL DEFAULT 0,
    created_at_millis INTEGER NOT NULL
);

CREATE INDEX idx_outbound_items_peer_id ON outbound_items (peer_id);
CREATE INDEX idx_outbound_items_state ON outbound_items (state);
