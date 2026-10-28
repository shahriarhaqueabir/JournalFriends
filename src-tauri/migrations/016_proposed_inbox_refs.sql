BEGIN;

-- accepted_task_id originally carried tasks(id) (003), but proposed_action also
-- stores event and note proposal ids there, so accepting an event or note failed
-- with FOREIGN KEY constraint failed and rolled back the whole acceptance.
-- SQLite cannot drop an FK via ALTER, so rebuild the table exactly as 003
-- defined it minus that one FK. conversation_id/source_entry_id keep their
-- ON DELETE SET NULL behaviour. No other table references proposed_inbox
-- (grep: zero matches), so the rename cannot re-point anything; the copy still
-- nulls unresolvable conversation/source refs (possible from the era before
-- pool connections enforced foreign_keys) so deferred enforcement cannot fail
-- at COMMIT. defer_foreign_keys mirrors the 013 rebuild pattern so the
-- rename-then-drop never trips immediate enforcement on a pool connection
-- where foreign_keys = ON, and is restored to OFF (its default) before COMMIT.

PRAGMA defer_foreign_keys = ON;

ALTER TABLE proposed_inbox RENAME TO proposed_inbox_old;

-- Indexes follow the renamed table and keep their original names, so the
-- IF NOT EXISTS creates below would silently skip and DROP TABLE _old below
-- would take them with it. Drop them explicitly first.
DROP INDEX IF EXISTS idx_proposed_inbox_status;
DROP INDEX IF EXISTS idx_proposed_inbox_created;

CREATE TABLE proposed_inbox (
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
    accepted_task_id TEXT,
    accepted_by      TEXT,
    accepted_at      TEXT,
    last_reinforced  TEXT,
    created_at       TEXT NOT NULL
);

INSERT INTO proposed_inbox
    (id, conversation_id, source_entry_id, item_type, proposed_title, proposed_data,
     source_text, status, confidence, created_by, accepted_task_id, accepted_by,
     accepted_at, last_reinforced, created_at)
SELECT
    id,
    CASE
        WHEN conversation_id IS NULL THEN NULL
        WHEN EXISTS (SELECT 1 FROM ai_conversations c WHERE c.id = p.conversation_id) THEN conversation_id
        ELSE NULL
    END,
    CASE
        WHEN source_entry_id IS NULL THEN NULL
        WHEN EXISTS (SELECT 1 FROM journal_entries j WHERE j.id = p.source_entry_id) THEN source_entry_id
        ELSE NULL
    END,
    item_type, proposed_title, proposed_data, source_text,
    status, confidence, COALESCE(created_by, 'ai'), accepted_task_id, accepted_by,
    accepted_at, last_reinforced, created_at
FROM proposed_inbox_old p;

CREATE INDEX IF NOT EXISTS idx_proposed_inbox_status ON proposed_inbox(status);
CREATE INDEX IF NOT EXISTS idx_proposed_inbox_created ON proposed_inbox(created_at);

DROP TABLE proposed_inbox_old;

PRAGMA defer_foreign_keys = OFF;

COMMIT;
