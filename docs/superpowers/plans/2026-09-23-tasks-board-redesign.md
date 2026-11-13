# Tasks Board Redesign + Analysis-on-Save Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Tasks List/Board/Calendar toggle with a two-board kanban (In Progress / Completed, severity-colored cards, done/archived grey) and make new journal entries get AI-analyzed on save.

**Architecture:** Pure status→column mapping lives in a new frontend module (`board-model.js`) so it is Node-testable (matches the `proposals.js` harness pattern). `tasks.js` re-renders the existing `#task-kanban-board` container per active board and re-binds SortableJS per render; drop targets map to statuses via `dropStatusFor`. On the backend, a shared `enqueue_analysis` helper (extracted from the existing `journal_analysis_queued` listener) is also called from `save_journal_entry` when an entry is brand-new, so every first save enters the analysis queue. No schema changes: `archived` is a new status string accepted by `task_update_status` (verified: raw string passthrough, lib.rs:2273).

**Tech Stack:** Vanilla JS (ES modules), Rust/Tauri v2 (tokio, rusqlite), SortableJS (vendored), Node harnesses under `tools/js-checks/`.

## Global Constraints

- `tasks.status` stays a free-string column — NO schema/migration change.
- Status vocabulary: `todo` / `in_progress` / `done` / `archived` / `cancelled`. `cancelled` and `todo`-mapped work hidden from the wrong boards as defined by `columnStatusForTask`.
- Reuse the existing priority palette in `src/styles/tasks.css` (~L266): urgent `#ff4d4f`, high `#faad14`, medium `#1890ff`, low `#8c8c8c` (border-left accents). Do not change these hex values.
- D-141 hide-not-delete: List + Calendar markup, `renderTasks`, `initTasksCalendar`, `tasks-calendar.js` stay in the repo, unreachable from the toggle. Do not delete them.
- Every DnD status write goes through `task_update_status` (existing command; `TaskStatus` mapping is `dropStatusFor`).
- All files under `docs/superpowers/` and `tools/` are gitignored; harness files are committed with `git add -f` (precedent: `tools/js-checks/proposals.test.mjs`).
- Gates (run each task's final check): `cargo test`, `cargo fmt --check`, `node tools/js-checks/*.test.mjs`, `node --check` on every changed JS file.
- Existing `currentFilter` sub-tab filter (`all | pending | completed`) must keep working: `pending` = not done/cancelled/archived; `completed` = done or archived.

---

### Task 1: Backend — enqueue brand-new journal entries for analysis (bug fix #1)

**Files:**
- Modify: `src-tauri/src/lib.rs` (save_journal_entry ~L523, listener ~L2810)
- Test: `src-tauri/src/db/mock.rs` (append a test)

**Interfaces:**
- Consumes: `AppState.ai_state.{queue, queued_ids, status}` (VecDeque/HashSet/HashMap from `src-tauri/src/ai/analysis.rs`), `db::analysis_queue::{enqueue, list_queued}`, `crate::events::emit`, `AppEvent::{JournalAnalysisQueued, LogEvent}`, `AnalysisStatus::Queued`, `analysis::MAX_QUEUE` (=100), `db::pool::with_conn`.
- Produces: `async fn enqueue_analysis(state: &AppState, app: &tauri::AppHandle, entry_id: &str) -> Result<(), AppError>` — later reused by the rewritten listener.

- [ ] **Step 1: Add the DB-layer dedupe guard test**

Append to `src-tauri/src/db/mock.rs`:

```rust
#[test]
fn analysis_queue_enqueue_is_deduplicated() {
    let conn = test_db();
    crate::db::analysis_queue::enqueue(&conn, "entry-a").unwrap();
    crate::db::analysis_queue::enqueue(&conn, "entry-a").unwrap();
    crate::db::analysis_queue::enqueue(&conn, "entry-b").unwrap();
    let pending = crate::db::analysis_queue::list_queued(&conn).unwrap();
    assert_eq!(pending.len(), 2, "re-enqueue must stay one row per entry");
    assert!(pending.contains(&"entry-a".to_string()));
    assert!(pending.contains(&"entry-b".to_string()));
}
```

- [ ] **Step 2: Run it to verify it passes (guards the helper's persistence contract)**

Run: `cargo test db::mock::analysis_queue_enqueue_is_deduplicated`
Expected: PASS. (This guards the existing upsert; the helper is wired to this same function.)

- [ ] **Step 3: Extract the shared `enqueue_analysis` helper**

In `src-tauri/src/lib.rs`, add this free function just above the setup section where the `journal_analysis_queued` listener lives (near L2807). It is the exact body of the current listener inline code, generalized:

```rust
async fn enqueue_analysis(
    state: &AppState,
    app: &tauri::AppHandle,
    entry_id: &str,
) -> Result<(), AppError> {
    let db = state.db()?;
    let entry_id = entry_id.to_string();
    {
        let pool = db.clone();
        let id_db = entry_id.clone();
        let _ = db::pool::with_conn(pool, move |conn| {
            db::analysis_queue::enqueue(conn, &id_db)
        })
        .await;
    }
    let mut queue = state.ai_state.queue.lock().await;
    let mut queued_ids = state.ai_state.queued_ids.lock().await;
    if queue.len() < crate::ai::analysis::MAX_QUEUE {
        queue.retain(|existing_id| existing_id != &entry_id);
        queue.push_back(entry_id.clone());
        queued_ids.insert(entry_id.clone());
    }
    state
        .ai_state
        .status
        .lock()
        .await
        .insert(entry_id.clone(), crate::ai::AnalysisStatus::Queued);
    crate::events::emit(
        app,
        crate::events::AppEvent::JournalAnalysisQueued {
            entry_id: entry_id.clone(),
        },
    );
    crate::events::emit(
        app,
        crate::events::AppEvent::LogEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: "info".to_string(),
            source: "analysis".to_string(),
            message: format!("journal entry {entry_id} queued for analysis"),
        },
    );
    Ok(())
}
```

- [ ] **Step 4: Rewrite the listener to use the helper (DRY)**

Replace the body of `app.listen_any("journal_analysis_queued", ...)` (currently lib.rs L2810-L2849) with:

```rust
app.listen_any("journal_analysis_queued", move |event| {
    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
        if let Some(id) = payload["id"].as_str() {
            let task_handle = listener_handle.clone();
            let id_clone = id.to_string();
            tauri::async_runtime::spawn(async move {
                let state = task_handle.state::<AppState>();
                let _ = enqueue_analysis(&state, &task_handle, &id_clone).await;
            });
        }
    }
});
```

- [ ] **Step 5: Enqueue from `save_journal_entry` for brand-new entries**

Change the command signature (lib.rs L523) to accept the app handle and add the call. Current signature:

```rust
async fn save_journal_entry(
    state: tauri::State<'_, AppState>,
    id: String,
    title: Option<String>,
    content: String,
) -> Result<String, AppError> {
```

Replace with:

```rust
async fn save_journal_entry(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    id: String,
    title: Option<String>,
    content: String,
) -> Result<String, AppError> {
```

Record whether this save created the entry. After `let entry_id = if id.is_empty() { ... } else { ... };` add `let is_new = id.is_empty();`.

Then, after the in-memory mirror block ends (after the `if let Ok(mem_conn_mu) = ...` block, right before the final `tracing::debug!(... "journal entry saved");` line at L581), insert:

```rust
    if is_new {
        if let Err(e) = enqueue_analysis(&state, &app, &entry_id).await {
            tracing::warn!(target: "analysis", entry_id = %entry_id, error = %e, "new-entry analysis enqueue failed");
        }
    }
```

(`&state` coerces `State<AppState>` → `&AppState` via Deref; best-effort: a failure is logged, never fails the save.)

- [ ] **Step 6: Run the gates**

Run: `cargo test`
Expected: all pass (22 existing + 1 new = 23).

Run: `cargo fmt --check`
Expected: clean. If it reports formatting drift, run `cargo fmt` then re-check.

Run: `cargo check`
Expected: 0 errors (pre-existing warnings allowed).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/db/mock.rs
git commit -m "fix(ai): enqueue new journal entries for analysis on save"
```

---

### Task 2: Frontend — `board-model.js` pure mapping module (TDD harness)

**Files:**
- Create: `src/js/tabs/board-model.js`
- Test: `tools/js-checks/board.test.mjs`
- Modify: `src/js/tabs/tasks.js` (import, used in Task 3)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `export const BOARD_LAYOUTS = { inprogress: [...], completed: [...] }` (array of `{ status, label }`)
  - `export function columnStatusForTask(status: string): string | null`
  - `export function dropStatusFor(columnStatus: string): string`

- [ ] **Step 1: Create the module**

Create `src/js/tabs/board-model.js`:

```js
// Pure status/column mapping for the two-board Tasks kanban.
export const BOARD_LAYOUTS = {
  inprogress: [
    { status: "todo", label: "To Do" },
    { status: "done", label: "Done" },
    { status: "archived", label: "Archived" },
  ],
  completed: [{ status: "archived", label: "Archived" }],
};

export function columnStatusForTask(status) {
  if (status === "done") return "done";
  if (status === "archived") return "archived";
  if (status === "cancelled") return null;
  return "todo";
}

export function dropStatusFor(columnStatus) {
  if (columnStatus === "done") return "done";
  if (columnStatus === "archived") return "archived";
  return "todo";
}
```

- [ ] **Step 2: Write the failing test**

Create `tools/js-checks/board.test.mjs`:

```js
import assert from "node:assert/strict";
import { BOARD_LAYOUTS, columnStatusForTask, dropStatusFor } from "../../src/js/tabs/board-model.js";

assert.deepEqual(
  BOARD_LAYOUTS.inprogress.map(c => c.status),
  ["todo", "done", "archived"],
  "in-progress board columns"
);
assert.deepEqual(
  BOARD_LAYOUTS.completed.map(c => c.status),
  ["archived"],
  "completed board column"
);

assert.equal(columnStatusForTask("todo"), "todo", "todo -> To Do");
assert.equal(columnStatusForTask("in_progress"), "todo", "in_progress -> To Do");
assert.equal(columnStatusForTask("done"), "done", "done -> Done");
assert.equal(columnStatusForTask("archived"), "archived", "archived -> Archived");
assert.equal(columnStatusForTask("cancelled"), null, "cancelled hidden");

assert.equal(dropStatusFor("todo"), "todo", "drop on To Do normalizes");
assert.equal(dropStatusFor("done"), "done", "drop on Done");
assert.equal(dropStatusFor("archived"), "archived", "drop on Archived");
console.log("board-model: all assertions passed");
```

- [ ] **Step 3: Run to verify it fails initially**

Run: `node tools/js-checks/board.test.mjs`
Expected: FAIL — `ERR_MODULE_NOT_FOUND` for `../../src/js/tabs/board-model.js`.

- [ ] **Step 4: Run to verify it passes**

The module now exists (Step 1). Run: `node tools/js-checks/board.test.mjs`
Expected: `board-model: all assertions passed`.

- [ ] **Step 5: Commit**

```bash
git add -f tools/js-checks/board.test.mjs src/js/tabs/board-model.js
git commit -m "test(ui): board-model status/column mapping harness"
```

---

### Task 3: Tasks board UI — two-board toggle, renderer, Sortable drag

**Files:**
- Modify: `src/index.html` (view-toggler + initial container visibility)
- Modify: `src/js/tabs/tasks.js`

**Interfaces:**
- Consumes: `BOARD_LAYOUTS`, `columnStatusForTask`, `dropStatusFor` from `./board-model.js` (Task 2).
- Produces: `renderKanban(tasks, activeBoard)`; `activeBoard` state reusing `.view-tab` buttons with `data-board="inprogress|completed"`.

- [ ] **Step 1: Replace the view toggle markup**

In `src/index.html` replace lines 111-115 (`.view-toggler` with `data-view="list|board|calendar"` buttons) with:

```html
            <div class="view-toggler">
              <button class="view-tab active" data-board="inprogress">In Progress</button>
              <button class="view-tab" data-board="completed">Completed</button>
            </div>
```

Make the board the default-visible container: in the same file, change `#task-kanban-board` div (line 130) from `class="kanban-container hidden"` to `class="kanban-container"`, and `#task-list-items` (line 124) from `class="task-items-container"` to `class="task-items-container hidden"`.

- [ ] **Step 2: Rewrite `tasks.js` imports + view state**

At the top of `src/js/tabs/tasks.js`, after the existing imports, add:

```js
import { BOARD_LAYOUTS, columnStatusForTask, dropStatusFor } from "./board-model.js";
```

Replace the state variable + tab references (lines 10-15):

```js
  const boardTabs = document.querySelectorAll(".view-tab");
  const listContainer = document.getElementById("task-list-items");
  const boardContainer = document.getElementById("task-kanban-board");
  const calendarContainer = document.getElementById("task-calendar");

  let activeBoard = "inprogress";
```

(Keep `detailPanel`/`detail*` declarations and `window.loadTask` at lines 18-37 unchanged. Keep `if (!listEl) return;` as the module guard — `listEl` is still declared at line 6.)

- [ ] **Step 3: Rewrite `refreshTasks` to always render the active board**

Replace the `refreshTasks` body (lines 44-76) with:

```js
  async function refreshTasks(query = "") {
    try {
      let tasks = query.trim()
        ? await invoke("task_search", { query })
        : await invoke("task_list");

      // Client-side filtering for sub-tabs
      if (currentFilter === "pending") {
        tasks = tasks.filter(t =>
          t.status !== 'done' && t.status !== 'cancelled' && t.status !== 'archived');
      } else if (currentFilter === "completed") {
        tasks = tasks.filter(t => t.status === 'done' || t.status === 'archived');
      }

      renderKanban(tasks, activeBoard);
      listContainer.classList.add("hidden");
      boardContainer.classList.remove("hidden");
      calendarContainer.classList.add("hidden");
    } catch (err) {
      console.error("Failed to load tasks:", err);
    }
  }
```

- [ ] **Step 4: Replace `renderKanban` with the board renderer**

Replace the `renderKanban` function (lines 78-105) with:

```js
  function renderKanban(tasks, board) {
    const layout = BOARD_LAYOUTS[board] || BOARD_LAYOUTS.inprogress;

    boardContainer.innerHTML = layout.map(col => `
      <div class="kanban-column" data-status="${col.status}">
        <div class="column-header">${col.label} <span class="column-count">0</span></div>
        <div class="column-items" data-status="${col.status}"></div>
      </div>
    `).join("");
    boardContainer.classList.toggle("boards-single", layout.length === 1);

    tasks.forEach(task => {
      const colStatus = columnStatusForTask(task.status);
      if (!colStatus) return; // cancelled (or unknown) hidden

      const col = boardContainer.querySelector(`.kanban-column[data-status="${colStatus}"] .column-items`);
      if (!col) return;

      const card = document.createElement("div");
      const stateClass = task.status === 'done' ? ' is-done'
        : task.status === 'archived' ? ' is-archived' : '';
      card.className = `kanban-card priority-${task.priority}${stateClass}`;
      card.draggable = true;
      card.dataset.id = task.id;

      const subtasks = tasks.filter(t => t.parent_task_id === task.id).length;
      card.innerHTML = `
        <div class="card-top">
          <span class="severity-badge ${escapeHtml(task.priority)}">${escapeHtml(task.priority)}</span>
          ${task.is_blocked ? '<span class="card-flag" title="Blocked">\u{1F512}</span>' : ""}
          ${task.parent_task_id ? '<span class="subtag">subtask</span>' : ""}
        </div>
        <div class="card-title">${escapeHtml(task.title)}</div>
        <div class="card-meta">
          ${task.due_date ? `<span class="due-chip">${escapeHtml(task.due_date)}</span>` : ""}
          ${subtasks > 0 ? `<span>${subtasks} subtask${subtasks > 1 ? "s" : ""}</span>` : ""}
        </div>
        ${task.status === 'archived' ? '<button class="restore-btn" type="button" title="Restore to To Do">\u{21A9} Restore</button>' : ""}
      `;

      card.addEventListener("click", (e) => {
        if (e.target.closest(".restore-btn")) return;
        openDetail(task.id);
      });

      if (task.status === 'archived') {
        card.querySelector(".restore-btn").addEventListener("click", async () => {
          try {
            await invoke("task_update_status", { id: task.id, status: "todo" });
            refreshTasks(searchInput.value);
          } catch (err) {
            console.error("Restore failed:", err);
          }
        });
      }

      col.appendChild(card);
    });

    boardContainer.querySelectorAll(".kanban-column").forEach(col => {
      col.querySelector(".column-count").textContent =
        col.querySelectorAll(".kanban-card").length;
    });

    // SortableJS — re-created on every render (columns are rebuilt)
    if (typeof Sortable !== "undefined") {
      boardContainer.querySelectorAll(".column-items").forEach((col) => {
        new Sortable(col, {
          group: { name: "board", pull: true, put: true },
          animation: 150,
          onEnd: async (evt) => {
            const newColumn = evt.to?.dataset.status;
            const oldColumn = evt.from?.dataset.status;
            if (!newColumn || newColumn === oldColumn) return;
            const newStatus = dropStatusFor(newColumn);
            try {
              await invoke("task_update_status", { id: evt.item.dataset.id, status: newStatus });
              refreshTasks(searchInput.value);
            } catch (err) {
              console.error("Status update failed:", err);
            }
          },
        });
      });
    }
  }
```

(Inside the `tasks.forEach` loop, `colStatus` mapping comes from `columnStatusForTask` so `in_progress` and `todo` land in To Do, `cancelled` is skipped before the query runs.)

- [ ] **Step 5: Replace the view-switching block + Sortable block**

Delete the old `// --- VIEW SWITCHING ---` block (lines 400-408) and replace with:

```js
  // --- BOARD SWITCHING ---
  boardTabs.forEach(tab => {
    tab.addEventListener("click", () => {
      boardTabs.forEach(t => t.classList.remove("active"));
      tab.classList.add("active");
      activeBoard = tab.dataset.board;
      refreshTasks(searchInput.value);
    });
  });
```

Delete the old `// --- BOARD DRAG-AND-DROP (SortableJS, once) ---` block (lines 410-426) entirely — binding now happens inside `renderKanban`.

- [ ] **Step 6: Syntax check + run harnesses**

Run: `node --check src/js/tabs/tasks.js` and `node --check src/js/tabs/board-model.js` and `node tools/js-checks/board.test.mjs`
Expected: `node --check` exits 0 (no printed output on success); harness prints `board-model: all assertions passed`.

- [ ] **Step 7: Commit**

```bash
git add src/index.html src/js/tabs/tasks.js
git commit -m "feat(tasks): two-board kanban (In Progress/Completed) with severity cards"
```

---

### Task 4: Card styling — severity accents + done/archived grey states

**Files:**
- Modify: `src/styles/tasks.css`

**Interfaces:**
- Consumes: the `priority-{urgent,high,medium,low}` classes (exist at ~L266) and new classes emitted by `renderKanban` (`kanban-card`, `is-done`, `is-archived`, `card-top`, `severity-badge`, `subtag`, `due-chip`, `restore-btn`, `kanban-column`, `column-count`, `boards-single`).

- [ ] **Step 1: Restyle the board + cards**

Append to `src/styles/tasks.css`:

```css
/* ── TWO-BOARD KANBAN (In Progress / Completed) ── */
.kanban-container.boards-single {
  grid-template-columns: minmax(320px, 460px);
  justify-content: start;
}

.kanban-card {
  border-radius: 12px;
  padding: 12px 14px;
  cursor: grab;
  position: relative;
  transition: all 0.2s;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-left-width: 5px;                       /* severity accent bar */
  box-shadow: 0 2px 6px rgba(0,0,0,0.08);
}

.kanban-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 6px 16px rgba(0,0,0,0.14);
  border-color: var(--color-accent);
}

.kanban-card .card-top {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 8px;
}

.kanban-card .severity-badge {
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  padding: 2px 8px;
  border-radius: 99px;
}

.kanban-card .severity-badge.urgent { background: #ff4d4f; color: #fff; }
.kanban-card .severity-badge.high   { background: #faad14; color: #222; }
.kanban-card .severity-badge.medium { background: #1890ff; color: #fff; }
.kanban-card .severity-badge.low    { background: #8c8c8c; color: #fff; }

.kanban-card .card-flag { margin-left: auto; font-size: 12px; }
.kanban-card .subtag {
  font-size: 9px;
  color: var(--color-muted);
  border: 1px solid var(--color-border);
  padding: 1px 6px;
  border-radius: 99px;
}

.kanban-card .card-title {
  font-size: 13px;
  font-weight: 600;
  line-height: 1.35;
}

.kanban-card .card-meta {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-top: 8px;
  font-size: 10px;
  color: var(--color-muted);
}

.kanban-card .due-chip {
  background: var(--color-bg);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  padding: 1px 6px;
}

/* Done / archived cards are grey & faded */
.kanban-card.is-done,
.kanban-card.is-archived {
  background: var(--color-bg);
  opacity: 0.65;
  border-color: var(--color-border);
  border-left-color: #8c8c8c;
}

.kanban-card.is-done .card-title,
.kanban-card.is-archived .card-title { text-decoration: line-through; color: var(--color-muted); }

/* Archived cards always carry the defeat accent; keep their own priority bar via priority-* */
.kanban-card.is-archived { border-left-width: 5px; }

.kanban-card .restore-btn {
  margin-top: 10px;
  width: 100%;
  font-size: 11px;
  padding: 5px 10px;
  border-radius: 8px;
  border: 1px solid var(--color-border);
  background: var(--color-bg);
  color: var(--color-text);
  cursor: pointer;
}
.kanban-card .restore-btn:hover { border-color: var(--color-accent); color: var(--color-accent); }

.kanban-column[data-status="archived"] .column-items {
  outline: 1px dashed var(--color-border);
  outline-offset: -4px;
  border-radius: 10px;
  min-height: 60px;
}

.column-count {
  font-weight: 600;
  color: var(--color-muted);
  font-size: 11px;
  margin-left: 6px;
}
```

(Do not change the existing `priority-*` rules at L266-269 — the left-border accent widths are now inherited from `.kanban-card`.)

- [ ] **Step 2: Quick sanity — check CSS vars exist**

Run: `node --check` is inapplicable to CSS; instead grep the CSS variables used exist:

Run: `rg -n "color-surface|color-bg|color-muted|color-border" src/styles/base.css`
Expected: all four tokens are defined in `base.css`.

- [ ] **Step 3: Commit**

```bash
git add src/styles/tasks.css
git commit -m "style(tasks): severity-colored kanban cards with grey done/archived states"
```

---

### Task 5: End-to-end verification — gates + live smoke

**Files:**
- Modify: none (verification only)

- [ ] **Step 1: Run the full gate suite**

Run: `cargo test`
Expected: all pass (incl. the new `analysis_queue_enqueue_is_deduplicated`).

Run: `cargo fmt --check` (clean), `cargo check` (0 errors), then

Run: `node tools/js-checks/chat-format.test.mjs && node tools/js-checks/mood.test.mjs && node tools/js-checks/proposals.test.mjs && node tools/js-checks/board.test.mjs && node --check src/js/tabs/tasks.js && node --check src/js/tabs/board-model.js`
Expected: all harnesses print their pass line.

- [ ] **Step 2: Analyze-on-save live smoke (CDP rig)**

With the app running under `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"` and both engines healthy (poll `http://127.0.0.1:8080/health`, `:8081/health` until 200):

1. Via CDP evaluate: create a fresh journal entry with actionable content (e.g. title `SMOKE ANALYSIS`, content `"Schedule the dentist for Thursday and refill the car oil."`), wait for autosave.
2. Poll `window.__TAURI_INTERNALS__.invoke("app_diagnostics", {})` every 5s for up to 2 min.
3. Expected: `analysis_queue_len` reaches ≥1 or `analysis_statuses` shows a non-zero bucket, and a smoke task appears in `task_list` (AI-proposed) or a pending row in `proposed_list` if it is awaiting confirmation.
4. Assert the debug log (`%APPDATA%/ArnoldJournals/arnold-debug.log`) contains `queued for analysis` for the new entry id.

- [ ] **Step 3: Board drag live smoke**

1. Confirm the Tasks tab shows the **In Progress** board default with columns `To Do` / `Done` / `Archived` and the toggle buttons `In Progress` / `Completed`.
2. Create a high-priority task → card lands in To Do with an orange accent + `high` badge; `is-done`/`is-archived` classes absent.
3. Drag the card To Do → Done: card gets `is-done` (grey, strike-through) and stays on the board.
4. Drag a second card To Do → Archived: it disappears from In Progress; toggle to **Completed** shows it grey with a **↩ Restore** button. Click Restore → card returns to To Do on the In Progress board.
5. Column counts (`column-count`) match visible cards in every column.
6. Click a card → the Task Detail slide-in opens; Save/Delete still work.

- [ ] **Step 4: Clean up smoke artifacts**

Via CDP `invoke`: `journal_delete` the smoke entry, `task_delete` the smoke tasks (both boards), verify `task_list` is back to the pre-smoke set.

- [ ] **Step 5: Rebuild + record**

Run: `cargo tauri build --bundles msi nsis` (optional until a committed pass, then rebuild releases).

Update `CHANGELOG.md` (`### Added` / `### Fixed` entries) and `AGENTS.md` (status block, file structure for `board-model.js`, decisions if any), commit:

```bash
git add CHANGELOG.md AGENTS.md
git commit -m "docs: record two-board kanban + analysis-on-save fixes"
```

---

## Self-Review Notes

- **Spec coverage:** 2.1 (toggle+boards) → Task 3; 2.2 (cards/colors/click) → Tasks 3+4; 2.3 (status↔column) → Task 2; 2.4 (new-card flow) → Task 3; 2.5 (styling) → Task 4; Section 4 (analysis fix) → Task 1; Section 6 (testing) → Tasks 2/5; Section 7 open questions resolved as approved (Restore button instead of cross-board drag; `in_progress` → To Do with normalization; Done on In Progress board).
- One intentional deviation flagged to the user pre-plan: un-archive via **Restore button**, not drag-back (two boards are never simultaneously in DOM).
- Severity palette hex values copied verbatim from existing CSS; no `priority-*` rule edits, only additive card styling.