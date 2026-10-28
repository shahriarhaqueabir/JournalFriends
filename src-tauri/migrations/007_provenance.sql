BEGIN;

-- Add provenance columns to existing tables
ALTER TABLE proposed_task_log ADD COLUMN confidence REAL;
ALTER TABLE proposed_task_log ADD COLUMN created_by TEXT;
ALTER TABLE proposed_task_log ADD COLUMN accepted_by TEXT;
ALTER TABLE proposed_task_log ADD COLUMN accepted_at TEXT;
ALTER TABLE proposed_task_log ADD COLUMN last_reinforced TEXT;

ALTER TABLE ai_conversations ADD COLUMN created_by TEXT;

ALTER TABLE ai_messages ADD COLUMN source_entry_id TEXT REFERENCES journal_entries(id) ON DELETE SET NULL;
ALTER TABLE ai_messages ADD COLUMN created_by TEXT;
ALTER TABLE ai_messages ADD COLUMN last_reinforced TEXT;

ALTER TABLE user_profile ADD COLUMN created_by TEXT;
ALTER TABLE user_profile ADD COLUMN accepted_by TEXT;
ALTER TABLE user_profile ADD COLUMN accepted_at TEXT;
ALTER TABLE user_profile ADD COLUMN last_reinforced TEXT;

-- Persistent proposed inbox for review (rich provenance)
CREATE TABLE IF NOT EXISTS proposed_inbox (
  id TEXT PRIMARY KEY,
  conversation_id TEXT REFERENCES ai_conversations(id) ON DELETE SET NULL,
  source_entry_id TEXT REFERENCES journal_entries(id) ON DELETE SET NULL,
  item_type TEXT NOT NULL,
  proposed_title TEXT NOT NULL,
  proposed_data TEXT NOT NULL,
  source_text TEXT,
  status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','accepted','dismissed','edited')),
  confidence REAL,
  created_by TEXT,
  accepted_task_id TEXT REFERENCES tasks(id),
  accepted_by TEXT,
  accepted_at TEXT,
  last_reinforced TEXT,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_proposed_inbox_status ON proposed_inbox(status);

COMMIT;
