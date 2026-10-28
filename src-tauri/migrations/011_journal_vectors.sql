-- Sprint 7: T-02 Semantic Search
BEGIN;

CREATE TABLE IF NOT EXISTS journal_vectors (
  entry_id TEXT PRIMARY KEY,
  vector   BLOB NOT NULL,
  model    TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(entry_id) REFERENCES journal_entries(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_vectors_model ON journal_vectors(model);

COMMIT;
