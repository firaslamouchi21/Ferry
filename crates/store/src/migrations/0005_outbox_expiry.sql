ALTER TABLE outbox_entries ADD COLUMN outbox_expires_at_wall_estimate_millis INTEGER;
ALTER TABLE outbox_entries ADD COLUMN outbox_expiry_session_id TEXT;
