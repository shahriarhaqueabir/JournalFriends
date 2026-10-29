# Memory / Knowledge Base v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the Memory tab as a shared knowledge base (`knowledge_items` + typed `knowledge_links`) with full CRUD commands, migration of the old `user_profile` facts, AI tools that propose memory writes into the Review Inbox, and a card-grid + detail-panel frontend.

**Architecture:** Drop the dead `user_profile`/`ProfileFact`/graph path; add migration `012`, a new `db/knowledge.rs` module, 7 Tauri commands, and 5 AI tools. The frontend is a rebuild of `views/memory.html` + `js/tabs/memory.js` (ES module, `ipc.js` + `escapeHtml`), styled by a new `memory.css`.

**Tech Stack:** Rust (rusqlite, SQLCipher), Tauri v2 commands, vanilla ES modules.

## Global Constraints

- Migration files MUST be wrapped in one outer `BEGIN;`/`COMMIT;` pair and contain NO interior transaction keywords (`db/migrations.rs` strips the outer pair, then rejects any leftover `BEGIN`/`COMMIT`).
- Column names are byte-identical across the SQL DDL, `db/knowledge.rs` structs/rows, Tauri JSON output, and frontend JS keys. All snake_case: `id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at`.
- `kind` ∈ `memory|information|knowledge`; `category` ∈ `preference|habit|constraint|person|goal|other`; `relation` ∈ `related-to|part-of|contradicts|cause-of|reminds-of`; `provenance` ∈ `manual|ai|journal`; `created_by` ∈ `user|ai`.
- All handlers return `Result<T, AppError>`, never panic. DB writes inside `proposed_action` use `with_tx`; reads use `pool::with_conn`.
- Frontend: every `innerHTML` segment escapes user/model data via `escapeHtml`; no inline `onclick`; invoke goes through `ipc.js`.
- The AI NEVER writes memory directly — write tools create `proposed_inbox` rows (item_type `memory`); the user confirms in the Inbox tab (this is the existing D-19 confirmation machinery — there is no inline chat YES/NO timer in this codebase).
- `user_profile` is dropped by migration 012 → all its references (`lib.rs`, `prompt.rs`, `backup/export.rs`) are updated in this plan.

---

### Task 1: Migration 012 + registration

**Files:**
- Create: `src-tauri/migrations/012_knowledge.sql`
- Modify: `src-tauri/src/db/migrations.rs`

**Interfaces:**
- Consumes: no prior tasks (schema is defined here).
- Produces: tables `knowledge_items`, `knowledge_links`; all other tasks depend on this exact DDL.

- [ ] **Step 1: Create `src-tauri/migrations/012_knowledge.sql`**

```sql
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
```

- [ ] **Step 2: Register it in the runner — insert after the `journal_vectors` Migration struct (~line 66)**

```rust
    Migration {
        version: 12,
        name: "knowledge_base",
        sql: include_str!("../../migrations/012_knowledge.sql"),
    },
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: 0 errors (migration SQL is a plain include_str string — compile proves paths, not SQL).

- [ ] **Step 4: Verify the SQL parses against a scratch DB**

```bash
printf '  002_fts_triggers.sql\n  003_analysis_tracking.sql\n  004_field_expansion.sql\n  005_settings_v16.sql\n  006_user_profile.sql\n  007_provenance.sql\n  008_events_notes.sql\n  009_analysis_queue.sql\n  010_event_reminders.sql\n  011_journal_vectors.sql\n  012_knowledge.sql\n' > /tmp/migr.txt
```

Instead, run the app once (Task 8 does the full smoke); here run `cargo check` only. SQL is exercised in Task 8.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/migrations/012_knowledge.sql src-tauri/src/db/migrations.rs
git commit -m "feat: add knowledge base tables migration 012"
```

---

### Task 2: `db/knowledge.rs` module

**Files:**
- Create: `src-tauri/src/db/knowledge.rs`
- Modify: `src-tauri/src/db/mod.rs` (`pub mod profile;` → `pub mod knowledge;`)
- Delete: `src-tauri/src/db/profile.rs`

**Interfaces:**
- Consumes: Task 1 DDL.
- Produces: `KnowledgeItem`, `KnowledgeLink`, `KnowledgeLinkRow`, `KnowledgeItemDetail`, `list_items`, `get_item`, `get_item_detail`, `create_item`, `update_item`, `delete_item`, `link_items`, `unlink_item`, `validate_kind`, `validate_category`, `validate_relation`.

- [ ] **Step 1: Create `src-tauri/src/db/knowledge.rs` with sections in this exact order**

```rust
use crate::error::AppError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KnowledgeItem {
    pub id: String,
    pub title: String,
    pub content: String,
    pub kind: String,
    pub category: String,
    pub tags: String,
    pub item_date: Option<String>,
    pub confidence: f64,
    pub provenance: String,
    pub created_by: String,
    pub source_entry_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KnowledgeLink {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KnowledgeLinkRow {
    pub id: String,
    pub direction: String, // "out" => this item is the source; "in" => this item is the target
    pub relation: String,
    pub other_id: String,
    pub other_title: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KnowledgeItemDetail {
    pub item: KnowledgeItem,
    pub links: Vec<KnowledgeLinkRow>,
}

// SELECT column order for knowledge_items — keep in sync with row_to_item and the SQL strings above.
const ITEM_COLS: &str = "id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at";

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeItem> {
    Ok(KnowledgeItem {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        kind: row.get(3)?,
        category: row.get(4)?,
        tags: row.get(5)?,
        item_date: row.get(6)?,
        confidence: row.get(7)?,
        provenance: row.get(8)?,
        created_by: row.get(9)?,
        source_entry_id: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

pub fn validate_kind(v: &str) -> bool {
    matches!(v, "memory" | "information" | "knowledge")
}

pub fn validate_category(v: &str) -> bool {
    matches!(v, "preference" | "habit" | "constraint" | "person" | "goal" | "other")
}

pub fn validate_relation(v: &str) -> bool {
    matches!(v, "related-to" | "part-of" | "contradicts" | "cause-of" | "reminds-of")
}

pub fn list_items(
    conn: &Connection,
    kind: Option<&str>,
    category: Option<&str>,
    search: Option<&str>,
) -> Result<Vec<KnowledgeItem>, AppError> {
    let mut sql = format!("SELECT {ITEM_COLS} FROM knowledge_items");
    let mut conds: Vec<String> = Vec::new();
    let mut vals: Vec<String> = Vec::new();

    if let Some(k) = kind {
        if !k.is_empty() {
            conds.push("kind = ?".into());
            vals.push(k.to_string());
        }
    }
    if let Some(c) = category {
        if !c.is_empty() {
            conds.push("category = ?".into());
            vals.push(c.to_string());
        }
    }
    if let Some(s) = search {
        if !s.is_empty() {
            conds.push("(title LIKE ? OR content LIKE ?)".into());
            let term = format!("%{}%", s);
            vals.push(term.clone());
            vals.push(term);
        }
    }
    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }
    sql.push_str(" ORDER BY updated_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(vals.iter()), |row| row_to_item(row))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_item(conn: &Connection, id: &str) -> Result<Option<KnowledgeItem>, AppError> {
    let sql = format!("SELECT {ITEM_COLS} FROM knowledge_items WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], |row| row_to_item(row))?;
    match rows.next() {
        Some(Ok(item)) => Ok(Some(item)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn get_item_detail(conn: &Connection, id: &str) -> Result<Option<KnowledgeItemDetail>, AppError> {
    let Some(item) = get_item(conn, id)? else {
        return Ok(None);
    };

    let mut links = Vec::new();
    let mut stmt_out = conn.prepare(
        "SELECT kl.id, kl.relation, kl.created_by, kl.created_at, ki.id, ki.title
         FROM knowledge_links kl
         JOIN knowledge_items ki ON ki.id = kl.target_id
         WHERE kl.source_id = ?1
         ORDER BY kl.created_at ASC",
    )?;
    let out_rows = stmt_out.query_map(params![id], |row| {
        Ok(KnowledgeLinkRow {
            id: row.get(0)?,
            direction: "out".to_string(),
            relation: row.get(1)?,
            other_id: row.get(4)?,
            other_title: row.get(5)?,
            created_by: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    for r in out_rows {
        links.push(r?);
    }

    let mut stmt_in = conn.prepare(
        "SELECT kl.id, kl.relation, kl.created_by, kl.created_at, ki.id, ki.title
         FROM knowledge_links kl
         JOIN knowledge_items ki ON ki.id = kl.source_id
         WHERE kl.target_id = ?1
         ORDER BY kl.created_at ASC",
    )?;
    let in_rows = stmt_in.query_map(params![id], |row| {
        Ok(KnowledgeLinkRow {
            id: row.get(0)?,
            direction: "in".to_string(),
            relation: row.get(1)?,
            other_id: row.get(4)?,
            other_title: row.get(5)?,
            created_by: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    for r in in_rows {
        links.push(r?);
    }

    Ok(Some(KnowledgeItemDetail { item, links }))
}

pub fn create_item(conn: &Connection, item: &KnowledgeItem) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO knowledge_items
            (id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            item.id, item.title, item.content, item.kind, item.category,
            item.tags, item.item_date, item.confidence, item.provenance,
            item.created_by, item.source_entry_id, item.created_at, item.updated_at,
        ],
    )?;
    Ok(())
}

pub fn update_item(conn: &Connection, item: &KnowledgeItem) -> Result<(), AppError> {
    conn.execute(
        "UPDATE knowledge_items
         SET title = ?1, content = ?2, kind = ?3, category = ?4,
             tags = ?5, item_date = ?6, confidence = ?7, updated_at = ?8
         WHERE id = ?9",
        params![
            item.title, item.content, item.kind, item.category,
            item.tags, item.item_date, item.confidence, item.updated_at, item.id,
        ],
    )?;
    Ok(())
}

pub fn delete_item(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM knowledge_items WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn link_items(conn: &Connection, link: &KnowledgeLink) -> Result<(), AppError> {
    let changed = conn.execute(
        "INSERT INTO knowledge_links (id, source_id, target_id, relation, created_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(source_id, target_id, relation) DO NOTHING",
        params![link.id, link.source_id, link.target_id, link.relation, link.created_by, link.created_at],
    )?;
    if changed == 0 {
        return Err(AppError::InvalidInput(
            format!("Link already exists: {} -> {} ({})", link.source_id, link.target_id, link.relation),
        ));
    }
    Ok(())
}

pub fn unlink_item(conn: &Connection, link_id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM knowledge_links WHERE id = ?1", params![link_id])?;
    Ok(())
}
```

- [ ] **Step 2: Update `src-tauri/src/db/mod.rs`**

Replace line `pub mod profile;` with `pub mod knowledge;`.

- [ ] **Step 3: Delete `src-tauri/src/db/profile.rs`**

```bash
git rm src-tauri/src/db/profile.rs
```

- [ ] **Step 4: Verify**

Run: `cargo check`
Expected: 0 errors IF `lib.rs`/`prompt.rs` no longer reference `profile::` — that happens in Tasks 3-4. For this task, cargo will error on the stale references; proceed to Task 3 and Task 4, then re-run `cargo check`. (If you prefer to keep each task green, apply Task 3 + Task 4 SQL edits immediately after this task.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db/knowledge.rs src-tauri/src/db/mod.rs
git commit -m "feat: add knowledge base store module"
```

---

### Task 3: Tauri commands + diagnostics fix

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Task 2 module functions.
- Produces: commands `memory_list`, `memory_get`, `memory_create`, `memory_update`, `memory_delete`, `memory_link`, `memory_unlink` (registered in `invoke_handler`). Frontend (Task 6) and tools (Task 5) call them.

- [ ] **Step 1: Add the command handlers**

Insert **before the `proposed_action` command** in `src-tauri/src/lib.rs`. The `proposed_action` signature starts at line 1187; insert a blank line then these functions above it:

```rust
#[tauri::command]
async fn memory_list(
    state: tauri::State<'_, AppState>,
    kind: Option<String>,
    category: Option<String>,
    search: Option<String>,
) -> Result<Vec<crate::db::knowledge::KnowledgeItem>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        crate::db::knowledge::list_items(conn, kind.as_deref(), category.as_deref(), search.as_deref())
    })
    .await
}

#[tauri::command]
async fn memory_get(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<crate::db::knowledge::KnowledgeItemDetail, AppError> {
    let db = state.db()?;
    let detail = db::pool::with_conn(db, move |conn| crate::db::knowledge::get_item_detail(conn, &id))
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Memory item {} not found", id)))?;
    Ok(detail)
}

#[tauri::command]
async fn memory_create(
    state: tauri::State<'_, AppState>,
    input: crate::db::knowledge::KnowledgeItem,
) -> Result<crate::db::knowledge::KnowledgeItem, AppError> {
    if !crate::db::knowledge::validate_kind(&input.kind) {
        return Err(AppError::InvalidInput(format!("Invalid kind: {}", input.kind)));
    }
    if !crate::db::knowledge::validate_category(&input.category) {
        return Err(AppError::InvalidInput(format!("Invalid category: {}", input.category)));
    }
    let now = Utc::now().to_rfc3339();
    let mut item = input;
    item.id = if item.id.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        item.id.clone()
    };
    item.provenance = if item.provenance.is_empty() { "manual".into() } else { item.provenance };
    item.created_by = if item.created_by.is_empty() { "user".into() } else { item.created_by };
    item.confidence = item.confidence.clamp(0.0, 1.0);
    item.created_at = now.clone();
    item.updated_at = now;

    let db = state.db()?;
    let item_clone = item.clone();
    db::pool::with_conn(db, move |conn| crate::db::knowledge::create_item(conn, &item_clone)).await?;
    Ok(item)
}

#[tauri::command]
async fn memory_update(
    state: tauri::State<'_, AppState>,
    input: crate::db::knowledge::KnowledgeItem,
) -> Result<crate::db::knowledge::KnowledgeItem, AppError> {
    if !crate::db::knowledge::validate_kind(&input.kind) {
        return Err(AppError::InvalidInput(format!("Invalid kind: {}", input.kind)));
    }
    if !crate::db::knowledge::validate_category(&input.category) {
        return Err(AppError::InvalidInput(format!("Invalid category: {}", input.category)));
    }
    let db = state.db()?;
    let item_id = input.id.clone();
    let existing = db::pool::with_conn(db.clone(), move |conn| {
        crate::db::knowledge::get_item(conn, &item_id)
    })
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Memory item {} not found", input.id)))?;

    let mut updated = existing.clone();
    updated.title = input.title;
    updated.content = input.content;
    updated.kind = input.kind;
    updated.category = input.category;
    updated.tags = input.tags;
    updated.item_date = input.item_date;
    updated.confidence = input.confidence.clamp(0.0, 1.0);
    updated.updated_at = Utc::now().to_rfc3339();

    let updated_clone = updated.clone();
    db::pool::with_conn(db, move |conn| crate::db::knowledge::update_item(conn, &updated_clone)).await?;
    Ok(updated)
}

#[tauri::command]
async fn memory_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| crate::db::knowledge::delete_item(conn, &id)).await?;
    Ok(())
}

#[tauri::command]
async fn memory_link(
    state: tauri::State<'_, AppState>,
    source_id: String,
    target_id: String,
    relation: String,
    created_by: String,
) -> Result<crate::db::knowledge::KnowledgeLink, AppError> {
    if !crate::db::knowledge::validate_relation(&relation) {
        return Err(AppError::InvalidInput(format!("Invalid relation: {}", relation)));
    }
    if source_id == target_id {
        return Err(AppError::InvalidInput("Cannot link an item to itself".into()));
    }
    let now = Utc::now().to_rfc3339();
    let link = crate::db::knowledge::KnowledgeLink {
        id: Uuid::new_v4().to_string(),
        source_id,
        target_id,
        relation,
        created_by: if created_by.is_empty() { "user".into() } else { created_by },
        created_at: now,
    };
    let db = state.db()?;
    let link_clone = link.clone();
    db::pool::with_conn(db, move |conn| crate::db::knowledge::link_items(conn, &link_clone)).await?;
    Ok(link)
}

#[tauri::command]
async fn memory_unlink(state: tauri::State<'_, AppState>, link_id: String) -> Result<(), AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| crate::db::knowledge::unlink_item(conn, &link_id)).await?;
    Ok(())
}
```

- [ ] **Step 2: Register the commands** — add to the `tauri::generate_handler![...]` list (after `proposed_action,`):

```rust
            memory_list,
            memory_get,
            memory_create,
            memory_update,
            memory_delete,
            memory_link,
            memory_unlink,
```

- [ ] **Step 3: Fix `app_diagnostics`** — replace line 1012:

```rust
            count_rows(conn, "knowledge_items").unwrap_or(0),
```

- [ ] **Step 4: Verify**

Run: `cargo check`
Expected: fails only if Tasks 2/4 remain incomplete (stale `profile::` refs). Details in Task 2 Step 4.

---

### Task 4: Rewire chat context + prompt

**Files:**
- Modify: `src-tauri/src/ai/prompt.rs`
- Modify: `src-tauri/src/lib.rs` (context injection + `proposed_action` fact branch + remove `use crate::db::profile`)

**Interfaces:**
- Consumes: Task 2 `KnowledgeItem` + `list_items`.
- Produces: `get_chat_system_prompt(&[KnowledgeItem], ...)`; the accept branch for inbox item_type `fact` writes a `KnowledgeItem` (kind `information`).

- [ ] **Step 1: Change `prompt.rs` signature and fact rendering**

Replace the signature (line 31-37):

```rust
pub fn get_chat_system_prompt(
    facts: &[crate::db::knowledge::KnowledgeItem],
    journal_context: &[crate::db::journal::JournalContextSnippet],
    task_context: &[crate::db::tasks::Task],
    event_context: &[crate::db::events::Event],
    weekly_review: Option<&crate::db::reports::WeeklyReviewReport>,
) -> String {
```

Replace the facts block near `## Knowledge Base` (lines 90-104):

```rust
    prompt.push_str(
        "
## Knowledge Base
You have access to the user's Memory — durable memories, information, and knowledge:
",
    );

    if facts.is_empty() {
        prompt.push_str("- No memory items yet.\n");
    } else {
        for fact in facts.iter().take(10) {
            let content_tail: String = fact.content.chars().take(120).collect();
            prompt.push_str(&format!("- {}: {} ({})\n", fact.title, content_tail, fact.category));
        }
    }
```

- [ ] **Step 2: Swap the context source in `lib.rs:745-751`**

Replace:

```rust
    // Build context: memory bank facts + journal snippets (Hybrid Search) + open tasks
    let facts = {
        let pool = pool.clone();
        db::pool::with_conn(pool, |conn| crate::db::profile::get_facts(conn, None))
            .await
            .unwrap_or_default()
    };
```

with:

```rust
    // Build context: memory items + journal snippets (Hybrid Search) + open tasks
    let facts = {
        let pool = pool.clone();
        db::pool::with_conn(pool, |conn| crate::db::knowledge::list_items(conn, None, None, None))
            .await
            .unwrap_or_default()
    };
```

- [ ] **Step 3: Rewire the `proposed_action` "fact" branch (lines 1255-1292)**

Replace the entire `"fact" => { ... }` arm (from the `let data` line through `profile::upsert_fact(tx, &fact)?;`) with:

```rust
                    "fact" => {
                        let data: serde_json::Value = serde_json::from_str(&proposal.proposed_data)
                            .unwrap_or(serde_json::json!({}));
                        let fact_key = data.get("fact_key")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&proposal.proposed_title)
                            .to_string();
                        let content = data.get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&proposal.proposed_title)
                            .to_string();
                        let category = data.get("category")
                            .and_then(|v| v.as_str())
                            .unwrap_or("other")
                            .to_string();
                        if !crate::db::knowledge::validate_category(&category) {
                            return Err(crate::error::AppError::InvalidInput(format!(
                                "Invalid fact category: {}", category
                            )));
                        }
                        let confidence = data.get("confidence")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.8)
                            .clamp(0.0, 1.0);

                        let now = Utc::now().to_rfc3339();
                        let item = crate::db::knowledge::KnowledgeItem {
                            id: Uuid::new_v4().to_string(),
                            title: fact_key,
                            content,
                            kind: "information".to_string(),
                            category,
                            tags: "[]".to_string(),
                            item_date: None,
                            confidence,
                            provenance: "ai".to_string(),
                            created_by: proposal.created_by.clone().unwrap_or_else(|| "ai".to_string()),
                            source_entry_id: proposal.source_entry_id.clone(),
                            created_at: now.clone(),
                            updated_at: now,
                        };
                        crate::db::knowledge::create_item(tx, &item)?;

                        ai::update_proposal_status(tx, &id, "accepted", None, Some("user"), Some(&Utc::now().to_rfc3339()))?;
                    }
```

- [ ] **Step 4: Drop the stale import**

Remove the line `use crate::db::profile;` from the `proposed_action` fn's `use` block (lines 1194-1196). If the block ends up empty, remove it too.

- [ ] **Step 5: Verify**

Run: `cargo check`
Expected: 0 errors (Tasks 2-4 now align).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/ai/prompt.rs src-tauri/src/lib.rs
git commit -m "feat: route chat context and fact accept through knowledge store"
```

---

### Task 5: AI memory tools (proposals)

**Files:**
- Modify: `src-tauri/src/ai/tools.rs`
- Modify: `src-tauri/src/lib.rs` (`proposed_action` gains a `"memory"` accept branch)

**Interfaces:**
- Consumes: Task 2 module functions; existing `crate::db::ai::insert_proposed_item`.
- Produces: tool defs `list_memory`, `create_memory`, `update_memory`, `delete_memory`, `link_memory`; `proposed_inbox` rows with `item_type = "memory"`.

- [ ] **Step 1: Add `execute` arms in `tools.rs`**

In `ToolExecutor::execute` (line 45-61), add after the `"complete_task"` arm:

```rust
            "list_memory" => self.exec_list_memory(args).await,
            "create_memory" => self.propose_memory_action(args, "create").await,
            "update_memory" => self.propose_memory_action(args, "update").await,
            "delete_memory" => self.propose_memory_action(args, "delete").await,
            "link_memory" => self.propose_memory_action(args, "link").await,
```

- [ ] **Step 2: Add the executor methods**

Add these methods to the `impl ToolExecutor` block (before the closing `}` at line 416). `insert_proposed_item` signature is `(conn, id, Option<&str> conversation_id, Option<&str> source_entry_id, &str item_type, &str proposed_title, &str proposed_data, Option<&str> status, Option<f64> confidence, Option<&str> created_by, &str created_at)`:

```rust
    async fn exec_list_memory(&self, args: Value) -> ToolResult {
        let search = args
            .get("search")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let items = pool::with_conn(self.db_pool.clone(), move |conn| {
            crate::db::knowledge::list_items(conn, None, None, search.as_deref())
        })
        .await
        .unwrap_or_default();

        let summaries: Vec<Value> = items
            .iter()
            .map(|i| {
                json!({
                    "id": i.id,
                    "title": i.title,
                    "kind": i.kind,
                    "category": i.category,
                    "content": i.content,
                    "tags": i.tags,
                })
            })
            .collect();

        ToolResult {
            success: true,
            message: format!("Found {} memory item(s).", summaries.len()),
            data: json!({"items": summaries, "count": summaries.len()}),
            proposal_id: None,
        }
    }

    async fn propose_memory_action(&self, args: Value, action: &str) -> ToolResult {
        let now = chrono::Utc::now().to_rfc3339();
        let proposal_id = uuid::Uuid::new_v4().to_string();
        let proposal_id_clone = proposal_id.clone();

        let mut data_obj = args.as_object().cloned().unwrap_or_default();
        data_obj.insert("type".to_string(), json!("memory"));
        data_obj.insert("memory_action".to_string(), json!(action));
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .or_else(|| args.get("item_id").and_then(|v| v.as_str()))
            .unwrap_or("memory")
            .to_string();
        let data_str = Value::Object(data_obj).to_string();

        let summary = format!(
            "{} memory item{}{}",
            action,
            if action == "create" { "" } else { "" },
            if title.is_empty() { "" } else { &format!(": {}", title) }
        );

        let pool = self.db_pool.clone();
        if let Err(error) = pool::with_conn(pool, move |conn| {
            crate::db::ai::insert_proposed_item(
                conn,
                &proposal_id_clone,
                None,
                None,
                "memory",
                &summary,
                &data_str,
                None,
                None,
                Some("ai"),
                &now,
            )
        })
        .await
        {
            return ToolResult {
                success: false,
                message: format!("Could not propose memory action: {error}"),
                data: json!(null),
                proposal_id: None,
            };
        }

        ToolResult {
            success: true,
            message: "Proposed memory update. User must confirm in Inbox.".into(),
            data: json!({"proposal_id": proposal_id}),
            proposal_id: Some(proposal_id),
        }
    }
```

- [ ] **Step 3: Add the 5 definitions to `get_tools()`**

Append these five entries at the end of the `vec![...]` (after the `fetch_url` entry at line 549, before the closing `]`):

```rust
        json!({
            "type": "function",
            "function": {
                "name": "list_memory",
                "description": "Search the user's memory/knowledge base (memories, information, knowledge about the user). Read-only.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "search": { "type": "string", "description": "Optional keyword to filter memory items by title or content" }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_memory",
                "description": "Propose adding a memory item to the user's knowledge base. The user confirms in the Review Inbox before it is saved.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Short label for the item" },
                        "content": { "type": "string", "description": "The factual content to remember" },
                        "kind": { "type": "string", "enum": ["memory", "information", "knowledge"], "description": "memory = dated personal memory, information = fact about user, knowledge = learned concept" },
                        "category": { "type": "string", "enum": ["preference", "habit", "constraint", "person", "goal", "other"] },
                        "tags": { "type": "array", "items": { "type": "string" }, "description": "Optional tags" },
                        "item_date": { "type": "string", "description": "Optional YYYY-MM-DD date if this is a dated memory" }
                    },
                    "required": ["title", "content", "kind"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "update_memory",
                "description": "Propose editing an existing memory item (title, content, kind, category, tags, date, confidence). Requires the item ID (use list_memory first).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "item_id": { "type": "string", "description": "The ID of the memory item to edit" },
                        "title": { "type": "string" },
                        "content": { "type": "string" },
                        "kind": { "type": "string", "enum": ["memory", "information", "knowledge"] },
                        "category": { "type": "string", "enum": ["preference", "habit", "constraint", "person", "goal", "other"] },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "item_date": { "type": "string" },
                        "confidence": { "type": "number" }
                    },
                    "required": ["item_id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "delete_memory",
                "description": "Propose deleting a memory item permanently. Requires the item ID.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "item_id": { "type": "string", "description": "The ID of the memory item to delete" }
                    },
                    "required": ["item_id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "link_memory",
                "description": "Propose linking two memory items with a typed relation. Requires the source and target item IDs and one of the preset relations.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "source_id": { "type": "string", "description": "Source item ID" },
                        "target_id": { "type": "string", "description": "Target item ID" },
                        "relation": { "type": "string", "enum": ["related-to", "part-of", "contradicts", "cause-of", "reminds-of"] }
                    },
                    "required": ["source_id", "target_id", "relation"]
                }
            }
        }),
```

- [ ] **Step 4: Add the `"memory"` accept branch in `proposed_action`**

In `match action.as_str()` → `"accept"` → inner `match proposal.item_type.as_str()`, add after the `"fact" => { ... }` arm (before `"event" =>`):

```rust
                    "memory" => {
                        let data: serde_json::Value = serde_json::from_str(&proposal.proposed_data)
                            .unwrap_or(serde_json::json!({}));
                        let sub_action = data.get("memory_action")
                            .and_then(|v| v.as_str())
                            .unwrap_or("create");
                        let now = Utc::now().to_rfc3339();

                        match sub_action {
                            "update" => {
                                let item_id = data.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
                                if item_id.is_empty() {
                                    return Err(crate::error::AppError::InvalidInput(
                                        "memory update proposal missing item_id".into(),
                                    ));
                                }
                                let item = crate::db::knowledge::get_item(tx, item_id)?
                                    .ok_or_else(|| crate::error::AppError::NotFound(format!("Memory item {} not found", item_id)))?;
                                let mut updated = item.clone();
                                if let Some(t) = data.get("title").and_then(|v| v.as_str()) {
                                    updated.title = t.to_string();
                                }
                                if let Some(c) = data.get("content").and_then(|v| v.as_str()) {
                                    updated.content = c.to_string();
                                }
                                if let Some(k) = data.get("kind").and_then(|v| v.as_str()) {
                                    if !crate::db::knowledge::validate_kind(k) {
                                        return Err(crate::error::AppError::InvalidInput(format!("Invalid kind: {}", k)));
                                    }
                                    updated.kind = k.to_string();
                                }
                                if let Some(c) = data.get("category").and_then(|v| v.as_str()) {
                                    if !crate::db::knowledge::validate_category(c) {
                                        return Err(crate::error::AppError::InvalidInput(format!("Invalid category: {}", c)));
                                    }
                                    updated.category = c.to_string();
                                }
                                if let Some(t) = data.get("tags").and_then(|v| v.as_array()) {
                                    updated.tags = t.to_string();
                                }
                                if data.get("item_date").is_some() {
                                    updated.item_date = data.get("item_date").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
                                }
                                if let Some(c) = data.get("confidence").and_then(|v| v.as_f64()) {
                                    updated.confidence = c.clamp(0.0, 1.0);
                                }
                                updated.updated_at = now.clone();
                                crate::db::knowledge::update_item(tx, &updated)?;
                            }
                            "delete" => {
                                let item_id = data.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
                                if item_id.is_empty() {
                                    return Err(crate::error::AppError::InvalidInput(
                                        "memory delete proposal missing item_id".into(),
                                    ));
                                }
                                crate::db::knowledge::delete_item(tx, item_id)?;
                            }
                            "link" => {
                                let source_id = data.get("source_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let target_id = data.get("target_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let relation = data.get("relation").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                if source_id.is_empty() || target_id.is_empty() || !crate::db::knowledge::validate_relation(&relation) {
                                    return Err(crate::error::AppError::InvalidInput(
                                        "memory link proposal missing or invalid fields".into(),
                                    ));
                                }
                                if source_id == target_id {
                                    return Err(crate::error::AppError::InvalidInput(
                                        "Cannot link an item to itself".into(),
                                    ));
                                }
                                let link = crate::db::knowledge::KnowledgeLink {
                                    id: Uuid::new_v4().to_string(),
                                    source_id,
                                    target_id,
                                    relation,
                                    created_by: "ai".to_string(),
                                    created_at: now,
                                };
                                crate::db::knowledge::link_items(tx, &link)?;
                            }
                            _ => {
                                let title = data.get("title")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&proposal.proposed_title)
                                    .to_string();
                                let content = data.get("content")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&title)
                                    .to_string();
                                let kind = data.get("kind")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("information")
                                    .to_string();
                                if !crate::db::knowledge::validate_kind(&kind) {
                                    return Err(crate::error::AppError::InvalidInput(format!("Invalid kind: {}", kind)));
                                }
                                let category = data.get("category")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("other")
                                    .to_string();
                                if !crate::db::knowledge::validate_category(&category) {
                                    return Err(crate::error::AppError::InvalidInput(format!("Invalid category: {}", category)));
                                }
                                let confidence = data.get("confidence")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(1.0)
                                    .clamp(0.0, 1.0);
                                let tags = data.get("tags")
                                    .and_then(|v| v.as_array())
                                    .map(|a| a.to_string())
                                    .unwrap_or_else(|| "[]".to_string());
                                let item_date = data.get("item_date")
                                    .and_then(|v| v.as_str())
                                    .filter(|s| !s.is_empty())
                                    .map(|s| s.to_string());

                                let item = crate::db::knowledge::KnowledgeItem {
                                    id: Uuid::new_v4().to_string(),
                                    title,
                                    content,
                                    kind,
                                    category,
                                    tags,
                                    item_date,
                                    confidence,
                                    provenance: "ai".to_string(),
                                    created_by: proposal.created_by.clone().unwrap_or_else(|| "ai".to_string()),
                                    source_entry_id: proposal.source_entry_id.clone(),
                                    created_at: now.clone(),
                                    updated_at: now,
                                };
                                crate::db::knowledge::create_item(tx, &item)?;
                            }
                        }

                        ai::update_proposal_status(tx, &id, "accepted", None, Some("user"), Some(&Utc::now().to_rfc3339()))?;
                    }
```

- [ ] **Step 5: Verify**

Run: `cargo check`
Expected: 0 errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/ai/tools.rs src-tauri/src/lib.rs
git commit -m "feat: add memory tools with inbox confirmation proposals"
```

---

### Task 6: Backup export/import for knowledge items

**Files:**
- Modify: `src-tauri/src/backup/export.rs`

**Interfaces:**
- Consumes: Task 1 DDL (user_profile dropped).
- Produces: `ExportFact` renamed/mapped to `knowledge_items` shape; export + import wired.

- [ ] **Step 1: Rewrite `ExportFact`, the export block, and the import block**

Replace the `ExportFact` struct (lines 53-63) with:

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct ExportFact {
    pub id: String,
    pub fact_key: String,
    pub content: String,
    pub category: String,
    pub source: Option<String>,
    pub confidence: f64,
    pub created_at: String,
    pub updated_at: String,
}
```

(unchanged — the JSON export contract stays. Only the SQL behind it changes.)

Replace the `let facts = { ... }` block (lines 214-241) with:

```rust
    let facts = {
        let mut stmt = conn.prepare(
            "SELECT id, title, content, category, confidence, created_at, updated_at
             FROM knowledge_items",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExportFact {
                id: row.get(0)?,
                fact_key: row.get(1)?,
                content: row.get(2)?,
                category: row.get(3)?,
                source: None,
                confidence: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
```

Replace the import loop (lines 406-429) with:

```rust
    for fact in &data.facts {
        let count = conn.execute(
            "INSERT OR IGNORE INTO knowledge_items
             (id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'information', ?4, '[]', NULL, ?5, 'ai', 'ai', NULL, ?6, ?7)",
            params![
                fact.id,
                fact.fact_key,
                fact.content,
                fact.category,
                fact.confidence,
                fact.created_at,
                fact.updated_at,
            ],
        )?;
        result.facts += count as u64;
    }
```

- [ ] **Step 2: Verify**

Run: `cargo check`
Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/backup/export.rs
git commit -m "feat: migrate backup export/import to knowledge_items"
```

---

### Task 7: Database completeness check (`app_diagnostics` + stragglers)

**Files:**
- Modify: `src-tauri/src/lib.rs` (already done in Task 3/4) — plus a compile-time re-check of remaining `profile`/`user_profile` references.

- [ ] **Step 1: Grep for stale references**

Run:

```bash
rg -n "profile::|ProfileFact|user_profile|get_facts|upsert_fact" src-tauri/src
```

Expected: NO matches (any hit must be removed). Special cases: `src-tauri/src/ai/mod.rs` lines 48/93/141 reference `fact_key` on its own `Fact` structs — those are the analysis-draft types, NOT `db::profile`, and must be left untouched.

- [ ] **Step 2: Verify the whole crate**

Run: `cargo check`
Expected: 0 errors, warnings unchanged (pre-existing set only).

---

### Task 8: Memory tab frontend

**Files:**
- Create: `src/styles/memory.css`
- Create: `src/views/memory.html` (full rewrite)
- Delete: `src/js/tabs/memory.js` (rewrite, kept as file)
- Modify: `src/index.html` (link memory.css)
- Modify: `src/js/app.js` (lazy-loader → dynamic import)

**Interfaces:**
- Consumes: Task 3 commands (`memory_list`/`memory_get`/`memory_create`/`memory_update`/`memory_delete`/`memory_link`/`memory_unlink`), `ipc.js::invoke`, `sanitize.js::escapeHtml`.
- Produces: `initMemory()` exported from `js/tabs/memory.js`.

- [ ] **Step 1: Add the stylesheet link** in `src/index.html`, right after the `calendar.css` line (line 14):

```html
  <link rel="stylesheet" href="styles/memory.css" />
```

- [ ] **Step 2: Rewrite `src/views/memory.html`**

```html
<div id="memory-view" class="tab-content">
    <div class="memory-view-layout">
        <aside class="memory-sidebar">
            <div class="memory-sidebar-section">
                <div class="memory-sidebar-title">Kinds</div>
                <button class="memory-filter-btn active" data-kind="">All Kinds</button>
                <button class="memory-filter-btn" data-kind="memory">Memories</button>
                <button class="memory-filter-btn" data-kind="information">Information</button>
                <button class="memory-filter-btn" data-kind="knowledge">Knowledge</button>
            </div>
            <div class="memory-sidebar-section">
                <div class="memory-sidebar-title">Categories</div>
                <button class="memory-filter-btn cat-btn active" data-cat="">All Categories</button>
                <button class="memory-filter-btn cat-btn" data-cat="preference">Preferences</button>
                <button class="memory-filter-btn cat-btn" data-cat="habit">Habits</button>
                <button class="memory-filter-btn cat-btn" data-cat="constraint">Constraints</button>
                <button class="memory-filter-btn cat-btn" data-cat="person">People</button>
                <button class="memory-filter-btn cat-btn" data-cat="goal">Goals</button>
                <button class="memory-filter-btn cat-btn" data-cat="other">Other</button>
            </div>
        </aside>

        <div class="memory-main">
            <header class="memory-header">
                <h2>Memory</h2>
                <div class="memory-header-actions">
                    <span id="memory-count" class="memory-count"></span>
                    <div class="task-search-box">
                        <input type="text" id="memory-search" placeholder="Search memory..." />
                    </div>
                    <button id="btn-new-memory-item" class="btn-primary">+ Add Item</button>
                </div>
            </header>
            <div id="memory-grid" class="memory-grid">
                <div class="empty-state">
                    <div class="empty-state-icon">🧠</div>
                    <p>No memory items yet.<br><small>Add one manually or let the AI propose items from your journal.</small></p>
                </div>
            </div>
        </div>
    </div>

    <aside id="memory-detail" class="memory-detail-panel hidden" aria-label="Memory Item Details">
        <div id="memory-detail-content"></div>
    </aside>

    <div id="memory-form" class="memory-form-overlay hidden">
        <div class="memory-form-card">
            <header class="panel-header">
                <h3 id="memory-form-title">Add Memory Item</h3>
                <button type="button" id="btn-memory-form-close" class="btn-icon">×</button>
            </header>
            <div class="panel-content">
                <input type="hidden" id="memory-form-id" />
                <div class="form-group">
                    <label for="memory-form-kind">Kind</label>
                    <select id="memory-form-kind">
                        <option value="information">Information</option>
                        <option value="memory">Memory</option>
                        <option value="knowledge">Knowledge</option>
                    </select>
                </div>
                <div class="form-group">
                    <label for="memory-form-title-input">Title</label>
                    <input type="text" id="memory-form-title-input" placeholder="Short label (e.g. prefers black coffee)" />
                </div>
                <div class="form-group">
                    <label for="memory-form-content">Content</label>
                    <textarea id="memory-form-content" rows="5" placeholder="The detail to remember..."></textarea>
                </div>
                <div class="form-row">
                    <div class="form-group">
                        <label for="memory-form-category">Category</label>
                        <select id="memory-form-category">
                            <option value="preference">Preference</option>
                            <option value="habit">Habit</option>
                            <option value="constraint">Constraint</option>
                            <option value="person">Person</option>
                            <option value="goal">Goal</option>
                            <option value="other">Other</option>
                        </select>
                    </div>
                    <div class="form-group">
                        <label for="memory-form-tags">Tags (comma-separated)</label>
                        <input type="text" id="memory-form-tags" placeholder="coffee, mornings" />
                    </div>
                </div>
                <div class="form-row">
                    <div class="form-group">
                        <label for="memory-form-item-date">Date (for memories)</label>
                        <input type="date" id="memory-form-item-date" />
                    </div>
                    <div class="form-group">
                        <label for="memory-form-confidence">Confidence (0-1)</label>
                        <input type="number" id="memory-form-confidence" min="0" max="1" step="0.05" value="1" />
                    </div>
                </div>
            </div>
            <footer class="panel-footer">
                <button type="button" id="btn-memory-form-delete" class="btn-danger hidden">Delete</button>
                <button type="button" id="btn-memory-form-cancel" class="btn-ghost">Cancel</button>
                <button type="button" id="btn-memory-form-save" class="btn-primary">Save</button>
            </footer>
        </div>
    </div>
</div>
```

- [ ] **Step 3: Rewrite `src/js/tabs/memory.js`**

```js
import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";

const RELATIONS = ["related-to", "part-of", "contradicts", "cause-of", "reminds-of"];

let items = [];
let currentKind = "";
let currentCategory = "";
let searchTerm = "";
let detailId = null;
let linkSearchResults = [];

export async function initMemory() {
    const view = document.getElementById("memory-view");
    if (!view) return;

    document.getElementById("btn-new-memory-item").addEventListener("click", () => openForm(null));
    document.getElementById("memory-search").addEventListener("input", (e) => {
        searchTerm = e.target.value.trim().toLowerCase();
        refresh();
    });
    document.querySelectorAll("[data-kind]").forEach((btn) => {
        btn.addEventListener("click", () => {
            currentKind = btn.dataset.kind;
            document.querySelectorAll("[data-kind]").forEach((b) => b.classList.toggle("active", b === btn));
            refresh();
        });
    });
    document.querySelectorAll(".cat-btn").forEach((btn) => {
        btn.addEventListener("click", () => {
            currentCategory = btn.dataset.cat;
            document.querySelectorAll(".cat-btn").forEach((b) => b.classList.toggle("active", b === btn));
            refresh();
        });
    });
    document.getElementById("memory-grid").addEventListener("click", onGridClick);
    document.getElementById("memory-detail").addEventListener("click", onDetailClick);
    document.getElementById("memory-form").addEventListener("click", onFormClick);
    document.getElementById("btn-memory-form-close").addEventListener("click", () => closeForm());
    document.getElementById("btn-memory-link-search").addEventListener("click", doLinkSearch);
    document.getElementById("memory-link-search-input")?.addEventListener("keydown", (e) => {
        if (e.key === "Enter") { e.preventDefault(); doLinkSearch(); }
    });

    await refresh();
}

async function refresh() {
    try {
        items = await invoke("memory_list", {
            kind: currentKind || null,
            category: currentCategory || null,
            search: searchTerm || null,
        });
    } catch {
        return;
    }
    renderGrid();
    const countEl = document.getElementById("memory-count");
    if (countEl) countEl.textContent = `${items.length} item${items.length === 1 ? "" : "s"}`;
}

function renderGrid() {
    const grid = document.getElementById("memory-grid");
    if (!items.length) {
        grid.innerHTML = `<div class="empty-state"><div class="empty-state-icon">🧠</div><p>No memory items yet.<br><small>Add one manually or let the AI propose items from your journal.</small></p></div>`;
        return;
    }
    grid.innerHTML = items.map((i) => `
        <article class="memory-card" data-id="${escapeHtml(i.id)}" tabindex="0">
            <div class="memory-card-header">
                <span class="memory-kind-badge kind-${escapeHtml(i.kind)}">${escapeHtml(i.kind)}</span>
                ${i.item_date ? `<span class="memory-card-date">${escapeHtml(i.item_date)}</span>` : ""}
            </div>
            <h4 class="memory-card-title">${escapeHtml(i.title)}</h4>
            <p class="memory-card-content">${escapeHtml(i.content)}</p>
            <div class="memory-card-footer">
                <span class="memory-cat-chip">${escapeHtml(i.category)}</span>
                ${i.tags !== "[]" ? `<span class="memory-tags">${escapeHtml(JSON.parse(i.tags || "[]").join(", "))}</span>` : ""}
                <span class="memory-conf">${Math.round((i.confidence || 1) * 100)}%</span>
            </div>
        </article>
    `).join("");
}

function onGridClick(e) {
    const card = e.target.closest(".memory-card");
    if (card) openDetail(card.dataset.id);
}

async function openDetail(id) {
    let detail;
    try {
        detail = await invoke("memory_get", { id });
    } catch {
        return;
    }
    detailId = id;
    const { item, links } = detail;
    const panel = document.getElementById("memory-detail-content");
    const linkRows = (links || []).map((l) => `
        <div class="memory-link-row" data-link-id="${escapeHtml(l.id)}">
            <span class="memory-link-rel">${l.direction === "out" ? "→" : "←"} ${escapeHtml(l.relation)}</span>
            <button class="memory-link-target" data-link-open="${escapeHtml(l.other_id)}">${escapeHtml(l.other_title)}</button>
            <button class="btn-icon memory-link-remove" data-link-remove="${escapeHtml(l.id)}" title="Remove link">×</button>
        </div>
    `).join("");

    panel.innerHTML = `
        <header class="panel-header">
            <h3>${escapeHtml(item.title)}</h3>
            <button id="btn-close-memory-detail" class="btn-icon">×</button>
        </header>
        <div class="panel-content">
            <div class="memory-detail-meta">
                <span class="memory-kind-badge kind-${escapeHtml(item.kind)}">${escapeHtml(item.kind)}</span>
                <span class="memory-cat-chip">${escapeHtml(item.category)}</span>
                <span class="memory-conf">${Math.round((item.confidence || 1) * 100)}% confident</span>
            </div>
            <p class="memory-detail-content">${escapeHtml(item.content)}</p>
            ${item.item_date ? `<div class="memory-detail-meta-line">Date: <strong>${escapeHtml(item.item_date)}</strong></div>` : ""}
            ${item.tags !== "[]" ? `<div class="memory-detail-meta-line">Tags: ${escapeHtml(JSON.parse(item.tags || "[]").join(", "))}</div>` : ""}
            <div class="memory-detail-meta-line">Provenance: <strong>${escapeHtml(item.provenance)}</strong>${item.created_by ? ` by <strong>${escapeHtml(item.created_by)}</strong>` : ""}</div>
            ${item.source_entry_id ? `<div class="memory-detail-meta-line">Source entry: <strong>${escapeHtml(item.source_entry_id)}</strong></div>` : ""}
            <div class="memory-detail-meta-line">Updated: <strong>${escapeHtml(new Date(item.updated_at).toLocaleString())}</strong></div>

            <div class="memory-connections">
                <h4>Connections</h4>
                ${linkRows.length ? linkRows : '<p class="memory-muted">No connections yet.</p>'}
                <div class="memory-link-add">
                    <select id="memory-link-relation">
                        ${RELATIONS.map((r) => `<option value="${r}">${r.replace("-", " ")}</option>`).join("")}
                    </select>
                    <input type="text" id="memory-link-search-input" placeholder="Search item to link..." />
                    <button id="btn-memory-link-search" class="btn-ghost-sm">Search</button>
                    <div id="memory-link-results" class="mini-search-results hidden"></div>
                </div>
            </div>
        </div>
        <footer class="panel-footer">
            <button id="btn-memory-edit" class="btn-ghost">Edit</button>
            <button id="btn-memory-delete" class="btn-danger">Delete</button>
        </footer>
    `;
    document.getElementById("memory-detail").classList.remove("hidden");
    document.getElementById("btn-close-memory-detail").addEventListener("click", closeDetail);
    document.getElementById("btn-memory-edit").addEventListener("click", () => startEdit(item));
    document.getElementById("btn-memory-delete").addEventListener("click", () => deleteItem(item.id));
}

function onDetailClick(e) {
    if (e.target.id === "btn-close-memory-detail") return closeDetail();
    const removeBtn = e.target.closest("[data-link-remove]");
    if (removeBtn) return removeLink(removeBtn.dataset.linkRemove);
    const openBtn = e.target.closest("[data-link-open]");
    if (openBtn) return openDetail(openBtn.dataset.linkOpen);
    if (e.target.id === "btn-memory-link-search") return doLinkSearch();
    const pick = e.target.closest("[data-link-pick]");
    if (pick) return addLink(pick.dataset.linkTarget, pick.dataset.linkTitle);
}

async function doLinkSearch() {
    const input = document.getElementById("memory-link-search-input");
    const box = document.getElementById("memory-link-results");
    if (!input || !box) return;
    const term = input.value.trim();
    if (!term) return;
    let hits = [];
    try {
        hits = await invoke("memory_list", { kind: null, category: null, search: term });
    } catch {
        return;
    }
    hits = hits.filter((h) => h.id !== detailId);
    if (!hits.length) {
        box.innerHTML = '<div class="mini-search-empty">No matches.</div>';
    } else {
        box.innerHTML = hits.map((h) => `
            <button class="mini-search-result" data-link-pick="${escapeHtml(h.id)}" data-link-title="${escapeHtml(h.title)}">
                ${escapeHtml(h.title)} <span class="memory-cat-chip">${escapeHtml(h.category)}</span>
            </button>
        `).join("");
    }
    box.classList.remove("hidden");
}

async function addLink(targetId, targetTitle) {
    const relation = document.getElementById("memory-link-relation").value;
    try {
        await invoke("memory_link", { source_id: detailId, target_id: targetId, relation, created_by: "user" });
    } catch {
        return;
    }
    linkSearchResults = [];
    const box = document.getElementById("memory-link-results");
    if (box) box.classList.add("hidden");
    openDetail(detailId);
}

async function removeLink(linkId) {
    try {
        await invoke("memory_unlink", { link_id: linkId });
    } catch {
        return;
    }
    openDetail(detailId);
}

function startEdit(item) {
    openForm(item);
}

function openForm(item) {
    const overlay = document.getElementById("memory-form");
    document.getElementById("memory-form-title").textContent = item ? "Edit Memory Item" : "Add Memory Item";
    document.getElementById("memory-form-id").value = item ? item.id : "";
    document.getElementById("memory-form-kind").value = item ? item.kind : "information";
    document.getElementById("memory-form-title-input").value = item ? item.title : "";
    document.getElementById("memory-form-content").value = item ? item.content : "";
    document.getElementById("memory-form-category").value = item ? item.category : "preference";
    document.getElementById("memory-form-tags").value = item ? (JSON.parse(item.tags || "[]")).join(", ") : "";
    document.getElementById("memory-form-item-date").value = item && item.item_date ? item.item_date : "";
    document.getElementById("memory-form-confidence").value = item ? String(item.confidence ?? 1) : "1";
    document.getElementById("btn-memory-form-delete").classList.toggle("hidden", !item);
    overlay.classList.remove("hidden");
}

async function onFormClick(e) {
    if (e.target.id === "btn-memory-form-save") return saveForm();
    if (e.target.id === "btn-memory-form-cancel") return closeForm();
    if (e.target.id === "btn-memory-form-delete") {
        const id = document.getElementById("memory-form-id").value;
        if (id) await deleteItem(id);
    }
}

async function saveForm() {
    const id = document.getElementById("memory-form-id").value;
    const title = document.getElementById("memory-form-title-input").value.trim();
    const content = document.getElementById("memory-form-content").value.trim();
    if (!title || !content) {
        alert("Title and content are required.");
        return;
    }
    const tags = document.getElementById("memory-form-tags").value
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean);
    const rawDate = document.getElementById("memory-form-item-date").value;
    const confidence = Math.min(1, Math.max(0, Number(document.getElementById("memory-form-confidence").value) || 1));
    const payload = {
        id: id || "",
        title,
        content,
        kind: document.getElementById("memory-form-kind").value,
        category: document.getElementById("memory-form-category").value,
        tags: JSON.stringify(tags),
        item_date: rawDate || null,
        confidence,
    };
    try {
        if (id) {
            await invoke("memory_update", { input: payload });
        } else {
            await invoke("memory_create", { input: payload });
        }
    } catch {
        return;
    }
    closeForm();
    if (id) {
        detailId = id;
        openDetail(id);
    }
    await refresh();
}

async function deleteItem(id) {
    if (!confirm("Permanently delete this memory item and its connections?")) return;
    try {
        await invoke("memory_delete", { id });
    } catch {
        return;
    }
    closeDetail();
    closeForm();
    detailId = null;
    await refresh();
}

function closeDetail() {
    const panel = document.getElementById("memory-detail");
    panel.classList.add("hidden");
    document.getElementById("memory-detail-content").innerHTML = "";
    detailId = null;
}

function closeForm() {
    document.getElementById("memory-form").classList.add("hidden");
}
```

> Note: `memory_list`/`memory_get`/`memory_link` return snake_case JSON from Rust (e.g. `source_entry_id`, `created_by`); the single JS command invocations above use the exact Tauri-arg names `sourceId`/`targetId`/`createdBy`/`linkId` which match the Rust command parameters (`source_id`/`target_id`/`created_by`/`link_id` — Tauri camelCases the JS side automatically). If `memory_link` arg names mis-declare, the safe form is `{ source_id, target_id, relation, created_by }`. Verify at smoke time; keep JS keys equal to the Rust fn param names, which Tauri converts both ways (see existing calls like `task_update` using `{ ...task }`).

- [ ] **Step 4: Update `src/js/app.js`**

Replace the entire memory lazy-loader block (lines 156-169):

```js
// Load and Init Memory tab when clicked
document.querySelector('[data-tab="memory"]').addEventListener('click', async () => {
  const loaded = await loadView('memory', 'views/memory.html');
  if (loaded && !window._memoryInitialized) {
    window._memoryInitialized = true;
    const m = await import('./tabs/memory.js');
    m.initMemory();
  }
});
```

- [ ] **Step 5: Write `src/styles/memory.css`**

```css
#memory-view {
    height: 100%;
    min-height: 0;
    display: flex;
    overflow: hidden;
}

.memory-view-layout {
    display: flex;
    min-width: 0;
    min-height: 0;
    flex: 1;
}

.memory-sidebar {
    width: 220px;
    flex: 0 0 220px;
    border-right: 1px solid var(--border-color);
    padding: 16px 12px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 18px;
}

.memory-sidebar-title {
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.05rem;
    color: var(--text-muted);
    margin-bottom: 8px;
}

.memory-filter-btn {
    display: block;
    width: 100%;
    text-align: left;
    padding: 6px 10px;
    border: none;
    background: transparent;
    color: var(--text-primary);
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.88rem;
}

.memory-filter-btn:hover { background: var(--color-hover, rgba(128, 128, 128, 0.1)); }
.memory-filter-btn.active { background: var(--color-primary-soft, rgba(90, 173, 226, 0.15)); color: var(--color-primary, #5dade2); font-weight: 600; }

.memory-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
}

.memory-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    padding: 14px 20px;
    border-bottom: 1px solid var(--border-color);
    flex: 0 0 auto;
}

.memory-header h2 { margin: 0; font-size: 1.15rem; }
.memory-header-actions { display: flex; align-items: center; gap: 10px; }
.memory-count { color: var(--text-muted); font-size: 0.82rem; }

.memory-grid {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 20px;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 14px;
    align-content: start;
}

.memory-card {
    background: var(--color-surface, var(--bg-secondary));
    border: 1px solid var(--border-color);
    border-radius: var(--radius, 12px);
    padding: 16px;
    cursor: pointer;
    transition: transform 0.15s, border-color 0.15s, box-shadow 0.15s;
    display: flex;
    flex-direction: column;
    gap: 8px;
}

.memory-card:hover {
    transform: translateY(-2px);
    border-color: var(--color-primary, #5dade2);
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.18);
}

.memory-card-header { display: flex; justify-content: space-between; align-items: center; }
.memory-card-title { margin: 0; font-size: 0.98rem; line-height: 1.35; }
.memory-card-content {
    margin: 0;
    color: var(--text-muted);
    font-size: 0.84rem;
    line-height: 1.5;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
}
.memory-card-footer { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 0.75rem; }
.memory-cat-chip { text-transform: uppercase; letter-spacing: 0.03rem; font-size: 0.66rem; color: var(--text-muted); }
.memory-tags { color: var(--text-muted); }
.memory-conf { color: var(--color-primary, #5dade2); margin-left: auto; }

.memory-kind-badge {
    font-size: 0.66rem;
    text-transform: uppercase;
    letter-spacing: 0.04rem;
    padding: 2px 8px;
    border-radius: 999px;
    font-weight: 600;
}
.kind-memory { background: rgba(240, 180, 41, 0.15); color: var(--color-warn, #f0b429); }
.kind-information { background: rgba(90, 173, 226, 0.15); color: var(--color-primary, #5dade2); }
.kind-knowledge { background: rgba(72, 187, 120, 0.15); color: var(--color-success, #48bb78); }

.memory-detail-panel {
    position: fixed;
    top: 0;
    right: 420px;
    height: 100vh;
    width: 420px;
    background: var(--color-surface, var(--bg-secondary));
    border-left: 1px solid var(--border-color);
    box-shadow: -8px 0 24px rgba(0, 0, 0, 0.25);
    z-index: 40;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
}

.memory-detail-meta { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; flex-wrap: wrap; }
.memory-detail-content { font-size: 0.95rem; line-height: 1.6; white-space: pre-wrap; margin: 0 0 16px; }
.memory-detail-meta-line { font-size: 0.8rem; color: var(--text-muted); margin-bottom: 4px; }
.memory-connections { margin-top: 18px; border-top: 1px solid var(--border-color); padding-top: 14px; }
.memory-connections h4 { margin: 0 0 10px; font-size: 0.9rem; }
.memory-link-row { display: flex; align-items: center; gap: 8px; padding: 6px 0; border-bottom: 1px solid rgba(128, 128, 128, 0.12); font-size: 0.85rem; }
.memory-link-rel { color: var(--text-muted); }
.memory-link-target { background: none; border: none; color: var(--color-primary, #5dade2); cursor: pointer; text-align: left; flex: 1; }
.memory-link-remove { color: var(--text-muted); }
.memory-link-add { display: flex; gap: 8px; margin-top: 12px; align-items: center; flex-wrap: wrap; }
.memory-link-add select, .memory-link-add input { flex: 1 1 auto; }
.memory-muted { color: var(--text-muted); font-size: 0.82rem; }

.mini-search-empty { padding: 8px; color: var(--text-muted); font-size: 0.82rem; }

.memory-form-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
}
.memory-form-card {
    width: min(560px, 92vw);
    max-height: 92vh;
    background: var(--color-surface, var(--bg-secondary));
    border: 1px solid var(--border-color);
    border-radius: var(--radius, 12px);
    display: flex;
    flex-direction: column;
    overflow-y: auto;
}
```

- [ ] **Step 6: Verify frontend**

Run: `node --check src/js/tabs/memory.js` and `node --check src/js/app.js`
Expected: no syntax errors.

Run: `cargo check`
Expected: 0 errors.

- [ ] **Step 7: Commit**

```bash
git add src/styles/memory.css src/views/memory.html src/js/tabs/memory.js src/index.html src/js/app.js
git commit -m "feat: rebuild memory tab as knowledge base UI"
```

---

### Task 9: Docs + full verification + commit

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `AGENTS.md` (build status, file structure, decisions D-134..D-136)
- Modify: `docs/superpowers/specs/2026-09-16-memory-knowledge-base-design.md` (note Inbox-confirm mechanics, decision D-136 wording)

**Interfaces:**
- Consumes: every previous task.
- Produces: committed final docs.

- [ ] **Step 1: `CHANGELOG.md`** — add an `## [Unreleased]` (or dated) section describing the Memory/KB feature, listing files created/modified, and noting the `user_profile` → `knowledge_items` migration.

- [ ] **Step 2: `AGENTS.md`**:
- Under "Current Build Status", add a Sprint entry summarizing the Knowledge Base v1 build.
- File structure: swap `db/profile.rs` mention for `db/knowledge.rs`; add `src/js/tabs/memory-knowledge.js`? No — memory.js stays the filename; add `src/styles/memory.css`.
- Decisions table: append D-134 (unified items+links), D-135 (preset relations), D-136 (AI memory writes via inbox-confirmed proposals).
- Known Decisions count: "D-01 through D-133" → "D-01 through D-136" (or D-133→D-136).

- [ ] **Step 3: spec doc** — append an "Implementation recap" note: confirmation for AI memory writes uses the Review-Inbox proposal flow (existing machinery); there is no inline 300s chat YES/NO timer in the codebase; `ExportFact` JSON contract unchanged.

- [ ] **Step 4: Full verification**

```bash
cargo check
node --check src/js/tabs/memory.js src/js/app.js
```

Expected: 0 errors both.

- [ ] **Step 5: Build + live smoke**

```bash
cargo build
```

Launch via PowerShell:
```powershell
Start-Process "src-tauri\target\debug\hawkward-journals.exe"
```

Expected: app boots; log shows `[DB] Applying migration 12: knowledge_base`; Memory tab shows cards (migrated facts appear); add/edit/delete/link/unlink/search/filter work; Review-Inbox shows AI `memory` proposals after a chat tool call, and Accept creates the item.

- [ ] **Step 6: Commit**

```bash
git add CHANGELOG.md AGENTS.md
git commit -m "docs: record knowledge base v1"
```

> `docs/superpowers/` files are gitignored (`*.md` for untracked files); the spec is kept on disk only, consistent with the repo convention.