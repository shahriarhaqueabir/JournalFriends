# Design: In Progress Column · Analysis Clarity · Dedicated System Tab

**Date:** 2026-10-08
**Status:** Approved (user confirmed the presented design, sections 1–5)
**Scope:** Four items from the 2026-10-08 session: (1) an In Progress column on the Tasks board, (2) the `[object Object]` action-items bug + unclear/undiscoverable journal analysis status, (3) the stale Runtime & Logs grid, (4) a new dedicated System tab with AI/DB interactive controls. Approach A (surgical) — no migrations, reuse existing commands/renderers.

---

## 1. Problem

### 1a. No In Progress column
The board's default toggle ("In Progress" board) renders `To Do | Done | Archived` (`src/js/tabs/board-model.js:2-9`). The `in_progress` status exists end-to-end in the backend — DB CHECK (`001_initial.sql:46`), `update_task_status` pass-through (`db/tasks.rs:169-183`), Task Detail select (`src/index.html:183`), AI tool enums (`ai/tools.rs:570,599`), reports (`db/reports.rs:247-250`) — but the frontend never renders it: `columnStatusForTask` falls every non-done/non-archived task through to `todo` (`board-model.js:14-20`). A task set to `in_progress` is indistinguishable from `todo` on the board, and no drag target can ever produce the status.

### 1b. `[object Object]` action items + no analysis proposals surface
Commit `f5bb6c8` made `RawAnalysis.tasks` structured (`{title, due_date, priority}` objects, `ai/mod.rs:31,57-79`). Three frontend breakages followed:
- `ai-sidebar.js:377` renders `escapeHtml(t)` where `t` is now an object → `String(obj)` → **`[object Object]`**.
- `ai-sidebar.js:378` encodes the object into `data-task` → `%5Bobject%20Object%5D`; the accept handler (`:433-451`) never `decodeURIComponent`s it, so accepting creates a task literally titled `%5Bobject%20Object%5D`.
- `renderAnalysisResult` (`:334-419`) silently drops `result.events` and `result.notes` entirely.

Critically, the journal-analysis worker **never emits `AiProposalsReady`** — only `ai_chat` does (`lib.rs:1015`), so the correct escape-first `proposalCardHtml` inline cards never appear after journal analysis. The broken Action-Items/Facts blocks were the only sidebar accept surface for analysis results.

### 1c. Analysis status is invisible and confusing
- Trigger is a keystroke: `journal.js:247-265` emits `journal_analysis_queued` immediately (throttled, D-106) and debounces the save 1500 ms. Nothing in the UI says this.
- The worker's `Processing` transition emits **no event**: `AppEvent::JournalAnalysisProcessing` is declared `#[allow(dead_code)]` (`events.rs:18-21`) and never constructed. The user jumps from "Waiting in line…" to a result with no intermediate signal.
- Status renderers replace `messagesEl.innerHTML` wholesale (`ai-sidebar.js:296-332`) — every state change **wipes the chat history**.
- All analysis cases are gated on `currentEntryId.value === payload.entry_id` (`:196,202,208,214`) — an analysis for any other entry leaves the sidebar stuck on "Waiting in line…" forever.
- `#btn-analyze` (`src/index.html:90`) is dead HTML — no handler anywhere.

### 1d. Runtime & Logs shows 8 of 12 cells permanently `—`
`AppDiagnostics` (`lib.rs:121-136`) is `#[derive(Serialize)]` with **no rename** → wire keys are snake_case (`data_dir`, `db_path`, `db_size_bytes`, `log_path`, `journal_entries`, `memory_facts`, `analysis_queue_len`, `last_health_check`, `latency_ms`). The frontend reads camelCase (`settings-diagnostics.js:283-304`): only `tasks`, `conversations`, and `engine.*` match by coincidence. Health check therefore always renders "never checked". "Open log file" (`:349-363`) is broken for the same reason (`d.logPath`). Two populated fields in the payload (`analysis_statuses`, `models`) are never displayed.

The user wants this content out of Settings and in its own tab with AI + DB interactive controls.

### 1e. Naming-convention audit (asked during spec review)
An audit of every IPC surface (command args, returns, event payloads) against Tauri v2's actual behavior — one exact key per arg, default camelCase, no case fallback (`tauri-macros-2.6.1/src/command/wrapper.rs:506`, `tauri-2.11.1/src/ipc/command.rs:120`) — found three misalignment sites:

| Site | Bug | Symptom |
|---|---|---|
| `AppDiagnostics` reads (1d above) | frontend reads camelCase, backend serializes snake_case | 8/12 runtime cells permanently `—` |
| `src/js/tabs/memory.js:238` — `invoke("memory_link", { source_id, target_id, relation, created_by })` | top-level keys must be camelCase; all three are **required** params | hard error `missing required key sourceId` on every link attempt — memory linking is broken (masked since the Memory tab is hidden, D-141) |
| `src/js/tabs/memory.js:249` — `invoke("memory_unlink", { link_id })` | same; required `linkId` | hard error on every unlink |

Everything else is aligned: nested struct payloads (`task:{due_date…}`, `event:{start_at…}`, `input:{item_date…}`) correctly follow serde field names (snake); top-level camel args elsewhere (`includeCompleted`, `taskId`, `conversationId`, `parentTaskId`, `sourceEntryId`) are correct; event payloads are snake end-to-end; the three camelCase opt-in structs (`VaultStatus`, `EngineHttpRecord`, `LogTailResponse`) are read camelCase correctly.

---

## 2. Design

### 2.1 Tasks board — In Progress column

- `board-model.js`:
  - `BOARD_LAYOUTS.inprogress` = `[{todo:"To Do"}, {in_progress:"In Progress"}, {done:"Done"}, {archived:"Archived"}]`. Completed board unchanged (`[archived]`).
  - `columnStatusForTask`: archived flag wins → `status==="in_progress"` → `"in_progress"`; `done` → `done`; `cancelled` → `null`; else `todo`.
  - `dropStatusFor("in_progress")` → `"in_progress"` (new explicit branch; today the default returns `todo`).
- Drag into/out of the column calls the existing `task_update_status` (`tasks.js:148`); backend passes `in_progress` through and clears `archived`/`completed_at` (`tasks.rs:174-178` else-branch). **Zero backend change.**
- Cards in the column get class `is-in-progress` (subtle accent, consistent with existing `is-done`/`is-archived` styling, `tasks.js:86-88`).
- Replace the stale pre-seeded placeholder columns (`src/index.html:129-142`) with an empty `#task-kanban-board` container — `renderKanban` generates all columns; until `refreshTasks` resolves the user currently sees a misleading placeholder.
- `tools/js-checks/board.test.mjs`: update to 4-column layout, `in_progress → "in_progress"` mapping, `dropStatusFor` round-trip, archived-flag precedence.

### 2.2 Analysis result — unified rendering (fixes `[object Object]`)

- **Backend (one emission):** after the worker persists `proposed_inbox` rows (`lib.rs:2796-2827`), emit `AppEvent::AiProposalsReady { conversation_id: entry_id, count }` where `count` = number of rows just persisted (tasks + facts + events + notes). `conversation_id` is unused by the frontend handler (`ai-sidebar.js:268-292`) — documented as "entry id when emitted from analysis".
- **Frontend — delete** the bespoke renderers and handlers in `ai-sidebar.js`: Action Items block (`:370-384`), task-add handler (`:433-451`), Proposed Facts block (`:397-417`), fact accept/reject handlers (`:454-490`).
- **Analysis card becomes read-only presentation.** Keep Key Summary (with view-source), Emotions, Deeper Insights. Add an **"Extracted"** section rendering all four proposal types from `result` in structured form:
  - tasks → chip with **title · due date · priority** (fields read from the object, never `String(obj)`)
  - events → title + local time range; notes → title + content snippet; facts → statement
  - All via escape-first HTML built by a new pure module `src/js/analysis-view.js` exporting `extractedItemsHtml(result)` (testable in a node harness).
- **All accept/dismiss actions** flow through the existing `proposalCardHtml` cards (`proposals.js`), now triggered for journal analysis by the 2.2 emission. One renderer, escape-first, correct `proposed_action` pipeline (D-19 preserved), all four item types.
- Result card **appends** to the chat instead of replacing `messagesEl.innerHTML`.

### 2.3 Analysis status strip

- New fixed strip element at the top of the AI sidebar header (above the chat), rendered/updated by `ai-sidebar.js`:
  - **States:** `Idle → Queued → Analyzing… → Done | Error`, with entry title and an **elapsed-time ticker** started at each transition.
  - **Persistent hint** (Idle state): *"Auto-analyzed as you type — no save needed."*
- **Backend, two touches:**
  1. Worker emits `JournalAnalysisProcessing { entry_id }` right after `mark_processing` (`lib.rs:~2701`) and drops the `#[allow(dead_code)]` annotation (`events.rs:18-21`). The existing frontend case (`ai-sidebar.js:201-205`) starts working — retargeted at the strip.
  2. `analysis_requeue` command (see 2.4) powers the strip's Error-state **Retry** button (re-enqueues the failed entry; worker dedupes).
- **Strip is global** — updates for any entry id (not gated on `currentEntryId`). The result card keeps the existing "only when the entry is open" gating; if a different entry completed, the strip shows *"Done — open the entry to view."*
- Queued/Processing renderers no longer touch `messagesEl`; chat history is never wiped by status changes. Error state moves to the strip (keeps the retry affordance; the current "Retry Connection" logic is superseded by Retry → re-enqueue).

### 2.4 Dedicated System tab

**Placement:** `System` nav item (lucide `server`) in the sidebar's System group above Settings; container `#tab-system`; module `src/js/tabs/system.js`; wired via the existing generic router (`app.js:33-61`) + `loadView` lazy-cache pattern. Amends the lean nav (D-141): 5 visible tabs (Journal · Tasks · Mood · System · Settings).

**Contents (top → bottom):**

| Card | Data source | Controls |
|---|---|---|
| **Runtime grid** (moved from Settings `index.html:607-642`) | `app_diagnostics` — reads fixed to snake_case: `data_dir`, `db_path`, `db_size_bytes`, `log_path`, `journal_entries`, `memory_facts`, `analysis_queue_len`, `last_health_check`/`latency_ms`; additionally display `analysis_statuses` + `models` | 3 s auto-refresh gated on System tab active (same pattern as `settings-diagnostics.js:19-22`) |
| **Engines** | `get_engine_status`, `check_engine_health` | Start (`provision_model_cmd`), Stop (`stop_engine`), Restart (`restart_engine`), Health check now — per service (chat/embed), 10 s auto health refresh while tab open |
| **Analysis queue** | new `analysis_queue_list` | Retry failed → new `analysis_requeue`; manual Refresh |
| **DB** | diagnostics + new `backup_now` | Backup Now (returns produced path), Open data dir via `window.__TAURI__.shell.open(data_dir)` (same mechanism as the Open-log button) |
| **SQL runner** | new `sql_run` | textarea + Run → column/rows grid; **read-only** (see D-147); requires unlocked vault |
| **Log viewer** (moved from Settings) | `log_tail` | level filter + Open log file (fixed to `d.log_path`) |

**Settings after the move:** LLM Engine · System Check · Vault & Security · Activity Log · Development & Debugging. The Runtime & Logs section is deleted; `settings-diagnostics.js` retains Activity Log only (runtime + log-tail code relocates to `system.js`). The tab-active gate in `app.js:52-59` gains a `system` branch mirroring `settings`.

**Backend — 4 new commands** (registered in `generate_handler`):

1. `backup_now(state) -> Result<String, AppError>` — wraps existing `backup::manual::create_backup` (Backup API, D-98); returns the produced file path.
2. `analysis_queue_list(state) -> Result<Vec<QueueRow>, AppError>` — reads all rows from `analysis_queue` (`entry_id`, `status`, `queued_at`, `updated_at`, `attempts` — schema `009_analysis_queue.sql`); new `list_all` in `db/analysis_queue.rs`.
3. `analysis_requeue(state, entry_id: String) -> Result<(), AppError>` — new `reset_failed` in `db/analysis_queue.rs` (failed → queued), then call the existing `enqueue_analysis` path so the in-memory push + `JournalAnalysisQueued` emission happen exactly as for a fresh entry.
4. `sql_run(state, query: String) -> Result<SqlRunResult, AppError>` — `SqlRunResult { columns: Vec<String>, rows: Vec<Vec<serde_json::Value>>, changes: u64 }`. Guard: trimmed first keyword (after optional leading `--`/`/*` comments) must be one of `select | with | pragma | explain`, else `AppError::InvalidInput`. Runs on the keyed pool (vault must be unlocked). No write path (D-147).

### 2.5 Naming conventions (fixes 1e + locks the contract)

- **Fix** `memory.js:238` → `invoke("memory_link", { sourceId, targetId, relation, createdBy: "user" })` and `memory.js:249` → `invoke("memory_unlink", { linkId })` (Tauri converts to the Rust snake params automatically).
- **D-148 wire contract** (documented in AGENTS.md):
  - Top-level command arguments: **camelCase** (Tauri default — do not add `rename_all` to commands).
  - Nested struct payloads: serde struct field names (**snake_case**, declared fields / `rename_all = "snake_case"`).
  - Command returns + event payloads: **snake_case**, except structs that explicitly opt into `#[serde(rename_all = "camelCase")]` (must be read camelCase).
- **Static guard:** new harness `tools/js-checks/ipc-conventions.test.mjs` scans `src/js` invoke call sites with a small brace-depth walker and fails if any **top-level** payload key is snake_case (`foo_bar:` at depth 1) — permanently catching the `memory_link`-class bug. Nested struct keys are exempt (depth ≥ 2).

### 2.6 Proposed decision IDs (to be added to AGENTS.md Known Decisions)

| ID | Decision | Why |
|---|---|---|
| **D-144** | `in_progress` is a real column on the default board; two-board toggle retained | Status already exists in the backend; only rendering was missing |
| **D-145** | Journal-analysis proposals surface through the same `proposalCardHtml` cards as chat (analysis worker emits `AiProposalsReady`); the analysis card is read-only | One escape-first action surface instead of two divergent ones — the divergent copy produced the `[object Object]` bug |
| **D-146** | Dedicated System tab (5th nav item); Runtime & Logs + log viewer move; System Check and Activity Log stay in Settings for now | User request; keeps the change surgical |
| **D-147** | `sql_run` accepts read-only statements only (`SELECT/WITH/PRAGMA/EXPLAIN`) | All writes must flow through typed commands with D-19 confirmation and the D-33 write choke points; a generic write path would bypass both |
| **D-148** | Wire contract: top-level command args = camelCase (Tauri default, no `rename_all`); nested struct payloads + returns + events = snake_case unless a struct explicitly opts into camelCase | Tauri does exact single-key lookup with no case fallback — mixed casing caused the 8 dead diagnostics cells and two broken memory commands; contract now guarded by `AppDiagnostics` serde test + `ipc-conventions.test.mjs` |

---

## 3. Testing & Verification

**Frontend harnesses (node):**
- `board.test.mjs` — updated: 4-column layout, `in_progress` mapping, drop round-trip, archived precedence.
- New `analysis-view.test.mjs` — `extractedItemsHtml`: object task → title/due/priority rendered (regression: never `[object Object]`), escape-first assertions (HTML in title is escaped), legacy string tasks still render, events/notes/facts included.
- New `ipc-conventions.test.mjs` — fails on any top-level snake_case key in an `invoke` payload (D-148 guard; locks the `memory_link` fix).
- Existing `mood/proposals/chat-format` harnesses stay green; `node --check` on all touched JS.

**Rust tests (cargo test):**
- `sql_run` guard: accepts `SELECT`/`WITH`/`PRAGMA`/`EXPLAIN` (incl. leading comments), rejects `INSERT/UPDATE/DELETE/DROP/ALTER`.
- `analysis_queue_list` + `reset_failed` round-trip over the mock DB (009 schema).
- Serde contract: serialize `AppDiagnostics`, assert `data_dir`/`db_path`/`journal_entries`… keys present and camelCase equivalents absent — locks the wire format so the dead-fields bug cannot silently return.

**Gates:** `cargo test` (35 existing + new), `cargo check` (0 errors, 6 known warnings), `cargo fmt --check`, all node harnesses.

**Live E2E (rebuild release → relaunch):**
1. Drag a card into **In Progress** → persists after reload; Restore/Done drags still work.
2. Keystroke in journal → strip shows `Queued → Analyzing… (elapsed) → Done`; chat history intact; analysis card appends with structured Extracted chips (no `[object Object]`); proposal cards appear and accept correctly (task title correct, due/priority preserved).
3. **System tab**: all 12 runtime fields populate; `analysis_statuses`/`models` visible; Health check now updates latency; Backup Now produces a file; SQL runner returns rows and rejects a write; log tail renders.
4. Carry-over from the fix pass (same session): accept an **event proposal** and a **note proposal** (migration 016 / no FOREIGN KEY error), **Task Detail** open+save (no `missing 'archived'`), **Run System Check** embed-role result, controlled `:8081` squatter → explicit squatter message, no 501 spam in the log.

---

## 4. Out of Scope

- Reviving the dead List view (`renderTasks`, `tasks.js:159-226`) — stays dormant per D-141.
- Moving Activity Log / System Check into the System tab (candidate for a later pass).
- SQL writes through the runner; DB maintenance actions (VACUUM/checkpoint/integrity).
- Chat/analysis latency itself (local LLM inference time — the strip only makes the wait visible).
- Model management (GGUF import) relocation — stays in Settings' LLM Engine card.
