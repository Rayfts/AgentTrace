CREATE TABLE IF NOT EXISTS runs (
    run_id TEXT PRIMARY KEY NOT NULL,
    trace_id TEXT NOT NULL,
    harness TEXT NOT NULL,
    integration_mode TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'interrupted')),
    started_at TEXT NOT NULL,
    finished_at TEXT,
    last_sequence INTEGER NOT NULL DEFAULT 0,
    event_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS events (
    event_id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    kind TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    harness TEXT NOT NULL,
    integration_mode TEXT NOT NULL,
    provenance TEXT NOT NULL,
    event_blob BLOB NOT NULL,
    compressed INTEGER NOT NULL DEFAULT 0 CHECK (compressed IN (0, 1)),
    raw_present INTEGER NOT NULL DEFAULT 0 CHECK (raw_present IN (0, 1)),
    FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
    UNIQUE(run_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_events_run_sequence ON events(run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_events_run_kind_sequence ON events(run_id, kind, sequence);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_runs_started_at ON runs(started_at DESC);
