-- Analysis tracking and provenance: create a persistent proposals table
BEGIN;

CREATE TABLE IF NOT EXISTS proposed_inbox (
	id               TEXT PRIMARY KEY,
	conversation_id  TEXT REFERENCES ai_conversations(id) ON DELETE SET NULL,
	source_entry_id  TEXT REFERENCES journal_entries(id) ON DELETE SET NULL,
	item_type        TEXT NOT NULL CHECK(item_type IN ('task','fact','note','event','edge','other')),
	proposed_title   TEXT NOT NULL,
	proposed_data    TEXT NOT NULL,
	source_text      TEXT,
	status           TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','accepted','edited','dismissed')),
	confidence       REAL,
	created_by       TEXT NOT NULL DEFAULT 'ai' CHECK(created_by IN ('ai','user','system')),
	accepted_task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
	accepted_by      TEXT,
	accepted_at      TEXT,
	last_reinforced  TEXT,
	created_at       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_proposed_inbox_status ON proposed_inbox(status);
CREATE INDEX IF NOT EXISTS idx_proposed_inbox_created ON proposed_inbox(created_at);

COMMIT;
