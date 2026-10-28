-- Create persistent analysis queue to survive restarts
CREATE TABLE IF NOT EXISTS analysis_queue (
    entry_id TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'queued', -- queued | processing | done | failed
    queued_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_analysis_queue_status_queued ON analysis_queue (status, queued_at);
