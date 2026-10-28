BEGIN;

-- D-138: onboarding persona answers are stored as knowledge_items with
-- created_by = 'onboarding'. The original 012 CHECK only allowed 'user'/'ai'.
-- SQLite cannot drop a CHECK constraint via ALTER, so rebuild both tables,
-- relaxing the created_by CHECK to also accept 'onboarding'. The rebuild
-- follows the lint-verified order (child dropped before parent) and uses
-- defer_foreign_keys inside the runner's transaction so the rename-then-drop
-- re-pointing of the child's FK references never trips immediate enforcement
-- on a pool connection where foreign_keys = ON. defer_foreign_keys is restored
-- to OFF (its default) before COMMIT.

PRAGMA defer_foreign_keys = ON;

-- Rebuild knowledge_items (parent). Renaming first rewrites any existing
-- knowledge_links FK to reference the old table; the child is rebuilt and
-- dropped before the old parent is dropped, so no orphan-cascade can fire.
ALTER TABLE knowledge_items RENAME TO knowledge_items_old;

CREATE TABLE knowledge_items (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    content         TEXT NOT NULL,
    kind            TEXT NOT NULL DEFAULT 'information'
                    CHECK(kind IN ('memory', 'information', 'knowledge')),
    category        TEXT NOT NULL DEFAULT 'other'
                    CHECK(category IN ('preference', 'habit', 'constraint', 'person', 'goal', 'other')),
    tags            TEXT NOT NULL DEFAULT '[]',
    item_date       TEXT,
    confidence      REAL NOT NULL DEFAULT 1.0,
    provenance      TEXT NOT NULL DEFAULT 'manual'
                    CHECK(provenance IN ('manual', 'ai', 'journal')),
    created_by      TEXT NOT NULL DEFAULT 'user'
                    CHECK(created_by IN ('user', 'ai', 'onboarding')),
    source_entry_id TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

INSERT INTO knowledge_items
    (id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at)
SELECT id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at
FROM knowledge_items_old;

CREATE INDEX IF NOT EXISTS idx_knowledge_items_kind     ON knowledge_items(kind);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_category ON knowledge_items(category);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_title    ON knowledge_items(title);

-- Rebuild knowledge_links (child) against the new parent.
ALTER TABLE knowledge_links RENAME TO knowledge_links_old;

CREATE TABLE knowledge_links (
    id         TEXT PRIMARY KEY,
    source_id  TEXT NOT NULL REFERENCES knowledge_items(id) ON DELETE CASCADE,
    target_id  TEXT NOT NULL REFERENCES knowledge_items(id) ON DELETE CASCADE,
    relation   TEXT NOT NULL
               CHECK(relation IN ('related-to', 'part-of', 'contradicts', 'cause-of', 'reminds-of')),
    created_by TEXT NOT NULL DEFAULT 'user'
               CHECK(created_by IN ('user', 'ai', 'onboarding')),
    created_at TEXT NOT NULL,
    UNIQUE(source_id, target_id, relation)
);

INSERT INTO knowledge_links
    (id, source_id, target_id, relation, created_by, created_at)
SELECT id, source_id, target_id, relation, created_by, created_at
FROM knowledge_links_old;

CREATE INDEX IF NOT EXISTS idx_knowledge_links_source ON knowledge_links(source_id);
CREATE INDEX IF NOT EXISTS idx_knowledge_links_target ON knowledge_links(target_id);

-- Drop old tables in dependency order (child first, parent last).
DROP TABLE knowledge_links_old;
DROP TABLE knowledge_items_old;

PRAGMA defer_foreign_keys = OFF;

COMMIT;