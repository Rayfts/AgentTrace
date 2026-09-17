CREATE TABLE IF NOT EXISTS artifacts (
    artifact_id TEXT PRIMARY KEY NOT NULL,
    run_id TEXT NOT NULL,
    event_id TEXT,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    media_type TEXT,
    content_sha256 TEXT NOT NULL,
    original_size INTEGER NOT NULL CHECK (original_size >= 0),
    artifact_blob BLOB NOT NULL,
    compressed INTEGER NOT NULL DEFAULT 0 CHECK (compressed IN (0, 1)),
    created_at TEXT NOT NULL,
    FOREIGN KEY (run_id) REFERENCES runs(run_id) ON DELETE CASCADE,
    FOREIGN KEY (event_id) REFERENCES events(event_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_artifacts_run_created ON artifacts(run_id, created_at, artifact_id);
CREATE INDEX IF NOT EXISTS idx_artifacts_event ON artifacts(event_id) WHERE event_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_artifacts_sha256 ON artifacts(content_sha256);
