# Design: Tasks Kanban Board Redesign — In Progress / Completed Boards

**Date:** 2026-09-23
**Status:** Approved (user confirmed the presented design)
**Scope:** This spec covers (1) the Tasks tab kanban redesign and (2) the analysis-on-save bug fix (finding #1 from the live smoke test).

---

## 1. Problem

### 1a. Board redesign
The Tasks tab currently offers a 3-way view toggle (List / Board / Calendar). The Board itself is a 3-column kanban (`To Do` / `In Progress` / `Done`). The user wants a simpler, severity-driven board:

- **Board only** — the List and Calendar views are removed from the toggle (code stays dormant, not deleted — matches D-141 hide-not-delete convention).
- **Two board views** toggle: `In Progress` and `Completed`.
- Tasks start in a **To Do** column on the In Progress board, color-coded by **severity**.
- The user manually drags them to **Done** → considered done, grey/faded.
- An **Archived** drop target sends cards to the **Completed** board.

### 1b. Analysis-on-save bug
New journal entries are never AI-analyzed. The only enqueue point is the frontend `journal_analysis_queued` event, which `journal.js` emits only when an entry already has an id (`if (id)` in `triggerAutoSave`). `save_journal_entry` (lib.rs:523) never enqueues. Evidence: smoke test created 3 entries; analysis queue stayed empty; zero proposals/cards. Violates the product promise "The AI reads every journal entry automatically on save."

---

## 2. Design

### 2.1 Tasks tab — view toggle & boards

Replace the `.view-toggler` buttons (`data-view="list|board|calendar"`) with a two-button toggle:

| Toggle | Active board |
|---|---|
| **In Progress** | 3 columns: **To Do** · **Done** · **Archived** |
| **Completed** | 1 column: **Archived** |

- The toggle drives re-rendering of the single existing `#task-kanban-board` container (column set + cards depend on the active board).
- `#task-kanban-board` columns are rebuilt per board state:
  - **In Progress:** `.kanban-column[data-status="todo"]`, `[data-status="done"]`, `[data-status="archived"]` (last one acts as the Archive drop zone; empty-group styling removed so it reads as a drop target).
  - **Completed:** a single `.kanban-column[data-status="archived"]`.
- `#task-list-items` and `#task-calendar` (markup + `tasks-calendar.js`) remain in the codebase, unreachable. No deletion (D-141).

### 2.2 Card model

Every task renders as a **styled card** with:

- **Severity color accent** driven by `priority` field — left accent bar + top badge. Reuses the existing palette in `src/styles/tasks.css` (`priority-*` rules at ~L266, currently applied via border-left to `.priority-urgent/high/medium/low`):
  - `urgent` → red (`#ff4d4f`)
  - `high` → orange (`#faad14`)
  - `medium` → blue (`#1890ff`)
  - `low` → grey (`#8c8c8c`)
- Title (truncated), severity badge, due-date chip (`due_date`, local format), subtask count (`n subtasks`).
- **Click** → opens the existing Task Detail slide-in panel (`openDetail`, unchanged: edit/save/delete/subtasks/dependencies).
- Drag handle: the whole card is draggable via SortableJS (as today).

### 2.3 Status ↔ column mapping

`tasks.status` is a free-text string column — **no schema change**.

| Status | Column |
|---|---|
| `todo` | To Do (In Progress board) |
| `in_progress` | To Do (In Progress board) — treated as active work; no dedicated column |
| `done` | Done (In Progress board), grey/faded |
| `archived` | Archived — In Progress board drop zone target; Completed board column, grey/faded |
| `cancelled` | Hidden from all boards (unchanged) |

Drop behavior (via SortableJS `onEnd` → `task_update_status`):
- To Do column → `todo`
- Done column → `done`
- Archived column → `archived`
- Dropping onto the **Archived** zone on the In Progress board removes the card from In Progress (because status `archived` is filtered out there) and it appears on the Completed board.

**Un-archive (deviation from approved "drag back"):** Because the two boards are never in the DOM simultaneously, drag-back across boards is not feasible with SortableJS. Replaced with a **Restore (↩) button on each archived card** that sets status back to `todo`. Flagging this explicitly for the user's review (Section 7).

### 2.4 New-card flow

`+ Add Task` → `task_create` (status `todo`) → card lands in To Do on the In Progress board. Search box filters board cards as today.

### 2.5 Severity color styling

Card styles live in `src/styles/tasks.css` (`.kanban-card` at ~L114, `.priority-*` at ~L266). Restyle `.kanban-card` as a proper card (rounded, shadow, hover lift, severity accent bar) extending the existing `priority-*` palette. Status-based faded styles (`.is-done`, archived) added as muted/grey card variants regardless of severity accent.

---

## 3. Component changes

| File | Change |
|---|---|
| `src/index.html` | `.view-toggler` → 2 buttons (`In Progress` / `Completed`); hidden kanban column markup unchanged root panels; keep list/calendar markup |
| `src/js/tabs/tasks.js` | Replace `renderBoard`/`viewTabs` logic: `renderBoard('inprogress'|'completed')` rebuilds columns; SortableJS instances destroyed & re-created per render; `onEnd` maps column → status (todo/done/archived); archived cards get a Restore button; status filters for both boards; remove List/Calendar switch (leaf code stays, un-referenced) |
| `src/js/tabs/tasks-calendar.js` | Unchanged (dormant) |
| `src/js/tabs/tasks.js` + CSS | Card rendering + `priority-*` accent styles + grey `.is-done` / archived styles |
| `src-tauri/src/db/tasks.rs` | **No change expected** — verified `task_update_status` (lib.rs:2273) passes the raw `status: String` through to `update_task_status` with no allowlist; `archived` needs no schema/migration |
| `src-tauri/src/lib.rs` | **Bugfix #1:** enqueue new entries on save |

---

## 4. Design detail — analysis-on-save (bugfix #1)

- Extract the enqueue logic currently inline in the `journal_analysis_queued` listener (lib.rs ~2810) into a shared async helper, e.g. `async fn enqueue_analysis(app: &AppHandle, id: &str) -> Result<(), AppError>` that: pushes to `ai_state.queue` (respecting `MAX_QUEUE`, `retain` dedupe), inserts into `queued_ids`, sets status `Queued`, persists via `db::analysis_queue::enqueue`, and emits `JournalAnalysisQueued` + LogEvent (through the existing `crate::events::emit` path).
- In `save_journal_entry`: when `id.is_empty()` (brand-new entry), after the successful upsert + mem mirror update, call the helper with the new entry id.
- **Scope deliberately limited to NEW entries** so keystroke autosaves of existing entries don't create analysis storms (edits already trigger the frontend event). This directly fixes the observed dead path without behavior change for edits.
- Command signature gains `app: tauri::AppHandle` (commands already accept an opaque handle in Tauri v2).
- Test: extend `src-tauri/src/db/mock.rs` or add a focused unit test for the helper's queue behavior (dedupe + status). Full command-level coverage stays with the live smoke test.

---

## 5. Error handling

- Missed analysis enqueue must never fail the save: wrap enqueue call so errors are logged via `tracing::warn` and the save still returns `Ok(entry_id)` (analysis is best-effort).
- Drag drop errors (status update failure) → existing `ipc.js` error-toast path; board re-renders from `task_list` on refresh so a failed drop self-corrects on next `refreshTasks`.

---

## 6. Testing

- **Rust:** `cargo test` green (existing 22). New unit test(s) for enqueue-helper dedupe/status. Backend `cargo check` + `cargo fmt --check`.
- **Frontend harness:** follow `tools/js-checks/` pattern — add assertions for status→column mapping + archived filtering if a pure function is extracted (e.g., `boardColumnsFor('inprogress')`, `filterTasksForBoard(tasks, board)`).
- **Live smoke (CDP rig):** drag a card To Do→Done (grey), To Do→Archived (appears on Completed board, grey, Restore button), Restore→To Do, severity colors render per priority, card click opens detail. Create a new journal entry → verify analysis queue non-empty and a proposal appears.
- `node --check` on changed JS.

---

## 7. Open questions for user review

1. **Un-archive mechanism:** Approved design said "drag back"; because the two boards are never simultaneously in the DOM, I'm replacing it with a **Restore (↩) button** on archived cards (sets status → `todo`). OK?
2. **`in_progress` status:** has no column under the new model; it renders in the To Do column. Dropping back into To Do sets status `todo` (normalizes). OK?
3. **Done-column placement:** "Done" lives on the **In Progress** board (right of To Do). The Completed board only ever shows **Archived**. OK?