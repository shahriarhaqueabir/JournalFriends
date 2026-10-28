BEGIN;

CREATE TABLE IF NOT EXISTS knowledge_items (
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
                    CHECK(created_by IN ('user', 'ai')),
    source_entry_id TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_knowledge_items_kind     ON knowledge_items(kind);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_category ON knowledge_items(category);
CREATE INDEX IF NOT EXISTS idx_knowledge_items_title    ON knowledge_items(title);

CREATE TABLE IF NOT EXISTS knowledge_links (
    id         TEXT PRIMARY KEY,
    source_id  TEXT NOT NULL REFERENCES knowledge_items(id) ON DELETE CASCADE,
    target_id  TEXT NOT NULL REFERENCES knowledge_items(id) ON DELETE CASCADE,
    relation   TEXT NOT NULL
               CHECK(relation IN ('related-to', 'part-of', 'contradicts', 'cause-of', 'reminds-of')),
    created_by TEXT NOT NULL DEFAULT 'user'
               CHECK(created_by IN ('user', 'ai')),
    created_at TEXT NOT NULL,
    UNIQUE(source_id, target_id, relation)
);

CREATE INDEX IF NOT EXISTS idx_knowledge_links_source ON knowledge_links(source_id);
CREATE INDEX IF NOT EXISTS idx_knowledge_links_target ON knowledge_links(target_id);

INSERT INTO knowledge_items
    (id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at)
SELECT id, fact_key, content, 'information', category, '[]', NULL, confidence, 'ai',
       COALESCE(created_by, 'ai'), source_entry_id, created_at, updated_at
FROM user_profile;

DROP TABLE user_profile;

COMMIT;