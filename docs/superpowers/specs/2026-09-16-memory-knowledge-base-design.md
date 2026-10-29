# Memory / Knowledge Base v1 — Design

**Date:** 2026-09-16
**Status:** Approved (approach A: unified `knowledge_items` + `knowledge_links`)
**Related:** replays/upgrades the dead `MemoryManager` tab (`src/js/tabs/memory.js`, `src/views/memory.html`) whose backend commands (`graph_query`, `profile_get_facts`, `profile_delete_fact`) no longer exist.

## Goal

The **Memory** tab becomes an evolving knowledge base you and the AI share:

- **Holds** memories (dated), information, and knowledge about the user, plus typed connections between items.
- **Written by** the user manually (in-tab form) and by the AI (chat tools that raise Review-Inbox proposals the user confirms in the Inbox tab).
- **Read by** the AI from chat context and via read-only tools.
- **Managed by** both: add / update / edit / delete items and links.

v1 is deliberately card-grid + detail-panel; the old canvas "Neural Map" is dropped.

## 1. Data model — migration `012_knowledge.sql`

Follow the house migration rules (D-97): the file MUST be wrapped in an outer `BEGIN;` … `COMMIT;` (the runner strips exactly one outer pair via `strip_outer_transaction` and then errors on any *interior* `BEGIN`/`COMMIT` via `ensure_no_explicit_transactions` — `db/migrations.rs:69-116`). No interior transaction keywords anywhere; SQLite keywords may be uppercase or lowercase as long as they parse, but the migration **must** keep `CHECK` constraints and column names byte-identical to what `db/knowledge.rs` and the frontend use.

**`knowledge_items`**

```sql
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
```

**`knowledge_links`** (typed, directed: source --relation--> target)

```sql
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
```

**Data migration** — exact column map. `user_profile` gained 4 provenance columns in `007_provenance.sql`, so it is **12 columns**: `id, fact_key, content, category, confidence, source_entry_id, created_at, updated_at, created_by, accepted_by, accepted_at, last_reinforced`:

```sql
INSERT INTO knowledge_items
    (id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at)
SELECT id, fact_key, content, 'information', category, '[]', NULL, confidence, 'ai',
       COALESCE(created_by, 'ai'), source_entry_id, created_at, updated_at
FROM user_profile;

DROP TABLE user_profile;
```

Notes:
- The table is `knowledge_items`/`knowledge_links` everywhere (no `memories`/`items`/`links`/`knowledge` shorthand); column names byte-identical in DDL, `db/knowledge.rs`, and the frontend.
- `fact_key` becomes `title` (the key is a name/label). Full source text lives in `content`; the AI-filled `content` (sentence) may equal or differ from `title`.
- The `UNIQUE(source_id, target_id, relation)` means exactly one link per ordered pair + relation; re-linking idempotent.

## 2. Backend — `db/knowledge.rs` + commands

New module `src-tauri/src/db/knowledge.rs` (declared in `db/mod.rs`). All functions take `&Connection` (callers route through `db::pool::with_conn` / `with_tx`). Types: `KnowledgeItem` and `KnowledgeLink` (mirror the columns above with `serde::Serialize/Deserialize`, field names snake_case — byte-identical to columns and to what the frontend expects).

Public API:

| fn | SQL shape |
|----|-----------|
| `list_items(conn, kind: Option<&str>, category: Option<&str>, search: Option<&str>)` | `SELECT * FROM knowledge_items` + `WHERE` parts (`kind = ?`, `category = ?`, `(title LIKE ? OR content LIKE ?)` with `%term%`); `ORDER BY updated_at DESC` |
| `get_item(conn, id)` | row + all `knowledge_links` where `source_id = ?` (each joined to target title) |
| `create_item(conn, item: &KnowledgeItem)` | `INSERT INTO knowledge_items (…13 columns…) VALUES (?1..?13)` |
| `update_item(conn, item)` | `UPDATE knowledge_items SET title=?, content=?, kind=?, category=?, tags=?, item_date=?, confidence=?, updated_at=? WHERE id=?` |
| `delete_item(conn, id)` | `DELETE FROM knowledge_items WHERE id = ?` (links cascade) |
| `link_items(conn, source_id, target_id, relation, created_by)` | `INSERT INTO knowledge_links (id, source_id, target_id, relation, created_by, created_at) VALUES (?,?,?,?,?,?)` |
| `unlink_item(conn, link_id)` | `DELETE FROM knowledge_links WHERE id = ?` |

Rules:
- No `SELECT *` column-index assumptions — each query maps by explicit column order declared in the file next to `KnowledgeItem` (the `user_profile::get_facts` row-index style was fragile — `row.get(8).ok()` — and is not repeated).
- `DELETE` targets use parametrized `?` (never string-interpolated); `tags` stored as JSON string, validated round-trip with `serde_json::from_str::<Vec<String>>`.
- `confidence` `REAL`, clamped `0.0..=1.0` on write.
- `item_date` is a plain `TEXT` ISO `YYYY-MM-DD` (matches `calculate_next_due` `%Y-%m-%d` convention; no `toISOString` slicing on the frontend for local dates either).

### Tauri commands (`lib.rs`)

`memory_list(filter: Option<…>)`, `memory_get(id)`, `memory_create(item)`, `memory_update(item)`, `memory_delete(id)`, `memory_link(_link)`, `memory_unlink(link_id)` — each `async fn -> Result<_, AppError>` and registered in the `invoke_handler`. Fields serialize to camelCase? No — keep snake_case server-side unless existing handlers alias; all handlers in this app emit snake_case JSON (`due_date`, `source_entry_id`), so frontend keys stay snake_case.

## 3. Rewires

### 3.1 Inbox "accept fact" (`proposed_action` — `lib.rs:1266-1292`)
Replace `profile::upsert_fact` with a `create_item` call:
- `kind='information'`, `category` from payload (validate against the CHECK list; fall back `other`), `confidence` from payload (default 0.8), `provenance='ai'`, `source_entry_id=proposal.source_entry_id`, `created_by` tracked via `created_by` on the row (manual/ai). `fact_key` → `title`, payload `content` → `content`. Keep `update_proposal_status(…, "accepted")`.
- Delete `use crate::db::profile;` and drop `profile.rs` module + file.

### 3.2 AI chat context (`lib.rs:745-751`)
`let facts = get_facts…` block becomes:
- `db::knowledge::list_items(conn, None, None, None)` → take top ~10 by `updated_at DESC`, build one-line strings `• {title}: {content_tail}` (content truncated to ~120 chars). Injected as a "Memory" context block (D-94 compact budget); `unwrap_or_default()` on DB failure stays.

### 3.3 AI tools (`ai/tools.rs`, `ai/prompt.rs`)
Add 5 tools to the existing tool system (D-111 count grows — record as D-134):

| Tool | Writes? | Confirm? | Backend |
|------|---------|----------|---------|
| `list_memory` | No | No | `memory_list` with optional search |
| `create_memory` | Yes | Yes (Review-Inbox proposal) | `memory_create` |
| `update_memory` | Yes | Yes (Review-Inbox proposal) | `memory_update` |
| `delete_memory` | Yes | Yes (Review-Inbox proposal) | `memory_delete` |
| `link_memory` | Yes | Yes (Review-Inbox proposal) | `memory_link` (+ `memory_search`-style pick by id from `memory_get`) |

`ToolExecutor` wires each write tool to create a `proposed_inbox` row (`item_type = "memory"`, `memory_action` in the payload); the user confirms it in the Inbox tab, where `proposed_action` applies the change. This is the existing D-19 confirmation machinery — there is **no** inline chat `YES`/`NO` timer in this codebase. Prompt composer gains a **Memory** block: explains items are the user's knowledge base, read via `list_memory`, ask before writing, keep titles short and content factual.

### 3.4 Analysis worker
Leave the graphqlite triplet writes untouched (harmless dead writes, out of scope).

## 4. Frontend

### 4.1 `src/js/tabs/memory.js` (rewrite as ES module)
- `export function initMemory()` — no canvas, no physics. Uses `invoke` from `../ipc.js` and `escapeHtml` from `../sanitize.js`.
- State: `items`, `currentKind` (`''|memory|information|knowledge`), `currentCategory` (`''|preference|habit|constraint|person|goal|other`), `searchTerm`.
- `renderGrid()` — cards with `data-id`; content preview truncated by CSS; empty-state when none.
- Detail panel (right-in-tab or slide-in reusing `.detail-panel`): full content, kind/category/tags/confidence, provenance + source_entry_id link, created/updated, **Connections** list (each row `relation → target title`, remove button), **Add link** (relation `<select>` preset + mini search input with debounced `memory_list`, reuse the task-dependency mini-search pattern), **Edit** → swaps panel to form, **Delete** (confirm).
- Add/Edit form fields: title, kind `<select>`, category `<select>`, tags (comma text → JSON array), item_date (`<input type="date">`), confidence (number 0-1, default 1.0), content `<textarea>`. Save → `memory_create`/`memory_update` → re-fetch + re-render. Cancel returns to previous state.
- All `innerHTML` goes through `escapeHtml`; links handling via event delegation on the grid/panel (no inline `onclick`).
- XSS guard on provenance/source/tags: everything user-visible escaped.

### 4.2 `src/js/app.js`
Replace the lazy `loadView('memory')` + classic-script block (`app.js:155-166`) with `import('./tabs/memory.js').then((m) => m.initMemory())`, keeping the same load-once semantics (`memoryLoaded` guard).

### 4.3 `src/views/memory.html` (rebuild)
Retains only the shell: header (title + count + "＋ Add item"), toolbar (search box, kind switch, category sidebar), `#memory-grid`, `#memory-detail` slide-in, `#memory-form`. **No inline `<style>`** — remove the old `<style>` block; all CSS moves to a new `src/styles/memory.css` (linked from `src/index.html`), using existing tokens (`--color-*`, `--radius`, `--space`) and reusing `.tasks-header`, `.task-search-box`, `.detail-panel`, `.form-group`, `.btn-*` patterns. `#memory-view .hidden` semantics: `.hidden { display:none !important }` already exists in `base.css:78`; the new view must not redefine it.

## 5. Error handling

- All handlers `Result<T, AppError>`; missing IDs → `AppError::NotFound` (add variant if absent) + frontend `ipc.js` toast.
- Duplicate-link create → surfaces as `UNIQUE` constraint error → catch and toast "link already exists" (idempotency not enforced by caller).
- Migration failure aborts open (runner is transactional per migration).

## 6. Verification

No automated test suite (removed): verification = migration applied (`cargo build` + launch → log line `[DB] Applying migration 12`), `node --check` on changed JS, `cargo check` 0 errors, then manual smoke:
1. Launch → migration 012 applies on the existing keyed DB (vault unlock path).
2. Memory tab: add manual item (all kinds), edit, delete; search/filter works.
3. Connect two items (each preset relation); link appears in both directions' connection list; unlink works.
4. Inbox: accept a fact proposal → item appears in Memory with category/confidence/source intact; old `user_profile` table gone.
5. AI chat: `list_memory` returns items; `create_memory` raises an Inbox proposal; accepting it in the Inbox creates the item; nothing is written without acceptance.
6. Old `graph_query`/`profile_*` calls gone — no `Command not found` toasts.

## Decisions added (AGENTS.md)
- D-134: Memory = unified `knowledge_items` + typed `knowledge_links` (replaces `user_profile` + graph-queried map).
- D-135: Preset relations only: `related-to`, `part-of`, `contradicts`, `cause-of`, `reminds-of`.
- D-136: AI memory writes go through Review-Inbox proposals (`proposed_inbox` `item_type = "memory"`), confirmed by the user in the Inbox tab — extends D-19 to the knowledge base (no inline chat YES/NO timer).

## Out of scope (later versions)
- Neural Map / force-graph visualization.
- Item-level "reinforcement" / prominence decay.
- Multi-user / shared memory.
- Deleting the graphqlite graph + its analysis-worker triplets.

## Implementation recap (as built)

- **Confirmation mechanics:** AI memory writes create `proposed_inbox` rows (`item_type = "memory"`, `memory_action` ∈ `create|update|delete|link`); the user confirms in the Inbox tab, where `proposed_action` applies the change inside `with_tx`. There is **no** inline 300s chat YES/NO timer in the codebase — the spec's original "300s" wording was aspirational and has been corrected above.
- **`ExportFact` JSON contract unchanged:** the backup export/import JSON shape stays `{ id, fact_key, content, category, source, confidence, created_at, updated_at }`; only the SQL behind it now reads/writes `knowledge_items` (kind `information`, `provenance`/`created_by` `ai`).
- **Serde input handling:** `KnowledgeItem` carries `#[serde(default)]` on `item_date`, `provenance`, `created_by`, `source_entry_id`, `created_at`, `updated_at` so the frontend create/update payloads (which omit provenance/timestamps) deserialize; `memory_create` fills `provenance`/`created_by`/timestamps and clamps confidence.
- **Frontend CSS tokens:** the real theme tokens are `--color-border`, `--color-text`, `--color-muted`, `--color-accent`, `--color-surface`, `--color-surface-soft`, `--color-bg`, `--color-warn`, `--color-success`, `--color-*-soft`, `--radius`, `--radius-sm` — not the `--border-color`/`--text-*`/`--color-primary` names sketched during design. `memory.css` defines its own `.btn-icon`, `.mini-search-result`, and `.mini-search-empty` (they had no shared styles).
- **Deletions:** `db/profile.rs` removed; `app_diagnostics` now counts `knowledge_items` instead of `user_profile`.