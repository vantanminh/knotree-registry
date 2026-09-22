CREATE TABLE IF NOT EXISTS runtime_snapshots (
    snapshot_key TEXT PRIMARY KEY,
    payload JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
