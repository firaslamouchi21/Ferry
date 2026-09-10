CREATE TABLE remote_jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    params TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at_millis INTEGER NOT NULL,
    finished_at_millis INTEGER,
    result TEXT,
    error TEXT
);

CREATE INDEX remote_jobs_state ON remote_jobs (state, created_at_millis);
