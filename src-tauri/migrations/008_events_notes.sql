BEGIN;

-- Events table: calendar events, reminders, and recurrence metadata
CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    start_at TEXT NOT NULL,
    end_at TEXT,
    is_all_day INTEGER NOT NULL DEFAULT 0,
    location TEXT,
    recurrence_rule TEXT,
    status TEXT DEFAULT 'confirmed',
    created_by TEXT,
    source_entry_id TEXT REFERENCES journal_entries(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    is_deleted INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_events_start_at ON events(start_at);

-- Notes table: user notes, scratchpads, transcluded snippets
CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    title TEXT,
    content TEXT NOT NULL,
    tags TEXT DEFAULT '[]',
    source_entry_id TEXT REFERENCES journal_entries(id) ON DELETE SET NULL,
    created_by TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    is_deleted INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_notes_created_at ON notes(created_at);

COMMIT;
