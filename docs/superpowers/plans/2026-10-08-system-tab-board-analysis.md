# System Tab · In-Progress Column · Analysis Clarity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the approved 2026-10-08 spec (`docs/superpowers/specs/2026-10-08-system-tab-board-analysis-design.md`): a real **In Progress** column on the Tasks board, fix `[object Object]` action items + make journal-analysis status visible (status strip + unified proposal cards), move Runtime & Logs into a dedicated **System** tab with AI/DB interactive controls, and lock the IPC naming contract (D-148) with a static guard.

**Architecture:** Approach A (surgical) — no migrations, no new dependencies, no CDN (CSP `script-src 'self'`). Pure frontend logic lands in Node-testable modules (`board-model.js` extended; new `analysis-view.js`; new harness `ipc-conventions.test.mjs`). Backend gains 4 read-mostly commands (`backup_now`, `analysis_queue_list`, `analysis_requeue`, `sql_run`) plus two event emissions (worker `JournalAnalysisProcessing`, worker `AiProposalsReady`). The System tab follows the existing lazy-view pattern (`loadView` + dynamic `import`, memory-tab style); its runtime/log card reuses the `rt-*` markup moved out of Settings, reading the existing snake_case `AppDiagnostics` wire keys. The analysis strip lives at the top of the always-mounted right sidebar (D-23) and is updated for **any** entry id; the read-only analysis card appends into chat and all accept/dismiss actions flow through the existing escape-first `proposalCardHtml` pipeline (D-19 preserved).

**Tech Stack:** Vanilla JS (ES modules), Rust/Tauri v2 (`rusqlite 0.31` bundled-sqlcipher, tokio), SortableJS (vendored), Node harnesses under `tools/js-checks/` using `node:assert/strict`.

## Global Constraints

- **D-148 wire contract:** top-level command arguments = camelCase (never add `rename_all` to `#[tauri::command]`); nested struct payloads, command returns, and event payloads = snake_case unless a struct explicitly opts into `#[serde(rename_all = "camelCase")]` (then read camelCase). Tauri v2 does one exact key per arg — no case fallback.
- **Never set `card.draggable = true`** (`src/js/tabs/tasks.js:89`); SortableJS runs with `forceFallback: true` because WebView2 native drag never fires.
- **Escape-first:** every model/user string rendered via `innerHTML` goes through `escapeHtml` from `src/js/sanitize.js`. Strip/status labels are set via `textContent` (no escaping needed).
- **AI writes only through confirmation:** no new AI→DB path; `sql_run` is read-only (D-147); `analysis_requeue` only re-enqueues an existing failed row (no data mutation beyond queue state, no AI decision).
- **D-97/D-98 conventions untouched:** no migrations in this plan; backups keep using the `rusqlite::backup::Backup` API via `run_auto_backup`.
- **`.gitignore` traps:** `*.md` (line 114) and `tools/` are ignored — commit docs and harnesses with `git add -f` (precedent: `tools/js-checks/board.test.mjs`).
- **Gates after every task:** `cargo fmt --check`, `cargo check` (0 errors; the 6 known warnings allowed), `cargo test` (35 existing + new), `node --check` on every touched JS file, run all `tools/js-checks/*.test.mjs` harnesses.
- **CI parity** (`.github/workflows/quality.yml`): `cargo fmt --check`, `cargo check`, recursive `node --check` over `src\js`.
- `rg` is unavailable on this machine — use the grep tool or Python for scanning.

## File Structure

```
docs/superpowers/plans/2026-10-08-system-tab-board-analysis.md   this plan
src/js/tabs/board-model.js                    MOD — 4-column layout, in_progress mapping
src/js/tabs/tasks.js                          MOD — is-in-progress card class
src/js/tabs/memory.js                         MOD — camelCase invoke args (D-148 fix)
src/js/tabs/settings-diagnostics.js           MOD — trimmed to Activity Log only
src/js/tabs/system.js                         NEW — System tab module
src/js/analysis-view.js                       NEW — extractedItemsHtml() pure renderer
src/js/ai-sidebar.js                          MOD — status strip + append-only analysis card
src/js/tabs/journal.js                        MOD — #btn-analyze wiring
src/js/app.js                                 MOD — system tab router branch
src/index.html                                MOD — nav item, #tab-system, strip markup,
                                                    placeholder columns emptied, Runtime & Logs removed
src/styles/tasks.css                          MOD — 4-col grid fit + is-in-progress accent
src/styles/ai-chat.css                        MOD — .analysis-strip styles
src/views/system.html                         NEW — System tab page (runtime/engines/queue/db/sql/log)
src-tauri/src/lib.rs                          MOD — 2 emits, 4 commands, wire_tests mod
src-tauri/src/events.rs                       MOD — drop #[allow(dead_code)] on Processing
src-tauri/src/db/analysis_queue.rs            MOD — QueueRow, list_all, reset_failed + tests
tools/js-checks/board.test.mjs                MOD — 4-column assertions
tools/js-checks/ipc-conventions.test.mjs      NEW — D-148 static guard
tools/js-checks/analysis-view.test.mjs        NEW — [object Object] regression harness
CHANGELOG.md / AGENTS.md                      MOD — record + D-144..D-148
```

---

### Task 1: Tasks board — In Progress column (spec 2.1)

**Files:**
- Modify: `src/js/tabs/board-model.js`
- Modify: `tools/js-checks/board.test.mjs`
- Modify: `src/js/tabs/tasks.js` (card class only, ~L86-88)
- Modify: `src/index.html` (placeholder, L129-142)
- Modify: `src/styles/tasks.css` (L76-92, append after L594)

**Interfaces:**
- Consumes: existing `task_update_status` (backend passes `in_progress` through, `db/tasks.rs:169-183` — zero backend change).
- Produces: `BOARD_LAYOUTS.inprogress` gains `{ status: "in_progress", label: "In Progress" }`; `columnStatusForTask` maps `"in_progress" → "in_progress"`; `dropStatusFor("in_progress") → "in_progress"`; card class `is-in-progress`.

- [ ] **Step 1: Update the test first (TDD) — expects FAIL**

Replace in `tools/js-checks/board.test.mjs` the layout assertion (L4-8) with:

```js
assert.deepEqual(
  BOARD_LAYOUTS.inprogress.map(c => c.status),
  ["todo", "in_progress", "done", "archived"],
  "in-progress board columns"
);
```

Replace L16 (`columnStatusForTask("in_progress")` assertion) with:

```js
assert.equal(columnStatusForTask("in_progress"), "in_progress", "in_progress -> In Progress");
assert.equal(columnStatusForTask({ status: "in_progress", archived: false }), "in_progress", "in_progress object -> In Progress");
```

Append before the final `console.log`:

```js
// Round-trip: drop on each column reproduces the status that maps back to it.
for (const col of ["todo", "in_progress", "done", "archived"]) {
  assert.equal(columnStatusForTask(dropStatusFor(col)), col, `round-trip ${col}`);
}
```

Run: `node tools/js-checks/board.test.mjs` → **Expected FAIL** (layout still 3 columns; `in_progress` still maps to `todo`).

- [ ] **Step 2: Update `board-model.js`**

Replace the file content with:

```js
// Pure status/column mapping for the two-board Tasks kanban.
export const BOARD_LAYOUTS = {
  inprogress: [
    { status: "todo", label: "To Do" },
    { status: "in_progress", label: "In Progress" },
    { status: "done", label: "Done" },
    { status: "archived", label: "Archived" },
  ],
  completed: [{ status: "archived", label: "Archived" }],
};

// Maps a task (object; also accepts a bare status string) to its board column.
// The `archived` flag wins over status so archived tasks (status stays 'done')
// land in the Archived column and disappear from Done.
export function columnStatusForTask(task) {
  const t = typeof task === "string" ? { status: task } : task;
  if (t.archived) return "archived";
  if (t.status === "done") return "done";
  if (t.status === "in_progress") return "in_progress";
  if (t.status === "cancelled") return null;
  return "todo";
}

export function dropStatusFor(columnStatus) {
  if (columnStatus === "done") return "done";
  if (columnStatus === "archived") return "archived";
  if (columnStatus === "in_progress") return "in_progress";
  return "todo";
}
```

Run: `node tools/js-checks/board.test.mjs` → **Expected PASS**: `board-model: all assertions passed`.

- [ ] **Step 3: Card class in `tasks.js`**

Replace L86-88:

```js
      const stateClass = task.archived ? ' is-archived'
        : task.status === 'done' ? ' is-done'
        : task.status === 'in_progress' ? ' is-in-progress' : '';
      card.className = `kanban-card priority-${task.priority}${stateClass}`;
```

- [ ] **Step 4: Empty the stale placeholder in `index.html`**

Replace L129-142 (the three pre-seeded `.kanban-column` divs — they show a misleading static board until `refreshTasks` resolves) with:

```html
        <div id="task-kanban-board" class="kanban-container"></div>
```

(`renderKanban` builds every column from `BOARD_LAYOUTS`; `refreshTasks` clears `boardContainer.innerHTML` on each render.)

- [ ] **Step 5: CSS — 4-column fit + in-progress accent**

In `src/styles/tasks.css`:
1. L78: change `grid-template-columns: 1fr 1fr 1fr;` → `grid-template-columns: repeat(4, minmax(0, 1fr));`
2. L91: change `min-width: 280px;` → `min-width: 0;` on `.kanban-column` (3 columns at `min-width: 280px` already overflowed the ~760px main pane at 1400×900; `minmax(0,1fr)` + `min-width: 0` lets all four fit and scroll-content shrink; `.boards-single` override at L501 is unaffected).
3. Append after L594 (`/* Archived cards always carry ... */ .kanban-card.is-archived { border-left-width: 5px; }`) — same specificity (0,2,0) but later than the `.kanban-card.priority-*` rules at L518-521, so the state accent wins, consistent with `is-done`/`is-archived`:

```css
.kanban-card.is-in-progress { border-left-color: var(--color-accent); }
```

- [ ] **Step 6: Gates**

Run: `node --check src/js/tabs/board-model.js && node --check src/js/tabs/tasks.js && node tools/js-checks/board.test.mjs`
Expected: `node --check` silent exit 0; harness prints `board-model: all assertions passed`.

- [ ] **Step 7: Commit**

```bash
git add src/js/tabs/board-model.js src/js/tabs/tasks.js src/styles/tasks.css src/index.html
git add -f tools/js-checks/board.test.mjs
git commit -m "feat(tasks): real In Progress column on the default board"
```

---

### Task 2: D-148 wire-contract guard + memory_link/unlink fix (spec 1e, 2.5)

**Files:**
- Create: `tools/js-checks/ipc-conventions.test.mjs`
- Modify: `src/js/tabs/memory.js` (L238, L249)

**Interfaces:**
- Consumes: Tauri v2 top-level arg convention (camelCase keys → snake_case Rust params automatically).
- Produces: scanner that fails on any **top-level** snake_case key in an `invoke` payload; locks the memory fix permanently.

- [ ] **Step 1: Create the failing guard**

Create `tools/js-checks/ipc-conventions.test.mjs` (faithful JS port of the validated scanner a local scanner script — brace/string-aware, nested keys exempt):

```js
// D-148 guard: fails on any top-level snake_case key in an invoke() payload.
// Top-level command args must be camelCase (Tauri does exact single-key lookup,
// no case fallback). Nested struct keys (depth >= 2) are exempt — they follow
// serde field names (snake_case).
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../src/js");
const snake = /^[a-z][a-z0-9]*(_[a-z0-9]+)+$/;

function scan(text) {
  const out = [];
  const call = /\binvoke\s*\(\s*/g;
  let m;
  while ((m = call.exec(text)) !== null) {
    let i = m.index + m[0].length;
    if (i >= text.length) continue;
    const q = text[i];
    if (q !== '"' && q !== "'" && q !== "`") continue;
    i += 1;
    while (i < text.length && text[i] !== q) i += 1;
    i += 1;
    while (i < text.length && /\s/.test(text[i])) i += 1;
    if (text[i] !== ",") continue;
    i += 1;
    while (i < text.length && /\s/.test(text[i])) i += 1;
    if (text[i] !== "{") continue;

    // Extract the balanced payload object { i .. j }.
    let depth = 0;
    let instr = null;
    let j = i;
    for (; j < text.length; j++) {
      const c = text[j];
      if (instr) {
        if (c === "\\") { j += 1; continue; }
        if (c === instr) instr = null;
      } else if (c === '"' || c === "'" || c === "`") {
        instr = c;
      } else if (c === "{" || c === "[" || c === "(") {
        depth += 1;
      } else if (c === "}" || c === "]" || c === ")") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    const obj = text.slice(i + 1, j);
    const invokeLine = text.slice(0, m.index).split("\n").length;

    // Walk top-level entries only (d2 tracks nesting inside the payload).
    let d2 = 0;
    let s = null;
    let token = "";
    for (let k = 0; k < obj.length; k++) {
      const ch = obj[k];
      if (s) {
        token += ch;
        if (ch === "\\") { token += obj[k + 1] ?? ""; k += 1; continue; }
        if (ch === s) s = null;
        continue;
      }
      if (ch === '"' || ch === "'" || ch === "`") { s = ch; token += ch; continue; }
      if (ch === "{" || ch === "[" || ch === "(") { d2 += 1; token += ch; continue; }
      if (ch === "}" || ch === "]" || ch === ")") { d2 -= 1; token += ch; continue; }
      if (d2 > 0) { token += ch; continue; }
      if (ch === ",") {
        const key = token.trim().replace(/^["'`]|["'`]$/g, "");
        if (snake.test(key)) out.push({ key, line: invokeLine });
        token = "";
        continue;
      }
      if (ch === ":") {
        const key = token.trim().replace(/^["'`]|["'`]$/g, "");
        if (snake.test(key)) out.push({ key, line: invokeLine });
        token = "";
        // Skip the value until the next top-level comma.
        let vd = 0;
        let iv = null;
        k += 1;
        for (; k < obj.length; k++) {
          const c2 = obj[k];
          if (iv) {
            if (c2 === "\\") { k += 1; continue; }
            if (c2 === iv) iv = null;
          } else if (c2 === '"' || c2 === "'" || c2 === "`") {
            iv = c2;
          } else if (c2 === "{" || c2 === "[" || c2 === "(") {
            vd += 1;
          } else if (c2 === "}" || c2 === "]" || c2 === ")") {
            vd -= 1;
          } else if (c2 === "," && vd === 0) {
            break;
          }
        }
        token = "";
        continue;
      }
      token += ch;
    }
  }
  return out;
}

const files = [];
(function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.name.endsWith(".js")) files.push(p);
  }
})(root);

const violations = [];
let siteCount = 0;
for (const f of files.sort()) {
  const text = fs.readFileSync(f, "utf8");
  // Count payload-bearing invoke sites (command + object literal) for the
  // vacuous-pass guard below.
  siteCount += (text.match(/\binvoke\s*\(\s*["'`][^"'`]+["'`]\s*,\s*\{/g) || []).length;
  for (const v of scan(text)) {
    violations.push(`${path.relative(process.cwd(), f).replace(/\\/g, "/")}:${v.line}: ${v.key}`);
  }
}

assert.ok(
  siteCount >= 50,
  `vacuous guard: only ${siteCount} payload-bearing invoke sites found (expected >= 50)`,
);
assert.deepEqual(
  violations,
  [],
  `top-level snake_case invoke keys found (fix to camelCase):\n${violations.join("\n")}`,
);
console.log(`ipc-conventions: all assertions passed (${siteCount} invoke sites scanned)`);
```

Run: `node tools/js-checks/ipc-conventions.test.mjs` → **Expected FAIL** with exactly:

```
src/js/tabs/memory.js:238: source_id
src/js/tabs/memory.js:238: target_id
src/js/tabs/memory.js:238: created_by
src/js/tabs/memory.js:249: link_id
```

- [ ] **Step 2: Fix `memory.js` (the bug the guard caught)**

`memory_link` (lib.rs:1377) takes `source_id, target_id, relation, created_by`; `memory_unlink` (lib.rs:1418) takes `link_id`. Tauri maps camelCase JS keys to these automatically.

Replace L238:

```js
        await invoke("memory_link", { sourceId: detailId, targetId, relation, createdBy: "user" });
```

Replace L249:

```js
        await invoke("memory_unlink", { linkId });
```

- [ ] **Step 3: Verify green**

Run: `node tools/js-checks/ipc-conventions.test.mjs`
Expected: `ipc-conventions: all assertions passed (N invoke sites scanned)` with N ≥ 50 (105 at time of writing).

- [ ] **Step 4: Gates**

Run: `node --check src/js/tabs/memory.js` → silent exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/js/tabs/memory.js
git add -f tools/js-checks/ipc-conventions.test.mjs
git commit -m "fix(memory): camelCase invoke args + D-148 IPC conventions guard"
```

---

### Task 3: Backend emissions — Processing event + AiProposalsReady from analysis + serde wire test (spec 2.2, 2.3)

**Files:**
- Modify: `src-tauri/src/events.rs` (L18-21)
- Modify: `src-tauri/src/lib.rs` (worker ~L2711, persist block ~L2784-2828, append `mod wire_tests` at file end)

**Interfaces:**
- Consumes: `AppEvent::{JournalAnalysisProcessing, AiProposalsReady}` (already declared), `worker_handle` (in scope inside the worker spawn), `AnalysisResult { tasks, facts, events, notes }`.
- Produces: live `journal_analysis_processing` events (payload `{entry_id}`); `ai_proposals_ready` with `conversation_id = entry_id`, `count = rows persisted`, emitted only when the tx succeeded and count > 0.

- [ ] **Step 1: Drop the dead-code annotation**

In `events.rs` remove the `#[allow(dead_code)]` on L18-21:

```rust
    JournalAnalysisProcessing {
        entry_id: String,
    },
```

(The variant is about to be constructed; `StartupComplete` etc. keep their annotations.)

- [ ] **Step 2: Emit `JournalAnalysisProcessing` after `mark_processing`**

In `lib.rs` the worker's success branch (L2711-2713) currently only inserts the in-memory status. Replace:

```rust
                                } else {
                                    state.ai_state.status.lock().await.insert(id.clone(), crate::ai::AnalysisStatus::Processing);
                                }
```

with:

```rust
                                } else {
                                    state.ai_state.status.lock().await.insert(id.clone(), crate::ai::AnalysisStatus::Processing);
                                    crate::events::emit(
                                        &worker_handle,
                                        crate::events::AppEvent::JournalAnalysisProcessing {
                                            entry_id: id.clone(),
                                        },
                                    );
                                }
```

(`worker_handle` is the spawn's captured clone; later emits at L2839/L2856 already use `&worker_handle` the same way.)

- [ ] **Step 3: Emit `AiProposalsReady` after the persist tx**

In the `Ok(txt)` analysis branch, at the persist block. Current head (L2784-2787):

```rust
                                        // Persist proposals — same structure
                                        let conv_id = Uuid::new_v4().to_string();
                                        let created_at = chrono::Utc::now().to_rfc3339();
                                        {
```

Change to compute count + tx outcome first:

```rust
                                        // Persist proposals — same structure
                                        let conv_id = Uuid::new_v4().to_string();
                                        let created_at = chrono::Utc::now().to_rfc3339();
                                        let proposed_count = (result.tasks.len()
                                            + result.facts.len()
                                            + result.events.len()
                                            + result.notes.len()) as u32;
                                        let mut tx_ok = false;
                                        {
```

Then replace the tail of the block — currently:

```rust
                                                Ok::<(), AppError>(())
                                            }).await.ok();
                                        }
```

with:

```rust
                                                Ok::<(), AppError>(())
                                            })
                                            .await
                                            .is_ok();
                                        }

                                        if tx_ok && proposed_count > 0 {
                                            crate::events::emit(
                                                &worker_handle,
                                                crate::events::AppEvent::AiProposalsReady {
                                                    // Conversation id is unused by the frontend handler
                                                    // (ai-sidebar ai_proposals_ready) — from the analysis
                                                    // worker it carries the journal entry id.
                                                    conversation_id: id.clone(),
                                                    count: proposed_count,
                                                },
                                            );
                                        }
```

(`db::pool::with_tx(...)` returns `Result<R, AppError>`; the closure returns `Ok::<(), AppError>(())`, so `.await.is_ok()` gives `bool`. The inner `let id = id.clone();` is block-scoped — the outer `id` is what we emit. `result` is only moved at L2839 `JournalAnalysisCompleted`, after this emit.)

- [ ] **Step 4: TDD — add the serde wire-contract test (expects FAIL first? no — documents current behavior)**

Append at the very end of `lib.rs` (after `pub fn run()`):

```rust
#[cfg(test)]
mod wire_tests {
    use super::*;

    #[test]
    fn app_diagnostics_serializes_snake_case_wire_keys() {
        let d = AppDiagnostics {
            generated_at: "2026-10-08T00:00:00Z".to_string(),
            data_dir: "/data".to_string(),
            db_path: "/data/arnold.db".to_string(),
            log_path: "/data/arnold-debug.log".to_string(),
            db_size_bytes: 1,
            journal_entries: 1,
            tasks: 1,
            memory_facts: 1,
            conversations: 1,
            analysis_queue_len: 1,
            analysis_statuses: AnalysisStatusCounts::default(),
            engine: EngineDiagSnapshot::default(),
            models: Vec::new(),
        };
        let v = serde_json::to_value(&d).expect("AppDiagnostics must serialize");
        for key in [
            "generated_at", "data_dir", "db_path", "log_path", "db_size_bytes",
            "journal_entries", "tasks", "memory_facts", "conversations",
            "analysis_queue_len", "analysis_statuses", "engine", "models",
        ] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        for key in [
            "dataDir", "dbPath", "logPath", "dbSizeBytes", "journalEntries",
            "memoryFacts", "analysisQueueLen",
        ] {
            assert!(v.get(key).is_none(), "camelCase leak on wire: {key}");
        }
        // Nested engine/status shapes the System tab reads (snake_case too).
        assert!(v["engine"].get("last_health_check").is_some());
        assert!(v["engine"]["chat"].get("latency_ms").is_some());
        assert!(v["engine"]["embed"].get("pid").is_some());
        assert!(v["analysis_statuses"].get("queued").is_some());
        assert!(v["analysis_statuses"].get("failed").is_some());
    }
}
```

- [ ] **Step 5: Gates**

Run: `cargo fmt` (once, to format the edits) then `cargo fmt --check` → clean.
Run: `cargo check` → 0 errors (6 known warnings allowed; no NEW warnings — if `JournalAnalysisProcessing` now triggers a different dead-code note anywhere, it should be gone, not added).
Run: `cargo test` → all pass (35 existing + `wire_tests::app_diagnostics_serializes_snake_case_wire_keys` = 36).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/events.rs src-tauri/src/lib.rs
git commit -m "feat(analysis): emit processing + proposals-ready events from the journal worker"
```

### Task 4: Backend commands — backup_now · analysis_queue_list · analysis_requeue · sql_run (spec 2.4, 2.6/D-147)

**Files:**
- Modify: `src-tauri/src/db/analysis_queue.rs` (add `QueueRow`, `list_all`, `reset_failed`, tests)
- Modify: `src-tauri/src/lib.rs` (4 commands after `mood_delete` @ L2506; `generate_handler` after `mood_delete,` @ L2956; test module at file end)

**Interfaces:**
- Consumes: `AppState::{db, key, paths}` (lib.rs:54-68), `backup::auto::run_auto_backup` (auto.rs:18), `db::pool::with_conn` (pool.rs:74), `enqueue_analysis` (lib.rs:2508), `AppError::{InvalidInput, NotFound}` (error.rs:8,7).
- Produces (all registered in `generate_handler`; JS side passes **camelCase** top-level args per D-148):

```rust
#[tauri::command] async fn backup_now(state) -> Result<String, AppError>            // no args
#[tauri::command] async fn analysis_queue_list(state) -> Result<Vec<QueueRow>, AppError> // no args
#[tauri::command] async fn analysis_requeue(state, app, entry_id: String) -> Result<(), AppError> // JS: { entryId }
#[tauri::command] async fn sql_run(state, query: String) -> Result<SqlRunResult, AppError>        // JS: { query }
```

- `QueueRow { entry_id, status, queued_at, updated_at, attempts }` — snake_case wire (D-148), serialized by `serde`.
- `SqlRunResult { columns: Vec<String>, rows: Vec<Vec<serde_json::Value>>, changes: u64 }` — `changes` is structurally always `0`: every statement the guard permits is read-only (see Step 4 note).

- [ ] **Step 1: `db/analysis_queue.rs` — `QueueRow` + `list_all` + `reset_failed`**

Append after `mark_failed` (L71):

```rust
/// One row of `analysis_queue` as shown in the System tab (snake_case wire, D-148).
#[derive(serde::Serialize)]
pub struct QueueRow {
    pub entry_id: String,
    pub status: String,
    pub queued_at: String,
    pub updated_at: String,
    pub attempts: i64,
}

/// All queue rows: failures first (serves the Retry-failed control), then newest first.
pub fn list_all(conn: &Connection) -> Result<Vec<QueueRow>, AppError> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT entry_id, status, queued_at, updated_at, attempts
         FROM analysis_queue
         ORDER BY CASE status WHEN 'failed' THEN 0 ELSE 1 END, queued_at DESC, entry_id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(QueueRow {
            entry_id: row.get(0)?,
            status: row.get(1)?,
            queued_at: row.get(2)?,
            updated_at: row.get(3)?,
            attempts: row.get(4)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Flip failed → queued. Returns `false` when the row is not failed — the
/// caller maps that to `AppError::NotFound` (spec 2.4, item 3).
pub fn reset_failed(conn: &Connection, entry_id: &str) -> Result<bool, AppError> {
    ensure_table(conn)?;
    let now = Utc::now().to_rfc3339();
    let n = conn.execute(
        "UPDATE analysis_queue SET status = 'queued', updated_at = ?2
         WHERE entry_id = ?1 AND status = 'failed'",
        params![entry_id, now],
    )?;
    Ok(n > 0)
}
```

- [ ] **Step 2: Round-trip tests for the queue helpers**

Append at the bottom of `db/analysis_queue.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_pending;

    fn test_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_pending(&mut conn).unwrap();
        conn
    }

    #[test]
    fn list_all_orders_failures_first_then_newest() {
        let conn = test_db();
        enqueue(&conn, "e1").unwrap();
        enqueue(&conn, "e2").unwrap();
        mark_processing(&conn, "e1").unwrap();
        mark_done(&conn, "e1").unwrap();
        mark_failed(&conn, "e2").unwrap();
        // Pin timestamps so the DESC ordering is deterministic.
        conn.execute(
            "UPDATE analysis_queue SET queued_at = '2026-01-03T00:00:00Z' WHERE entry_id = 'e1'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE analysis_queue SET queued_at = '2026-01-02T00:00:00Z' WHERE entry_id = 'e2'",
            [],
        )
        .unwrap();

        let rows = list_all(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].entry_id, "e2"); // failed floats to the top
        assert_eq!(rows[0].status, "failed");
        assert_eq!(rows[1].entry_id, "e1");
        assert_eq!(rows[1].status, "done");
        assert_eq!(rows[1].attempts, 1); // mark_processing bumped it
    }

    #[test]
    fn reset_failed_requeues_only_failed_rows() {
        let conn = test_db();
        enqueue(&conn, "e1").unwrap();
        enqueue(&conn, "e2").unwrap();
        mark_done(&conn, "e1").unwrap();
        mark_failed(&conn, "e2").unwrap();

        assert!(reset_failed(&conn, "e2").unwrap());
        assert_eq!(list_queued(&conn).unwrap(), vec!["e2".to_string()]);
        assert!(!reset_failed(&conn, "e1").unwrap()); // done rows are not resettable
        assert!(!reset_failed(&conn, "nope").unwrap()); // unknown ids neither
    }
}
```

Run: `cargo test analysis_queue` → **Expected PASS** (2 tests).

- [ ] **Step 3: Four commands in `lib.rs` (after `mood_delete`, L2506)**

Insert:

```rust
// ── System tab commands (spec 2.4) ───────────────────────────────

/// Backup Now — same Backup API path as the auto-backup worker (D-98).
/// Returns the produced file path (lossy display form for the UI).
#[tauri::command]
async fn backup_now(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    let pool = state.db()?;
    let backup_dir = state.paths.backup_dir.clone();
    // T-02: same `x'<hex>'` key derivation as spawn_auto_backup_worker
    // (backup/auto.rs:64-69) so the backup file is encrypted like the source.
    let password = state.key().ok().map(|k| {
        format!(
            "x'{}'",
            k.iter().map(|b| format!("{:02x}", b)).collect::<String>()
        )
    });
    let path = crate::backup::auto::run_auto_backup(pool, backup_dir, password).await?;
    Ok(path.to_string_lossy().into_owned())
}

/// System tab — Analysis queue card.
#[tauri::command]
async fn analysis_queue_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::db::analysis_queue::QueueRow>, AppError> {
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| crate::db::analysis_queue::list_all(conn)).await
}

/// System tab / strip Retry — re-enqueue a failed analysis. `reset_failed`
/// gates on status='failed' (NotFound otherwise); `enqueue_analysis` then does
/// the in-memory push + `JournalAnalysisQueued` emission exactly as a fresh save.
#[tauri::command]
async fn analysis_requeue(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    entry_id: String,
) -> Result<(), AppError> {
    let db = state.db()?;
    let id = entry_id.clone();
    let reset = db::pool::with_conn(db, move |conn| {
        crate::db::analysis_queue::reset_failed(conn, &id)
    })
    .await?;
    if !reset {
        return Err(AppError::NotFound(format!(
            "no failed analysis for entry {entry_id}"
        )));
    }
    enqueue_analysis(&state, &app, &entry_id).await
}
```

- [ ] **Step 4: `sql_run` + the D-147 guard (same insertion point)**

```rust
/// Wire shape for the System tab SQL runner. `changes` is structurally always
/// 0: the guard only permits read-only statements (D-147), and `conn.changes()`
/// would report the pooled connection's *previous* write — a lie.
#[derive(Serialize)]
struct SqlRunResult {
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
    changes: u64,
}

/// D-147 gate: the first keyword (after any leading `--`/`/* */` comments)
/// must be one of select | with | pragma | explain.
fn ensure_read_only(query: &str) -> Result<(), AppError> {
    let mut s = query.trim_start();
    loop {
        if let Some(rest) = s.strip_prefix("--") {
            s = match rest.find('\n') {
                Some(i) => rest[i + 1..].trim_start(),
                None => "",
            };
        } else if let Some(rest) = s.strip_prefix("/*") {
            match rest.find("*/") {
                Some(i) => s = rest[i + 2..].trim_start(),
                None => {
                    return Err(AppError::InvalidInput(
                        "unterminated block comment".into(),
                    ))
                }
            }
        } else {
            break;
        }
    }
    let first = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match first.as_str() {
        "select" | "with" | "pragma" | "explain" => Ok(()),
        "" => Err(AppError::InvalidInput("empty query".into())),
        other => Err(AppError::InvalidInput(format!(
            "only read-only statements are allowed (SELECT/WITH/PRAGMA/EXPLAIN), got `{other}`"
        ))),
    }
}

fn sql_value_to_json(row: &rusqlite::Row<'_>, i: usize) -> Result<serde_json::Value, AppError> {
    let v: rusqlite::types::Value = row.get(i)?;
    Ok(match v {
        rusqlite::types::Value::Null => serde_json::Value::Null,
        rusqlite::types::Value::Integer(n) => serde_json::json!(n),
        rusqlite::types::Value::Real(f) => serde_json::json!(f), // NaN/Inf → Null (serde_json), no panic
        rusqlite::types::Value::Text(t) => serde_json::Value::String(t),
        rusqlite::types::Value::Blob(b) => {
            serde_json::Value::String(format!("<blob {} bytes>", b.len()))
        }
    })
}

/// System tab SQL runner — read-only statements only (D-147), 500-row cap.
#[tauri::command]
async fn sql_run(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<SqlRunResult, AppError> {
    ensure_read_only(&query)?;
    let db = state.db()?;
    db::pool::with_conn(db, move |conn| {
        // Backstop: even a keyword-allowlisted statement must report read-only
        // (blocks `PRAGMA journal_mode = DELETE`, `EXPLAIN INSERT`, etc.).
        let mut stmt = conn.prepare(&query)?;
        if !stmt.readonly() {
            return Err(AppError::InvalidInput(
                "statement rejected by the read-only backstop".into(),
            ));
        }
        let columns: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|c| c.to_string())
            .collect();
        let col_count = columns.len();
        let mut rows_stmt = stmt.query([])?;
        let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
        const MAX_ROWS: usize = 500;
        while let Some(row) = rows_stmt.next()? {
            if rows.len() >= MAX_ROWS {
                break;
            }
            let mut vals = Vec::with_capacity(col_count);
            for i in 0..col_count {
                vals.push(sql_value_to_json(row, i)?);
            }
            rows.push(vals);
        }
        Ok(SqlRunResult {
            columns,
            rows,
            changes: 0,
        })
    })
    .await
}
```

> **Probe-verified behavior** (rusqlite 0.31 / bundled SQLite, measured before writing this plan): `SELECT`, `WITH … SELECT`, `EXPLAIN SELECT`, `PRAGMA table_info/foreign_key_list/database_list` → `readonly() == true` (pass both layers); `INSERT/UPDATE/DELETE/DROP/VACUUM`, `PRAGMA journal_mode = DELETE`, **and `EXPLAIN INSERT`** → `readonly() == false` (backstop rejects). Known limitation: bare `PRAGMA journal_mode` (a read) also reports `false` in SQLite and is rejected — acceptable; introspection pragmas pass.

- [ ] **Step 5: Register in `generate_handler`**

After `mood_delete,` (L2956) add:

```rust
            backup_now,
            analysis_queue_list,
            analysis_requeue,
            sql_run,
```

- [ ] **Step 6: Guard tests in `lib.rs` (append after `mod wire_tests` from Task 3)**

```rust
#[cfg(test)]
mod sql_run_tests {
    use super::{ensure_read_only, AppError};

    #[test]
    fn ensure_read_only_accepts_read_verbs() {
        for ok in [
            "SELECT 1",
            "  select 1",
            " SELECT 1  ",
            "-- leading line comment\nSELECT 1",
            "-- a\n   -- b\nSELECT 1",
            "/* block */ SELECT 1",
            "/* a */ /* b */\nSELECT * FROM tasks",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "PRAGMA table_info(tasks)",
            "EXPLAIN SELECT 1",
            " \n\t SELECT 1",
        ] {
            assert!(ensure_read_only(ok).is_ok(), "expected accept: {ok:?}");
        }
    }

    #[test]
    fn ensure_read_only_rejects_writes() {
        for bad in [
            "",
            "   ",
            "INSERT INTO tasks (id) VALUES ('x')",
            "insert into tasks (id) VALUES ('x')",
            "UPDATE tasks SET title = 'x'",
            "DELETE FROM tasks",
            "DROP TABLE tasks",
            "ALTER TABLE tasks ADD COLUMN x TEXT",
            "CREATE TABLE evil(a)",
            "REPLACE INTO tasks VALUES ('x')",
            "VACUUM",
            "ATTACH DATABASE 'x.db' AS x",
            "BEGIN",
            "/* unterminated",
            " \n\t INSERT INTO tasks VALUES (1)",
        ] {
            let err = ensure_read_only(bad).expect_err(&format!("expected reject: {bad:?}"));
            assert!(matches!(err, AppError::InvalidInput(_)), "wrong variant: {err}");
        }
    }

    #[test]
    fn stmt_readonly_backstop_rejects_writes() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t(a TEXT); INSERT INTO t VALUES ('x');")
            .unwrap();
        // Probe-verified behavior (see Task 4 Step 4 note).
        assert!(conn.prepare("SELECT * FROM t").unwrap().readonly());
        assert!(conn
            .prepare("WITH x AS (SELECT 1) SELECT * FROM x")
            .unwrap()
            .readonly());
        assert!(conn.prepare("PRAGMA table_info(t)").unwrap().readonly());
        assert!(conn.prepare("EXPLAIN SELECT * FROM t").unwrap().readonly());
        assert!(!conn.prepare("INSERT INTO t VALUES ('y')").unwrap().readonly());
        assert!(!conn.prepare("UPDATE t SET a = 'z'").unwrap().readonly());
        assert!(!conn.prepare("DELETE FROM t").unwrap().readonly());
        assert!(!conn.prepare("DROP TABLE t").unwrap().readonly());
        assert!(!conn.prepare("VACUUM").unwrap().readonly());
        assert!(!conn
            .prepare("EXPLAIN INSERT INTO t VALUES ('y')")
            .unwrap()
            .readonly());
        assert!(!conn
            .prepare("PRAGMA journal_mode = DELETE")
            .unwrap()
            .readonly());
    }
}
```

- [ ] **Step 7: Gates**

Run: `cargo fmt` (once, to format the edits) then `cargo fmt --check` → clean.
Run: `cargo check` → 0 errors (6 known warnings allowed; no NEW warnings).
Run: `cargo test` → all pass: 35 existing + `wire_tests` (Task 3) + 2 queue tests + 3 `sql_run_tests` = **41**.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/db/analysis_queue.rs
git commit -m "feat(system): backup_now, analysis_queue_list, analysis_requeue, sql_run (read-only D-147)"
```

### Task 5: `src/js/analysis-view.js` — pure Extracted-section renderer + harness (spec 2.2, 2.6/D-145)

**Files:**
- Create: `src/js/analysis-view.js`
- Create: `tools/js-checks/analysis-view.test.mjs`

**Interfaces:**
- Consumes: `escapeHtml` from `src/js/sanitize.js` (pure string fn, no DOM — importable from node; same pattern as `mood.test.mjs` importing `src/js/tabs/mood.js`).
- Produces: `extractedItemsHtml(result) -> string` — HTML for the four proposal groups; `""` when nothing extractable (caller omits the section). Input is the `AnalysisResult` wire shape (snake_case, `ai/mod.rs:155-167`): `tasks: [{title, due_date, priority}]`, `events: [{title, start_at, location}]`, `notes: [{title, content, tags}]`, `facts: [{fact_key, content, category}]` — plus legacy plain-string tasks (tolerated, `de_analysis_tasks` keeps the frontend defensive).
- Pure: no DOM, no globals — node-harness testable. No buttons: the card is read-only (D-145); accept/dismiss flows through `proposalCardHtml` cards.

- [ ] **Step 1: Harness first — expects FAIL (module does not exist)**

Create `tools/js-checks/analysis-view.test.mjs`:

```js
import assert from "node:assert/strict";
import { extractedItemsHtml } from "../../src/js/analysis-view.js";

const has = (html, sub) =>
  assert.ok(html.includes(sub), `expected ${JSON.stringify(html)} to contain ${JSON.stringify(sub)}`);
const notHas = (html, sub) =>
  assert.ok(!html.includes(sub), `expected ${JSON.stringify(html)} NOT to contain ${JSON.stringify(sub)}`);

// 1. Structured tasks (post-f5bb6c8 wire shape) — never [object Object].
const all = extractedItemsHtml({
  id: "e1",
  tasks: [{ title: "Fix <b>bug</b>", due_date: "2026-10-10", priority: "high" }],
  events: [{ title: "Dentist", start_at: "2026-10-10T14:00:00Z", location: "Clinic" }],
  notes: [{ title: "", content: "n".repeat(400), tags: [] }],
  facts: [{ fact_key: "k", content: "Likes <tea>", category: "preference" }],
});
notHas(all, "[object Object]"); // THE regression this module exists for
has(all, "Fix &lt;b&gt;bug&lt;/b&gt;");
notHas(all, "<b>bug</b>");
has(all, "2026-10-10");
has(all, "high");
has(all, "Dentist");
has(all, "Clinic");
notHas(all, "Invalid Date"); // bad/missing dates must not leak "Invalid Date"
has(all, "Untitled note"); // empty note title fallback
has(all, "Likes &lt;tea&gt;");
has(all, "preference");
assert.ok(!all.includes("n".repeat(121)), "note snippet must be truncated");
has(all, "…");

// 2. Legacy string tasks still render as plain titles.
const legacy = extractedItemsHtml({ tasks: ["Buy milk"] });
has(legacy, "Buy milk");
notHas(legacy, "[object Object]");

// 3. Attribute/HTML injection in titles stays inert (escape-first).
const evil = extractedItemsHtml({
  tasks: [{ title: `"><img src=x onerror=alert(1)>` }],
  events: [{ title: `'); DROP TABLE tasks;--`, start_at: "nope" }],
});
notHas(evil, "<img");
has(evil, "&lt;img");
has(evil, "&#039;");
notHas(evil, "Invalid Date"); // unparseable start_at falls back to the raw string

// 4. Empty / missing → no section content.
assert.equal(extractedItemsHtml({}), "");
assert.equal(extractedItemsHtml(null), "");
assert.equal(extractedItemsHtml(undefined), "");

console.log("analysis-view: all assertions passed");
```

Run: `node tools/js-checks/analysis-view.test.mjs` → **Expected FAIL** (`ERR_MODULE_NOT_FOUND` for `src/js/analysis-view.js`).

- [ ] **Step 2: Create `src/js/analysis-view.js`**

```js
// Pure, escape-first renderer for the analysis card's "Extracted" section
// (spec 2.2 / D-145). No DOM access — node harnesses import this directly.
import { escapeHtml } from "./sanitize.js";

const SNIPPET_LEN = 120;

function truncate(text, max = SNIPPET_LEN) {
  const s = String(text ?? "");
  return s.length > max ? `${s.slice(0, max - 1)}…` : s;
}

function localStamp(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return String(iso ?? "");
  return d.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

function group(title, itemsHtml) {
  if (!itemsHtml) return "";
  return `<div class="extracted-group"><h4>${escapeHtml(title)}</h4><ul class="extracted-list">${itemsHtml}</ul></div>`;
}

function taskItem(t) {
  if (t == null) return "";
  if (typeof t !== "object") {
    return `<li><span class="extracted-chip">${escapeHtml(t)}</span></li>`;
  }
  const title = String(t.title ?? "").trim() || "Untitled task";
  const meta = [t.due_date, t.priority]
    .filter(Boolean)
    .map(escapeHtml)
    .join(" · ");
  return `<li><span class="extracted-chip">${escapeHtml(title)}${
    meta ? `<span class="extracted-meta"> · ${meta}</span>` : ""
  }</span></li>`;
}

function eventItem(e) {
  if (!e || typeof e !== "object") return "";
  const title = String(e.title ?? "").trim() || "Untitled event";
  const when = localStamp(e.start_at);
  const where = e.location ? ` · ${escapeHtml(e.location)}` : "";
  return `<li><span class="extracted-chip">${escapeHtml(title)} — ${escapeHtml(when)}${where}</span></li>`;
}

function noteItem(n) {
  if (!n || typeof n !== "object") return "";
  const title = String(n.title ?? "").trim() || "Untitled note";
  const snippet = truncate(n.content);
  return `<li><span class="extracted-chip"><strong>${escapeHtml(title)}</strong>${
    snippet ? ` — ${escapeHtml(snippet)}` : ""
  }</span></li>`;
}

function factItem(f) {
  if (!f || typeof f !== "object") return "";
  const cat = f.category
    ? `<span class="extracted-cat">${escapeHtml(f.category)}</span> `
    : "";
  return `<li><span class="extracted-chip">${cat}${escapeHtml(f.content ?? "")}</span></li>`;
}

export function extractedItemsHtml(result) {
  if (!result || typeof result !== "object") return "";
  const tasks = Array.isArray(result.tasks) ? result.tasks : [];
  const events = Array.isArray(result.events) ? result.events : [];
  const notes = Array.isArray(result.notes) ? result.notes : [];
  const facts = Array.isArray(result.facts) ? result.facts : [];
  return [
    group("Tasks", tasks.map(taskItem).join("")),
    group("Events", events.map(eventItem).join("")),
    group("Notes", notes.map(noteItem).join("")),
    group("Facts", facts.map(factItem).join("")),
  ].join("");
}
```

- [ ] **Step 3: Gates**

Run: `node tools/js-checks/analysis-view.test.mjs` → **PASS** (`analysis-view: all assertions passed`).
Run: `node --check src/js/analysis-view.js` → clean.
Run the other harnesses (must stay green): `node tools/js-checks/mood.test.mjs`, `node tools/js-checks/proposals.test.mjs`, `node tools/js-checks/chat-format.test.mjs`, `node tools/js-checks/board.test.mjs`.

- [ ] **Step 4: Commit**

```bash
git add src/js/analysis-view.js tools/js-checks/analysis-view.test.mjs
git commit -m "feat(analysis): escape-first Extracted-section renderer with node harness"
```

### Task 6: Analysis status strip + read-only analysis card + live Analyze button (spec 1c, 2.2, 2.3)

**Files:**
- Modify: `src/index.html` (strip markup after `.sidebar-header` close, L672)
- Modify: `src/styles/ai-chat.css` (append strip + extracted styles)
- Modify: `src/js/ai-sidebar.js` (strip state machine, ungated events, `renderAnalysisResult` rewrite, deletions)
- Modify: `src/js/tabs/journal.js` (wire `#btn-analyze`, INIT section ~L275)

**Interfaces:**
- Consumes: `JournalAnalysisQueued/Processing/Completed/Error` events (Task 3 emits `Processing`), `extractedItemsHtml` (Task 5), `analysis_requeue` (Task 4; JS arg `{ entryId }`), `journal_get` (lib.rs:595; JS arg `{ id }`).
- Produces: `#analysis-strip` element with `is-idle|is-queued|is-analyzing|is-done|is-error` classes; read-only analysis card appended to chat; `#btn-analyze` saves then emits `journal_analysis_queued` `{ id }` (payload shape matches the backend listener, lib.rs:2878-2881).

- [ ] **Step 1: Strip markup in `src/index.html`**

After the `.sidebar-header` closing `</div>` (L672), before `<div id="ai-chat-messages">` (L673), insert:

```html
    <div id="analysis-strip" class="analysis-strip is-idle" role="status" aria-live="polite">
      <span class="analysis-strip-state" id="analysis-strip-state">Idle</span>
      <span class="analysis-strip-detail" id="analysis-strip-detail">Auto-analyzed as you type — no save needed.</span>
      <span class="analysis-strip-time" id="analysis-strip-time"></span>
      <button id="btn-analysis-retry" class="btn-ghost-sm hidden">Retry</button>
    </div>
```

(`.hidden` = `base.css:78`, `display:none !important`.)

- [ ] **Step 2: Append to `src/styles/ai-chat.css` (end of file)**

```css
/* ── Analysis status strip (spec 2.3) ─────────────────── */
.analysis-strip {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-surface-soft);
  font-size: 12px;
  color: var(--color-muted);
  min-height: 32px;
}
.analysis-strip-state {
  flex: none;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--color-muted);
}
.analysis-strip.is-queued .analysis-strip-state { color: var(--color-warn); }
.analysis-strip.is-analyzing .analysis-strip-state { color: var(--color-accent); }
.analysis-strip.is-done .analysis-strip-state { color: var(--color-success); }
.analysis-strip.is-error .analysis-strip-state { color: var(--color-error); }
.analysis-strip-detail {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.analysis-strip-time { flex: none; font-variant-numeric: tabular-nums; }
.analysis-strip #btn-analysis-retry { flex: none; }

/* ── Analysis card "Extracted" section (spec 2.2) ─────── */
.extracted-group + .extracted-group { margin-top: 8px; }
.extracted-group h4 {
  margin: 0 0 4px;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-muted);
}
.extracted-list { list-style: none; margin: 0; padding: 0; }
.extracted-list li + li { margin-top: 4px; }
.extracted-chip {
  display: inline-block;
  max-width: 100%;
  padding: 3px 8px;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: var(--color-surface);
  font-size: 12px;
  color: var(--color-text);
  overflow-wrap: anywhere;
}
.extracted-meta { color: var(--color-muted); }
.extracted-cat {
  display: inline-block;
  margin-right: 6px;
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--color-accent-soft);
  color: var(--color-accent);
  font-size: 10px;
  text-transform: uppercase;
}
```

(All vars verified against `base.css:10-33`. The dead `.ai-bubble.queued/.processing` rules at `ai-chat.css:37` are left in place — cleanup-only, out of scope.)

- [ ] **Step 3: `src/js/ai-sidebar.js` — delete dead helpers**

- Delete `encodeAttr`, `decodeAttr`, `createFactId` (L15-32) — sole callers are the tasks/facts blocks deleted in Step 5 (`createFactId` is already unreferenced).
- Delete `showQueuedStatus` (L296-304) and `showProcessingStatus` (L306-314) — their two call sites are replaced in Step 4. **Keep `showErrorStatus`** — it is still called by the `ai_model_missing` case (L240-266).

- [ ] **Step 4: `src/js/ai-sidebar.js` — strip state machine + ungated events**

Add import at the top (after the `proposals.js` import, L6):

```js
import { extractedItemsHtml } from "./analysis-view.js";
```

Add the strip module below the module-scope state (after `inboxEl = null;`, L13):

```js
// ── ANALYSIS STATUS STRIP (spec 2.3) ────────────────────
// Global: updates for ANY entry id (not gated on currentEntryId).
const strip = { state: "idle", entryId: null, startedAt: 0, doneIn: null, errorText: null, timer: null };
const stripTitles = new Map(); // entryId -> cached title (foreign entries)

function stripEls() {
  return {
    root: document.getElementById("analysis-strip"),
    state: document.getElementById("analysis-strip-state"),
    detail: document.getElementById("analysis-strip-detail"),
    time: document.getElementById("analysis-strip-time"),
    retry: document.getElementById("btn-analysis-retry"),
  };
}

function stripTitle(entryId) {
  if (!entryId) return "";
  if (currentEntryId && currentEntryId.value === entryId) {
    const openTitle = document.getElementById("journal-title")?.value?.trim();
    return openTitle || "Untitled Entry";
  }
  return stripTitles.get(entryId) || "";
}

function fetchStripTitle(entryId) {
  if (!entryId || stripTitles.has(entryId)) return;
  if (currentEntryId && currentEntryId.value === entryId) return; // open entry reads live from the title input
  stripTitles.set(entryId, ""); // reserve so parallel events don't double-fetch
  invoke("journal_get", { id: entryId }, { silent: true })
    .then((entry) => {
      stripTitles.set(entryId, entry ? entry.title?.trim() || "Untitled Entry" : "");
      renderStrip();
    })
    .catch(() => {});
}

function stripElapsed() {
  if (!strip.startedAt) return "";
  return `${((Date.now() - strip.startedAt) / 1000).toFixed(1)}s`;
}

function renderStrip() {
  const els = stripEls();
  if (!els.root) return;
  els.root.classList.remove("is-idle", "is-queued", "is-analyzing", "is-done", "is-error");
  els.root.classList.add(`is-${strip.state}`);
  if (els.retry) els.retry.classList.toggle("hidden", strip.state !== "error");

  const title = stripTitle(strip.entryId);
  const ticking = strip.state === "queued" || strip.state === "analyzing";

  switch (strip.state) {
    case "queued":
      els.state.textContent = "Queued";
      els.detail.textContent = title ? `waiting — ${title}` : "waiting for analysis…";
      break;
    case "analyzing":
      els.state.textContent = "Analyzing…";
      els.detail.textContent = title || "working on it…";
      break;
    case "done":
      els.state.textContent = "Done";
      els.detail.textContent =
        strip.entryId && currentEntryId?.value === strip.entryId
          ? title
          : "open the entry to view.";
      break;
    case "error":
      els.state.textContent = "Error";
      els.detail.textContent = strip.errorText || "analysis failed";
      break;
    default:
      els.state.textContent = "Idle";
      els.detail.textContent = "Auto-analyzed as you type — no save needed.";
  }

  if (ticking) {
    els.time.textContent = stripElapsed();
    if (!strip.timer) strip.timer = setInterval(renderStrip, 200);
  } else {
    if (strip.timer) {
      clearInterval(strip.timer);
      strip.timer = null;
    }
    els.time.textContent = strip.doneIn != null ? `in ${strip.doneIn}s` : "";
  }
}

function updateAnalysisStrip(state, entryId, error) {
  const prev = strip.state;
  const sameEntry = strip.entryId === entryId;
  if (!sameEntry) {
    strip.entryId = entryId;
    strip.startedAt = Date.now();
    strip.doneIn = null;
  } else if (state === "queued" && (prev === "idle" || prev === "done" || prev === "error")) {
    strip.startedAt = Date.now(); // a new analysis cycle for the same entry
    strip.doneIn = null;
  } else if (state === "analyzing" && !strip.startedAt) {
    strip.startedAt = Date.now();
  }
  strip.state = state;
  if (state === "done") {
    strip.doneIn = strip.startedAt
      ? Number(((Date.now() - strip.startedAt) / 1000).toFixed(1))
      : null;
  }
  strip.errorText = state === "error" ? String(error ?? "analysis failed").slice(0, 200) : null;
  if (entryId && state !== "error") fetchStripTitle(entryId);
  renderStrip();
}
```

Wire the Retry button inside `initAiSidebar` (after the `inputEl` keydown block, L60):

```js
  const retryBtn = document.getElementById("btn-analysis-retry");
  retryBtn?.addEventListener("click", async () => {
    if (!strip.entryId) return;
    retryBtn.disabled = true;
    try {
      await invoke("analysis_requeue", { entryId: strip.entryId });
      recordDiagnostic("analysis:requeued", { entry_id: strip.entryId });
      updateAnalysisStrip("queued", strip.entryId);
    } catch (err) {
      // invoke() already toasted the failure (non-silent); just record it.
      recordDiagnostic("analysis:requeue_failed", {
        entry_id: strip.entryId,
        error: stringifyError(err),
      });
    } finally {
      retryBtn.disabled = false;
    }
  });
```

Replace the four analysis cases in `handleAppEvent` (L195-217) — **all ungated** (strip is global; only the result card stays gated on the open entry):

```js
    case "journal_analysis_queued":
      updateAnalysisStrip("queued", payload.entry_id);
      break;

    case "journal_analysis_processing":
      updateAnalysisStrip("analyzing", payload.entry_id);
      break;

    case "journal_analysis_completed":
      updateAnalysisStrip("done", payload.entry_id);
      if (currentEntryId.value === payload.entry_id) {
        renderAnalysisResult(payload.result);
      }
      break;

    case "journal_analysis_error":
      updateAnalysisStrip("error", payload.entry_id, payload.error);
      break;
```

(The `ai_model_missing` and `ai_proposals_ready` cases are untouched.)

- [ ] **Step 5: `src/js/ai-sidebar.js` — rewrite `renderAnalysisResult` (L334-491) as read-only + appended**

Replace the entire `renderAnalysisResult` function with:

```js
function renderAnalysisResult(result) {
  if (!messagesEl) return;
  const { summary, emotions, insights, mood } = result || {};
  recordDiagnostic("analysis:rendered", {
    mood,
    emotion_count: emotions?.length ?? 0,
    task_count: result?.tasks?.length ?? 0,
    insight_count: insights?.length ?? 0,
    fact_count: result?.facts?.length ?? 0,
    event_count: result?.events?.length ?? 0,
    note_count: result?.notes?.length ?? 0,
  });

  let html = `
    <div class="analysis-card">
      <div class="analysis-section summary">
        <div class="mood-badge">${escapeHtml(mood || "Reflective")}</div>
        <div style="display:flex; justify-content:space-between; align-items:center;">
          <h3>Key Summary</h3>
          <button class="btn-ghost-sm btn-view-source-entry" data-id="${escapeHtml(result.id)}" title="View original entry">
            <i data-lucide="eye" style="width:14px;height:14px;"></i>
          </button>
        </div>
        <p>${escapeHtml(summary)}</p>
      </div>
  `;

  if (emotions && emotions.length > 0) {
    html += `
      <div class="analysis-section emotions">
        <h3>Emotions</h3>
        <div class="tag-cloud">
          ${emotions.map((e) => `<span class="tag emotion">${escapeHtml(e)}</span>`).join("")}
        </div>
      </div>
    `;
  }

  if (insights && insights.length > 0) {
    html += `
      <div class="analysis-section insights">
        <h3>Deeper Insights</h3>
        <ul>${insights.map((i) => `<li>${escapeHtml(i)}</li>`).join("")}</ul>
      </div>
    `;
  }

  // Read-only extractions (D-145) — accept/dismiss happens in the
  // proposalCardHtml cards that AiProposalsReady renders (Task 3).
  const extracted = extractedItemsHtml(result);
  if (extracted) {
    html += `<div class="analysis-section extracted"><h3>Extracted</h3>${extracted}</div>`;
  }

  html += `</div>`;

  // APPEND — never wipe chat history (spec 1c/2.3).
  const wrap = document.createElement("div");
  wrap.className = "ai-result-flow";
  wrap.innerHTML = html;
  messagesEl.appendChild(wrap);
  messagesEl.scrollTop = messagesEl.scrollHeight;

  // Scoped to this card so earlier cards don't accumulate duplicate listeners.
  wrap.querySelectorAll(".btn-view-source-entry").forEach((btn) => {
    btn.addEventListener("click", () => {
      const id = btn.dataset.id;
      const journalTab = document.querySelector('[data-tab="journal"]');
      if (journalTab) journalTab.click();
      if (window.loadJournalEntry) window.loadJournalEntry(id);
    });
  });
}
```

This single replacement deletes, in one move: the Action Items block, the Proposed Facts block, the task-add listener, and both fact accept/reject listeners (the bespoke accept surfaces — spec 2.2).

- [ ] **Step 6: Wire `#btn-analyze` in `src/js/tabs/journal.js`**

In the INIT section, after `btnDelete?.addEventListener(...)` (L279), insert:

```js
  const btnAnalyze = document.getElementById("btn-analyze");
  btnAnalyze?.addEventListener("click", async () => {
    clearTimeout(saveTimeout); // don't let a pending autosave race the emit
    await save();
    const id = idInput.value;
    if (!id) return; // blank/new entry with nothing to analyze
    recordDiagnostic("journal:analysis_manual", { id, manual: true });
    globalThis.__TAURI__.event.emit("journal_analysis_queued", { id });
  });
```

(`{ id }` is exactly the payload shape the backend listener parses at lib.rs:2880; the backend echoes `AppEvent::JournalAnalysisQueued` on `app_event`, which drives the strip — no direct strip call needed here.)

- [ ] **Step 7: Gates**

Run: `node --check src/js/ai-sidebar.js src/js/analysis-view.js src/js/tabs/journal.js` → clean.
Run: `node tools/js-checks/analysis-view.test.mjs` → PASS.
Run: `node tools/js-checks/mood.test.mjs && node tools/js-checks/proposals.test.mjs && node tools/js-checks/chat-format.test.mjs && node tools/js-checks/board.test.mjs` → all PASS.
No Rust changes in this task — `cargo check`/`cargo test` unchanged from Task 4 (41).

- [ ] **Step 8: Commit**

```bash
git add src/index.html src/styles/ai-chat.css src/js/ai-sidebar.js src/js/tabs/journal.js
git commit -m "feat(analysis): global status strip, read-only analysis card, live Analyze button"
```

### Task 7: System tab shell — nav item, lazy view, moved Runtime & Logs, Settings trim (spec 2.4, 2.6/D-146)

**Files:**
- Modify: `src/index.html` (nav item after the System section label @ L38; `#tab-system` placeholder before `#tab-settings` @ L463; delete the Runtime & Logs section L607–643)
- Create: `src/views/system.html`
- Create: `src/js/tabs/system.js`
- Modify: `src/js/app.js` (`system` branch in the generic nav handler, after the `settings` branch @ L52-60)
- Modify: `src/js/tabs/settings-diagnostics.js` (trim to Activity Log only)

**Interfaces:**
- Consumes: `loadView(tabId, filePath)` (app.js:110 — hoisted function declaration, callable from the nav handler), `invoke` (ipc.js — non-silent errors toast + re-throw automatically), `escapeHtml` (sanitize.js), `showToast` (notifications.js). Commands:
  - `app_diagnostics` → `AppDiagnostics` **snake_case wire** (locked by Task 3's `wire_tests`): `data_dir`, `db_path`, `db_size_bytes`, `log_path`, `journal_entries`, `tasks`, `memory_facts`, `conversations`, `analysis_queue_len`, `analysis_statuses {queued, processing, done, failed}`, `engine {chat, embed}` → `ServiceDiagState {status, pid, latency_ms}` + `engine.last_health_check`, `models[] {kind, name, size_bytes}`.
  - `log_tail { lines }` → `LogTailResponse` (`#[serde(rename_all = "camelCase")]`, both keys single-word → identical either way).
- Produces: exported `initSystem()`; `window._systemInitialized`; `window.refreshSystemTab()`. The view **keeps the moved ids** (`rt-*`, `log-tail`, `log-tail-level`, `btn-open-log`) so `settings-diagnostics.css` (globally linked at index.html:11) styles them unchanged — zero CSS moves.
- **Wire fix included (spec 1d):** the old Settings code read `d.dataDir` / `d.dbPath` / `d.logPath` / `latencyMs` / `lastHealthCheck` — camelCase keys that do not exist on the snake_case wire, so 8 cells rendered `—` forever and Open-log silently dead-ended. `system.js` reads the snake_case keys.

- [ ] **Step 1: `src/index.html` — nav item, placeholder, delete Runtime & Logs**

1a. Replace the System section label + Settings nav item (L38-39) with label + System item + Settings item:

```html
      <div class="nav-section-label">System</div>
      <button class="nav-item" data-tab="system"><i data-lucide="server"></i><span>System</span></button>
      <button class="nav-item" data-tab="settings"><i data-lucide="settings"></i><span>Settings</span></button>
```

(Sidebar order becomes Journal · Tasks · Mood · **System** · Settings — the D-141 lean nav amends to 5 tabs, D-146.)

1b. Immediately before `<div id="tab-settings" class="tab-view">` (L463), insert the lazy placeholder (same spinner pattern as `#tab-reports` L457-461):

```html
    <div id="tab-system" class="tab-view">
      <div class="empty-state">
        <div class="loading-spinner"></div>
        <p>Loading system…</p>
      </div>
    </div>
```

1c. Delete the entire Runtime & Logs block: the `<section class="settings-section">` whose header contains `<h3>Runtime &amp; Logs</h3>` (section opens at L607) through its closing `</section>` (L643) — i.e. everything between the Activity Log section's `</section>` (L605) and the `<section>` that opens Development & Debugging (L644). Settings section order afterwards: **LLM Engine · System Check · Vault & Security · Activity Log · Development & Debugging** (spec 2.4).

- [ ] **Step 2: Create `src/views/system.html`**

```html
<div class="settings-view-layout">
  <header class="settings-header">
    <h2>System</h2>
    <p>Runtime state, engines, analysis queue, database tools, and the app log.</p>
  </header>

  <section class="settings-section">
    <div class="settings-section-header">
      <div class="settings-section-icon"><i data-lucide="gauge"></i></div>
      <div class="settings-section-title">
        <h3>Runtime</h3>
        <p>Live runtime state (DB, engines, models, analysis queue).</p>
      </div>
    </div>
    <div class="settings-section-body">
      <div class="runtime-grid" id="runtime-grid">
        <div class="runtime-cell"><span class="runtime-label">Data dir</span><span class="runtime-value mono" id="rt-data-dir">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">DB file</span><span class="runtime-value mono" id="rt-db-path">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">DB size</span><span class="runtime-value" id="rt-db-size">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Log file</span><span class="runtime-value mono" id="rt-log-path">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Journal entries</span><span class="runtime-value" id="rt-journal">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Tasks</span><span class="runtime-value" id="rt-tasks">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Memory facts</span><span class="runtime-value" id="rt-facts">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Conversations</span><span class="runtime-value" id="rt-convos">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Analysis queue</span><span class="runtime-value" id="rt-queue">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Analysis statuses</span><span class="runtime-value" id="rt-statuses">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Chat server</span><span class="runtime-value" id="rt-chat">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Embed server</span><span class="runtime-value" id="rt-embed">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Health check</span><span class="runtime-value" id="rt-health">—</span></div>
        <div class="runtime-cell"><span class="runtime-label">Models</span><span class="runtime-value" id="rt-models">—</span></div>
      </div>
    </div>
  </section>

  <!-- Task 8 inserts the Engines · Analysis queue · DB · SQL runner sections here. -->

  <section class="settings-section">
    <div class="settings-section-header">
      <div class="settings-section-icon"><i data-lucide="file-text"></i></div>
      <div class="settings-section-title">
        <h3>Log Viewer</h3>
        <p>The verbose DEBUG log the app writes to disk.</p>
      </div>
    </div>
    <div class="settings-section-body">
      <div class="log-tail-toolbar">
        <label for="log-tail-level">Level</label>
        <select id="log-tail-level" class="select-control">
          <option value="INFO" selected>INFO</option>
          <option value="DEBUG">DEBUG</option>
          <option value="WARN">WARN</option>
          <option value="ERROR">ERROR</option>
        </select>
        <button id="btn-open-log" class="btn-ghost-sm">Open log file</button>
      </div>
      <pre id="log-tail" class="log-tail-pre"><span class="log-tail-placeholder">Log file not available yet.</span></pre>
    </div>
  </section>
</div>
```

(Moved `rt-*` markup verbatim from index.html L612-625, plus the two new cells `rt-statuses` / `rt-models` from spec 2.4; moved log toolbar + `<pre>` verbatim from L626-641. `loadView` calls `renderIcons()` after injection, so the lucide icons render.)

- [ ] **Step 3: Create `src/js/tabs/system.js`** (runtime + log; Task 8 appends the control sections)

```js
// System tab — runtime grid + log viewer (moved out of Settings, spec 2.4).
// Reads snake_case AppDiagnostics keys (D-148): the old Settings copy read
// camelCase keys and silently rendered 8 dead "—" cells.
// Task 8 appends engines / analysis queue / DB / SQL controls below.
import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";
import { showToast } from "../notifications.js";

const RUNTIME_MS = 3000;
const TAIL_MS = 2000;
const LOG_TAIL_LINES = 200;
const LEVEL_ORDER = ["DEBUG", "INFO", "WARN", "ERROR"];

let initialized = false;
let runtimeTimer = null;
let tailTimer = null;

function systemTabActive() {
  const nav = document.querySelector('.nav-item[data-tab="system"]');
  return nav ? nav.classList.contains("active") : false;
}

function fmtTime(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso || "";
  const pad = (n, w = 2) => String(n).padStart(w, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
}

function fmtBytes(n) {
  if (n == null || n === 0) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(2)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function setCell(id, text) {
  const el = document.getElementById(id);
  if (el) el.textContent = text ?? "—";
}

async function refreshRuntime() {
  try {
    const d = await invoke("app_diagnostics", {}, { silent: true });
    setCell("rt-data-dir", d.data_dir);
    setCell("rt-db-path", d.db_path);
    setCell("rt-db-size", fmtBytes(d.db_size_bytes));
    setCell("rt-log-path", d.log_path);
    setCell("rt-journal", d.journal_entries);
    setCell("rt-tasks", d.tasks);
    setCell("rt-facts", d.memory_facts);
    setCell("rt-convos", d.conversations);
    setCell("rt-queue", d.analysis_queue_len);

    const counts = d.analysis_statuses || {};
    setCell(
      "rt-statuses",
      `queued ${counts.queued ?? 0} · processing ${counts.processing ?? 0} · done ${counts.done ?? 0} · failed ${counts.failed ?? 0}`,
    );
    const models = Array.isArray(d.models) ? d.models : [];
    setCell(
      "rt-models",
      models.length
        ? models.map((m) => `${m.kind}: ${m.name} (${fmtBytes(m.size_bytes)})`).join(" · ")
        : "no models imported",
    );

    const st = (s) => `${s.status || "unknown"}${s.pid ? ` (pid ${s.pid})` : ""}`;
    setCell("rt-chat", st(d.engine?.chat || {}));
    setCell("rt-embed", st(d.engine?.embed || {}));

    const chatLat = d.engine?.chat?.latency_ms;
    const embedLat = d.engine?.embed?.latency_ms;
    const checked = d.engine?.last_health_check;
    setCell(
      "rt-health",
      checked
        ? `${chatLat != null ? chatLat + "ms" : "?"} / ${embedLat != null ? embedLat + "ms" : "?"} @ ${fmtTime(checked)}`
        : "never checked",
    );
  } catch (e) {
    console.warn("[SYSTEM] runtime refresh failed", e);
  }
}

function lineLevel(line) {
  const m = line.match(/\b(DEBUG|INFO|WARN|ERROR)\b/);
  return m ? m[1] : "INFO";
}

async function refreshLogTail() {
  const pre = document.getElementById("log-tail");
  if (!pre) return;
  const filter = document.getElementById("log-tail-level")?.value || "INFO";
  const minIdx = LEVEL_ORDER.indexOf(filter);
  if (minIdx < 0) return;

  try {
    const resp = await invoke("log_tail", { lines: LOG_TAIL_LINES }, { silent: true });
    const lines = resp?.lines || [];
    if (!lines.length) {
      pre.innerHTML = '<span class="log-tail-placeholder">Log file not available yet.</span>';
      return;
    }
    const from = LOG_TAIL_LINES - Math.min(LOG_TAIL_LINES, lines.length);
    const filtered = lines
      .map((line, i) => ({ line, level: lineLevel(line), n: from + i + 1 }))
      .filter((x) => LEVEL_ORDER.indexOf(x.level) >= minIdx);

    pre.innerHTML = filtered
      .map(
        (x) =>
          `<span class="log-line-${x.level.toLowerCase()}">${x.n}. ${escapeHtml(x.line)}</span>` + "\n",
      )
      .join("");
  } catch {
    pre.innerHTML = '<span class="log-tail-placeholder">Log file not available yet.</span>';
  }
}

function wireLogToolbar() {
  document.getElementById("log-tail-level")?.addEventListener("change", refreshLogTail);
  document.getElementById("btn-open-log")?.addEventListener("click", async () => {
    try {
      const d = await invoke("app_diagnostics", {}, { silent: true });
      // Wire key is snake_case (D-148) — the old Settings code read d.logPath.
      if (window.__TAURI__?.shell?.open && d.log_path) {
        await window.__TAURI__.shell.open(d.log_path);
      } else {
        showToast("Cannot open log file here.", { variant: "warn" });
      }
    } catch (e) {
      console.warn("[SYSTEM] open log failed", e);
    }
  });
}

export function initSystem() {
  if (initialized) return;
  initialized = true;

  wireLogToolbar();
  refreshRuntime();
  refreshLogTail();

  runtimeTimer = setInterval(() => {
    if (systemTabActive()) refreshRuntime();
  }, RUNTIME_MS);
  tailTimer = setInterval(() => {
    if (systemTabActive()) refreshLogTail();
  }, TAIL_MS);

  window.refreshSystemTab = () => {
    refreshRuntime();
    refreshLogTail();
  };
}
```

- [ ] **Step 4: `src/js/app.js` — `system` branch in the generic nav handler**

After the `if (tab === "settings") { ... }` block (ends L60), insert:

```js
    if (tab === "system") {
      // The view is lazy (empty placeholder until first click) — unlike the
      // inline Settings markup, initSystem must wait for loadView to land DOM.
      loadView("system", "views/system.html").then(() => {
        if (!document.getElementById("tab-system")?.dataset.viewLoaded) return; // fetch failed — retry next click
        if (!window._systemInitialized) {
          window._systemInitialized = true;
          import("./tabs/system.js").then((m) => m.initSystem());
        } else if (window.refreshSystemTab) {
          window.refreshSystemTab();
        }
      });
    }
```

(First click → load view → `initSystem()`; later clicks → `refreshSystemTab()` — the memory-tab lazy pattern (app.js:159-166) with an added refresh, mirroring the settings branch's init-or-refresh shape.)

- [ ] **Step 5: `src/js/tabs/settings-diagnostics.js` — trim to Activity Log only**

1. Update the header comment (L1-3) to: `// Settings "Activity Log" card. The Runtime & Logs card moved to the System tab (spec 2.4).`
2. Delete constants `LOG_TAIL_LINES`, `LEVEL_ORDER` (L10-11) and state `runtimeTimer`, `tailTimer` (L16-17).
3. Delete the whole `// ── Runtime & Logs ──` section (L266-364): `fmtBytes`, `setCell`, `refreshRuntime`, `lineLevel`, `refreshLogTail`, `wireLogToolbar`.
4. Replace the tail of `initSettingsDiagnostics` (everything from `wireLogToolbar();` at L386 to the end of the function) with:

```js
  await refreshActivityLog();
  activityTimer = setInterval(() => {
    if (!paused && settingsTabActive()) refreshActivityLog();
  }, 700);

  // Hook so opening the Settings tab forces an immediate refresh.
  window.refreshSettingsDiagnostics = async () => {
    if (!paused) await refreshActivityLog();
  };
}
```

5. Replace `disposeSettingsDiagnostics` with:

```js
export function disposeSettingsDiagnostics() {
  if (activityTimer) clearInterval(activityTimer);
  activityTimer = null;
  initialized = false;
}
```

Imports stay as-is (`invoke`, `getDiagnosticsSnapshot`, `escapeHtml`, `showToast` are all still used by the Activity Log code). `settings.js` needs no change — it already only calls `initSettingsDiagnostics()`.

- [ ] **Step 6: Gates**

Run: `node --check src/js/tabs/system.js src/js/tabs/settings-diagnostics.js src/js/app.js` → clean.
Run all harnesses: `node tools/js-checks/mood.test.mjs && node tools/js-checks/proposals.test.mjs && node tools/js-checks/chat-format.test.mjs && node tools/js-checks/board.test.mjs && node tools/js-checks/ipc-conventions.test.mjs && node tools/js-checks/analysis-view.test.mjs` → all PASS.
Run: `grep -n "runtimeTimer\|tailTimer\|refreshRuntime\|refreshLogTail\|wireLogToolbar\|fmtBytes\|LOG_TAIL\|log-tail\|rt-" src/js/tabs/settings-diagnostics.js` → **no matches** (Activity Log only).
Run: `grep -c '<section class="settings-section">' src/index.html` → **7** (was 8; the Runtime & Logs section is gone).
No Rust changes in this task — `cargo check`/`cargo test` unchanged from Task 4 (41).

- [ ] **Step 7: Commit**

```bash
git add src/index.html src/views/system.html src/js/tabs/system.js src/js/app.js src/js/tabs/settings-diagnostics.js
git commit -m "feat(system): System tab shell with moved runtime grid + log viewer (D-146)"
```

### Task 8: System tab controls — engines, analysis queue, DB, SQL runner (spec 2.4)

**Files:**
- Modify: `src/views/system.html` (4 sections between Runtime and Log Viewer, + view-local `<style>`)
- Modify: `src/js/tabs/system.js` (control wiring, queue renderer, SQL renderer, engine timer)

**Interfaces:**
- Consumes (all commands registered by Task 4; top-level JS args camelCase per D-148):
  - `get_engine_status` (no args) → `EngineStatusSnapshot {chat, embed}` — status strings `running | starting | stopped | failed: … | not_configured` (lib.rs:1867).
  - `check_engine_health` (no args) → `EngineHealth {chat, embed, chat_latency_ms, embed_latency_ms, checked_at}` (snake keys, lib.rs:1853).
  - `provision_model_cmd` (no args) → summary `String`.
  - `stop_engine` / `restart_engine` → `{ service: "chat" | "embed" }`.
  - `analysis_queue_list` (no args) → `QueueRow[] {entry_id, status, queued_at, updated_at, attempts}` (Task 4; failures already float to the top).
  - `analysis_requeue` → `{ entryId }` (Task 4; resets a failed row then re-runs `enqueue_analysis`).
  - `backup_now` (no args) → produced backup path `String`.
  - `sql_run` → `{ query }` → `SqlRunResult {columns, rows, changes}` (`changes` structurally `0`).
  - `window.__TAURI__.shell.open` for Open data dir (same mechanism as Open log; `withGlobalTauri: true`).
- Reuses global CSS only: `badge-status.*`, `settings-status[.success|.error]`, `status-dot[.ok|.warn|.error]`, `activity-log-wrap/-table`, `col-time`, `col-dur`, `mono`, `btn-primary`, `btn-ghost-sm`, `textarea-control`, `select-control`, `.hidden` — plus a small view-local `<style>` (about.html pattern) for the new row/action classes.

- [ ] **Step 1: `src/views/system.html` — replace the Task 8 comment with 4 sections**

Replace `<!-- Task 8 inserts the Engines · Analysis queue · DB · SQL runner sections here. -->` with:

```html
  <section class="settings-section">
    <div class="settings-section-header">
      <div class="settings-section-icon"><i data-lucide="cpu"></i></div>
      <div class="settings-section-title">
        <h3>AI Engines</h3>
        <p>llama-server processes — chat (:8080) and embed (:8081).</p>
      </div>
    </div>
    <div class="settings-section-body">
      <div class="engine-rows" id="engine-controls">
        <div class="engine-row">
          <span class="status-dot" data-engine-dot="chat"></span>
          <span class="engine-name">Chat</span>
          <span class="engine-state mono" data-engine-state="chat">—</span>
          <span class="engine-latency mono" data-engine-latency="chat">—</span>
          <button class="btn-ghost-sm" data-engine-action="check" data-service="chat">Health</button>
          <button class="btn-ghost-sm" data-engine-action="restart" data-service="chat">Restart</button>
          <button class="btn-ghost-sm" data-engine-action="stop" data-service="chat">Stop</button>
        </div>
        <div class="engine-row">
          <span class="status-dot" data-engine-dot="embed"></span>
          <span class="engine-name">Embed</span>
          <span class="engine-state mono" data-engine-state="embed">—</span>
          <span class="engine-latency mono" data-engine-latency="embed">—</span>
          <button class="btn-ghost-sm" data-engine-action="check" data-service="embed">Health</button>
          <button class="btn-ghost-sm" data-engine-action="restart" data-service="embed">Restart</button>
          <button class="btn-ghost-sm" data-engine-action="stop" data-service="embed">Stop</button>
        </div>
      </div>
      <div class="sys-actions">
        <button id="btn-engine-start" class="btn-primary">Start engines</button>
        <span id="engine-start-status" class="settings-status"></span>
      </div>
    </div>
  </section>

  <section class="settings-section">
    <div class="settings-section-header">
      <div class="settings-section-icon"><i data-lucide="list"></i></div>
      <div class="settings-section-title">
        <h3>Analysis Queue</h3>
        <p>Journal entries waiting on — or failed in — the AI analysis worker.</p>
      </div>
    </div>
    <div class="settings-section-body">
      <div class="sys-actions">
        <button id="btn-queue-refresh" class="btn-ghost-sm">Refresh</button>
        <span id="queue-count" class="queue-count"></span>
      </div>
      <div class="activity-log-wrap">
        <table class="activity-log-table">
          <thead>
            <tr>
              <th>Entry</th>
              <th>Status</th>
              <th>Queued</th>
              <th>Updated</th>
              <th class="col-dur">Tries</th>
              <th></th>
            </tr>
          </thead>
          <tbody id="queue-body">
            <tr class="activity-log-empty">
              <td colspan="6">Queue is empty.</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </section>

  <section class="settings-section">
    <div class="settings-section-header">
      <div class="settings-section-icon"><i data-lucide="database"></i></div>
      <div class="settings-section-title">
        <h3>Database</h3>
        <p>Manual backup (SQLite Backup API, D-98) and quick access to the data directory.</p>
      </div>
    </div>
    <div class="settings-section-body">
      <div class="sys-actions">
        <button id="btn-backup-now" class="btn-primary">Backup now</button>
        <button id="btn-open-data-dir" class="btn-ghost-sm">Open data dir</button>
        <span id="db-action-status" class="settings-status"></span>
      </div>
    </div>
  </section>

  <section class="settings-section">
    <div class="settings-section-header">
      <div class="settings-section-icon"><i data-lucide="terminal"></i></div>
      <div class="settings-section-title">
        <h3>SQL Runner</h3>
        <p>Read-only queries against the unlocked vault (SELECT / WITH / PRAGMA / EXPLAIN — D-147).</p>
      </div>
    </div>
    <div class="settings-section-body">
      <label class="sys-label" for="sys-sql-input">Statement</label>
      <textarea id="sys-sql-input" class="textarea-control" rows="3" spellcheck="false" placeholder="SELECT status, COUNT(*) FROM tasks GROUP BY status;"></textarea>
      <div class="sys-actions">
        <button id="btn-sql-run" class="btn-primary">Run</button>
        <span id="sys-sql-status" class="settings-status"></span>
      </div>
      <div class="sql-result hidden" id="sys-sql-result">
        <div class="sql-result-meta mono" id="sys-sql-meta"></div>
        <div class="activity-log-wrap">
          <table class="activity-log-table">
            <thead id="sys-sql-thead"></thead>
            <tbody id="sys-sql-body"></tbody>
          </table>
        </div>
      </div>
    </div>
  </section>
```

Then append, immediately after the closing `</div>` of `.settings-view-layout` (end of file):

```html
<style>
  .engine-rows { display: flex; flex-direction: column; gap: 8px; }
  .engine-row {
    display: flex; align-items: center; gap: 10px; flex-wrap: wrap;
    padding: 8px 10px; background: var(--color-surface-soft);
    border: 1px solid var(--color-border); border-radius: var(--radius-sm);
  }
  .engine-name { font-weight: 600; min-width: 48px; }
  .engine-state { flex: 1; font-size: 12px; color: var(--color-muted); }
  .engine-latency { font-size: 12px; color: var(--color-muted); min-width: 76px; text-align: right; }
  .sys-actions { display: flex; align-items: center; gap: 10px; margin-top: 10px; flex-wrap: wrap; }
  .queue-count { font-size: 12px; color: var(--color-muted); }
  .sys-label { display: block; font-size: 12px; color: var(--color-muted); margin-bottom: 6px; }
  .sql-result { margin-top: 12px; }
  .sql-result-meta { font-size: 11px; color: var(--color-muted); margin-bottom: 6px; }
</style>
```

- [ ] **Step 2: `src/js/tabs/system.js` — add engine/queue/DB/SQL code**

2a. Extend the import block (line 4-6) to:

```js
import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";
import { showToast, stringifyError } from "../notifications.js";
import { recordDiagnostic } from "../diagnostics.js";
```

2b. After the existing `TAIL_MS` constant, add:

```js
const ENGINE_MS = 10000;
```

2c. After the existing state declarations (`tailTimer`), add:

```js
let engineTimer = null;
let healthBusy = false;
let lastDataDir = "";
```

2d. In `refreshRuntime`, after `setCell("rt-data-dir", d.data_dir);`, add:

```js
    lastDataDir = d.data_dir || lastDataDir;
```

2e. Append the new sections to the file (after `initSystem` — they are called from the replacement `initSystem` in 2f):

```js
// ── Engines ─────────────────────────────────────

function setStatus(id, text, cls) {
  const el = document.getElementById(id);
  if (!el) return;
  el.textContent = text || "";
  el.className = cls ? `settings-status ${cls}` : "settings-status";
}

function paintEngine(service, status) {
  const state = document.querySelector(`[data-engine-state="${service}"]`);
  if (state) state.textContent = status || "—";
  const dot = document.querySelector(`[data-engine-dot="${service}"]`);
  if (!dot) return;
  dot.className = "status-dot";
  if (status === "running") dot.classList.add("ok");
  else if (status === "starting") dot.classList.add("warn");
  else if (status && status.startsWith("failed")) dot.classList.add("error");
}

function paintHealth(service, ok, latencyMs) {
  const el = document.querySelector(`[data-engine-latency="${service}"]`);
  if (!el) return;
  el.textContent = ok ? (latencyMs != null ? `${latencyMs} ms` : "ok") : "unreachable";
}

async function refreshEngines() {
  try {
    const s = await invoke("get_engine_status", {}, { silent: true });
    paintEngine("chat", s.chat);
    paintEngine("embed", s.embed);
  } catch (e) {
    console.warn("[SYSTEM] engine status failed", e);
  }
}

async function checkHealthNow() {
  if (healthBusy) return; // never overlap the HTTP probes (10 s timer + button)
  healthBusy = true;
  try {
    const h = await invoke("check_engine_health", {}, { silent: true });
    paintHealth("chat", h.chat, h.chat_latency_ms);
    paintHealth("embed", h.embed, h.embed_latency_ms);
  } catch (e) {
    console.warn("[SYSTEM] engine health failed", e);
  } finally {
    healthBusy = false;
  }
}

function wireEngineControls() {
  document.getElementById("btn-engine-start")?.addEventListener("click", async () => {
    try {
      recordDiagnostic("system:engine_start", {});
      const summary = await invoke("provision_model_cmd");
      setStatus("engine-start-status", summary, "success");
      await refreshEngines();
      await checkHealthNow();
    } catch (e) {
      setStatus("engine-start-status", stringifyError(e), "error");
    }
  });

  document.getElementById("engine-controls")?.addEventListener("click", async (ev) => {
    const btn = ev.target.closest("button[data-engine-action]");
    if (!btn) return;
    const action = btn.dataset.engineAction;
    const service = btn.dataset.service;
    if (action === "check") {
      await checkHealthNow();
      return;
    }
    btn.disabled = true;
    try {
      recordDiagnostic("system:engine_action", { action, service });
      if (action === "restart") await invoke("restart_engine", { service });
      else if (action === "stop") await invoke("stop_engine", { service });
      await refreshEngines();
      await checkHealthNow();
    } catch {
      /* ipc.js already toasted the error */
    } finally {
      btn.disabled = false;
    }
  });
}

// ── Analysis queue ──────────────────────────────

function queueBadge(status) {
  const cls =
    status === "failed" ? "error" : status === "done" ? "ok" : status === "processing" ? "info" : "warn";
  return `<span class="badge-status ${cls}">${escapeHtml(status)}</span>`;
}

async function refreshQueue() {
  const tbody = document.getElementById("queue-body");
  if (!tbody) return;
  try {
    const rows = await invoke("analysis_queue_list", {}, { silent: true });
    const count = document.getElementById("queue-count");
    if (count) count.textContent = rows.length ? `${rows.length} row(s)` : "";
    if (!rows.length) {
      tbody.innerHTML = '<tr class="activity-log-empty"><td colspan="6">Queue is empty.</td></tr>';
      return;
    }
    tbody.innerHTML = rows
      .map((r) => {
        const retry =
          r.status === "failed"
            ? `<button class="btn-ghost-sm" data-requeue="${escapeHtml(r.entry_id)}">Retry</button>`
            : "";
        return (
          "<tr>" +
          `<td class="mono" title="${escapeHtml(r.entry_id)}">${escapeHtml(String(r.entry_id).slice(0, 8))}</td>` +
          `<td>${queueBadge(r.status)}</td>` +
          `<td class="mono col-time">${escapeHtml(fmtTime(r.queued_at))}</td>` +
          `<td class="mono col-time">${escapeHtml(fmtTime(r.updated_at))}</td>` +
          `<td class="mono col-dur">${Number(r.attempts) || 0}</td>` +
          `<td>${retry}</td>` +
          "</tr>"
        );
      })
      .join("");
  } catch (e) {
    console.warn("[SYSTEM] queue refresh failed", e);
  }
}

function wireQueueControls() {
  document.getElementById("btn-queue-refresh")?.addEventListener("click", refreshQueue);
  document.getElementById("queue-body")?.addEventListener("click", async (ev) => {
    const btn = ev.target.closest("button[data-requeue]");
    if (!btn) return;
    const entryId = btn.getAttribute("data-requeue");
    btn.disabled = true;
    try {
      recordDiagnostic("system:queue_requeue", { entryId });
      await invoke("analysis_requeue", { entryId });
      showToast("Entry re-queued for analysis.", { variant: "success" });
      await refreshQueue();
      refreshRuntime(); // queue count + statuses cells move with it
    } catch {
      /* ipc.js already toasted the error */
    } finally {
      btn.disabled = false;
    }
  });
}

// ── Database ────────────────────────────────────

function wireDbControls() {
  document.getElementById("btn-backup-now")?.addEventListener("click", async () => {
    try {
      recordDiagnostic("system:backup_now", {});
      const path = await invoke("backup_now");
      setStatus("db-action-status", `Backup written: ${path}`, "success");
      showToast("Backup complete.", { variant: "success" });
    } catch (e) {
      setStatus("db-action-status", stringifyError(e), "error");
    }
  });

  document.getElementById("btn-open-data-dir")?.addEventListener("click", async () => {
    try {
      if (!lastDataDir) {
        const d = await invoke("app_diagnostics", {}, { silent: true });
        lastDataDir = d.data_dir || "";
      }
      if (window.__TAURI__?.shell?.open && lastDataDir) {
        await window.__TAURI__.shell.open(lastDataDir);
      } else {
        showToast("Cannot open the data directory here.", { variant: "warn" });
      }
    } catch (e) {
      console.warn("[SYSTEM] open data dir failed", e);
    }
  });
}

// ── SQL runner (read-only, D-147) ───────────────

function cellText(v) {
  if (v === null || v === undefined) return "NULL";
  if (typeof v === "object") {
    try {
      return JSON.stringify(v);
    } catch {
      return String(v);
    }
  }
  return String(v);
}

function renderSqlResult(res) {
  const wrap = document.getElementById("sys-sql-result");
  const meta = document.getElementById("sys-sql-meta");
  const thead = document.getElementById("sys-sql-thead");
  const tbody = document.getElementById("sys-sql-body");
  if (!wrap) return;
  const rows = res.rows || [];
  const truncated = rows.length >= 500 ? " (first 500)" : "";
  if (meta) meta.textContent = `${rows.length} row(s)${truncated} · read-only`;
  const cols = res.columns || [];
  if (thead) thead.innerHTML = `<tr>${cols.map((c) => `<th>${escapeHtml(String(c))}</th>`).join("")}</tr>`;
  if (tbody) {
    tbody.innerHTML = rows
      .map(
        (row) =>
          `<tr>${row.map((cell) => `<td class="mono">${escapeHtml(cellText(cell))}</td>`).join("")}</tr>`,
      )
      .join("");
  }
  wrap.classList.remove("hidden");
}

async function runSql() {
  const input = document.getElementById("sys-sql-input");
  const query = input?.value ?? "";
  const result = document.getElementById("sys-sql-result");
  if (!query.trim()) {
    setStatus("sys-sql-status", "Enter a statement first.", "error");
    return;
  }
  try {
    recordDiagnostic("system:sql_run", { chars: query.length });
    const res = await invoke("sql_run", { query });
    renderSqlResult(res);
    setStatus("sys-sql-status", `${(res.rows || []).length} row(s) · read-only`, "success");
  } catch (e) {
    result?.classList.add("hidden");
    setStatus("sys-sql-status", stringifyError(e), "error");
  }
}

function wireSqlControls() {
  document.getElementById("btn-sql-run")?.addEventListener("click", runSql);
  document.getElementById("sys-sql-input")?.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" && (ev.ctrlKey || ev.metaKey)) runSql();
  });
}
```

2f. Replace the whole `initSystem` body with:

```js
export function initSystem() {
  if (initialized) return;
  initialized = true;

  wireLogToolbar();
  wireEngineControls();
  wireQueueControls();
  wireDbControls();
  wireSqlControls();

  refreshRuntime();
  refreshLogTail();
  refreshEngines();
  checkHealthNow();
  refreshQueue();

  runtimeTimer = setInterval(() => {
    if (systemTabActive()) refreshRuntime();
  }, RUNTIME_MS);
  tailTimer = setInterval(() => {
    if (systemTabActive()) refreshLogTail();
  }, TAIL_MS);
  engineTimer = setInterval(() => {
    if (systemTabActive()) {
      refreshEngines();
      checkHealthNow();
    }
  }, ENGINE_MS);

  window.refreshSystemTab = () => {
    refreshRuntime();
    refreshLogTail();
    refreshEngines();
    checkHealthNow();
    refreshQueue();
  };
}
```

(The queue has **manual Refresh only** — spec 2.4; runtime 3 s, log 2 s, engines 10 s, all gated on `systemTabActive()`.)

- [ ] **Step 3: Gates**

Run: `node --check src/js/tabs/system.js` → clean.
Run all 6 harnesses (same command as Task 7 Step 6) → all PASS.
Run: `grep -n "in_progress\|draggable" src/js/tabs/tasks.js | grep "draggable"` → no `card.draggable` assignment (unchanged guard, cheap re-check).
No Rust changes in this task — `cargo check`/`cargo test` unchanged from Task 4 (41).
Live behavior (button smoke: Start/Stop/Restart, Backup now, SQL select + rejected write) is verified in Task 9's E2E — this task gates syntax + harnesses only.

- [ ] **Step 4: Commit**

```bash
git add src/views/system.html src/js/tabs/system.js
git commit -m "feat(system): engines, analysis queue, backup, and read-only SQL runner controls"
```

### Task 9: Changelog, AGENTS.md decisions, full gates, release build, live E2E (spec 2.6, 3)

**Files:**
- Modify: `CHANGELOG.md` (Unreleased → Added + Fixed)
- Modify: `AGENTS.md` (D-144..D-148 rows, D-141 amendment, file structure, wire-convention bullet, build status, open issues)
- Modify: this plan (Self-Review Notes + spec-coverage map, final step)

**Interfaces:**
- Documents everything from Tasks 1-8; no behavior changes. `CHANGELOG.md` and `AGENTS.md` are already tracked (the repo-wide `*.md` ignore only affects untracked files) — but the plan file itself needs `git add -f`.

- [x] **Step 1: `CHANGELOG.md` — insert under the existing `## [Unreleased]` heading, above the current `### Fixed` block**

```markdown
### Added
- **In Progress column (D-144):** the default Tasks board is now four columns — To Do · In Progress · Done · Archived — mapping the existing `in_progress` status that previously collapsed into To Do (`board-model.js` `BOARD_LAYOUTS.inprogress` + `columnStatusForTask`/`dropStatusFor` round-trip, `is-in-progress` card accent in `tasks.css`, stale pre-seeded placeholder columns replaced by the rendered board).
- **Dedicated System tab (D-146):** fifth sidebar item (Journal · Tasks · Mood · **System** · Settings, lucide `server`) with the moved Runtime grid — now 14 cells including `analysis_statuses` + `models`, reading snake_case wire keys — the Engines card (per-service status/Health/Restart/Stop + Start via `provision_model_cmd`, 10 s auto-health), the Analysis queue card (list + Retry failed via `analysis_requeue`), the DB card (`backup_now` via the SQLite Backup API, Open data dir), a read-only SQL runner (`sql_run`, D-147: comment-stripped `SELECT/WITH/PRAGMA/EXPLAIN` keyword gate + `sqlite3_stmt_readonly` backstop + 500-row cap), and the moved log viewer (level filter; Open-log file). Settings now shows LLM Engine · System Check · Vault & Security · Activity Log · Development & Debugging; `settings-diagnostics.js` keeps the Activity Log only.
- **Analysis status strip (spec 2.3):** fixed strip at the top of the always-mounted AI sidebar — Idle ("Auto-analyzed as you type — no save needed.") → Queued → Analyzing… (live elapsed ticker) → Done | Error with entry title, and Retry → `analysis_requeue` on failure. Status changes never touch chat history. The analysis worker now emits `JournalAnalysisProcessing {entry_id}` (the `#[allow(dead_code)]` on the event variant is gone) and `AiProposalsReady {conversation_id: entry_id, count}` after proposal rows persist.
- **Read-only analysis card + Extracted section (D-145):** analysis results **append** into chat (never wipe it) through the new pure module `src/js/analysis-view.js` (`extractedItemsHtml`) — Tasks/Events/Notes/Facts rendered from structured fields (`title`/`due_date`/`priority`…, escape-first, legacy string tasks tolerated). All accept/dismiss actions flow through the existing escape-first `proposalCardHtml` cards; the bespoke Action Items / Proposed Facts renderers and their accept/reject handlers were deleted.
- **Live Analyze button:** the editor header's previously dead `#btn-analyze` now clears any pending autosave, saves, and emits `journal_analysis_queued {id}`.
- **New backend commands** (registered in `generate_handler`): `backup_now`, `analysis_queue_list` (failures first), `analysis_requeue` (reset failed row → `enqueue_analysis`), `sql_run` (read-only, D-147).
- **D-148 wire-contract guard:** `tools/js-checks/ipc-conventions.test.mjs` fails on any top-level snake_case `invoke` payload key (≥50-site vacuous-pass guard); Rust `wire_tests::app_diagnostics_serializes_snake_case_wire_keys` locks the `AppDiagnostics` snake_case wire.

### Fixed
- **`[object Object]` action items from journal analysis:** structured task/event/note/fact objects were stringified into HTML; analysis now renders through `analysis-view.js` reading the real fields (title, due date, priority, times, snippets).
- **`memory_link` / `memory_unlink` were dead commands:** `src/js/tabs/memory.js` passed top-level snake_case args (`source_id`/`target_id`/`created_by`, `link_id`) — Tauri v2 resolves exactly one key per arg with no case fallback, so both invokes always failed. Now camelCase (`sourceId`/`targetId`/`createdBy`, `linkId`); the ipc-conventions harness keeps them fixed.
- **Settings Runtime grid showed 8 dead `—` cells and Open-log did nothing (spec 1d):** the frontend read camelCase keys (`dataDir`, `dbPath`, `dbSizeBytes`, `logPath`, `journalEntries`, `memoryFacts`, `analysisQueueLen`, `latencyMs`, `lastHealthCheck`) against the snake_case `AppDiagnostics`/`EngineDiagSnapshot`/`ServiceDiagState` wire. The System tab move rewrites the reads snake_case (and Open-log now uses `d.log_path`); the `wire_tests` serde assertion prevents the mismatch from returning silently.
```

- [x] **Step 2: `AGENTS.md` — six edits**

2a. **Known Decisions table** — after the D-143 row, append:

```markdown
| D-144    | `in_progress` is a real column on the default board; two-board toggle retained | Status already existed in the backend; only rendering was missing |
| D-145    | Journal-analysis proposals surface through the same `proposalCardHtml` cards as chat (analysis worker emits `AiProposalsReady`); the analysis card is read-only | One escape-first action surface instead of two — the divergent copy produced the `[object Object]` bug |
| D-146    | Dedicated System tab (5th nav item, lucide `server`); Runtime & Logs + log viewer move there; System Check and Activity Log stay in Settings for now | User request; keeps the change surgical (amends D-141 lean nav to 5 tabs) |
| D-147    | `sql_run` accepts read-only statements only (`SELECT/WITH/PRAGMA/EXPLAIN`, comment-stripped keyword gate + `sqlite3_stmt_readonly` backstop, 500-row cap) | All writes must flow through typed commands with D-19 confirmation and the D-33 write choke points; a generic write path would bypass both |
| D-148    | Wire contract: top-level command args = camelCase (Tauri default, never `rename_all` on `#[tauri::command]`); nested payloads, command returns, and event payloads = snake_case unless a struct explicitly opts into `#[serde(rename_all = "camelCase")]` | Tauri v2 does exact single-key lookup with no case fallback — mixed casing caused 8 dead diagnostics cells and two broken memory commands; guarded by the `AppDiagnostics` serde test + `ipc-conventions.test.mjs` |
```

2b. **D-141 row** — change `Sidebar shows Journal · Tasks · Mood · Settings` to `Sidebar shows Journal · Tasks · Mood · System (D-146) · Settings`.

2c. **File Structure tree** — add three lines (keep the surrounding layout):

```text
│   │   ├── analysis-view.js          Pure extractedItemsHtml() renderer (D-145) — node-harness tested
│   │   └── tabs/
│   │       ├── system.js             System tab — runtime/engines/queue/db/sql/log (D-146)
```

(`analysis-view.js` goes with the other `src/js/*.js` files just above the `tabs/` directory; `system.js` appended inside `tabs/`. Also note `src/views/system.html` under `views/` and `settings-diagnostics.js` is now "Activity Log only" if its line is present.)

2d. **Conventions → Rust backend** — append the bullet:

```text
- D-148 wire contract: top-level command args = camelCase (never add `rename_all` to `#[tauri::command]`); nested struct payloads, command returns, and event payloads = snake_case unless the struct explicitly opts into `#[serde(rename_all = "camelCase")]` (then the frontend reads camelCase). Tauri v2 resolves exactly one key per arg — no case fallback. Guarded by `wire_tests` in lib.rs + `tools/js-checks/ipc-conventions.test.mjs`.
```

2e. **Current Build Status** — insert a new paragraph immediately after the `**Phase:**` line (newest first, before the 2026-10-06 entry):

```text
**Last built:** System tab + board/analysis pass (2026-10-08, spec `fc719c8`, plan `docs/superpowers/plans/2026-10-08-system-tab-board-analysis.md`): **In Progress board column** (D-144; `board-model.js` 4-col layout + `columnStatusForTask`/`dropStatusFor` round-trip + `is-in-progress` accent + placeholder swap), **`[object Object]` fixed** (D-145; pure `src/js/analysis-view.js` Extracted renderer, read-only analysis card that appends to chat, all accepts via `proposalCardHtml`; bespoke Action Items/Facts renderers deleted), **analysis status strip** (global, elapsed ticker, Retry → `analysis_requeue`; worker emits `JournalAnalysisProcessing` + `AiProposalsReady {conversation_id: entry_id, count}`), **live `#btn-analyze`**, **System tab** (D-146; nav item + lazy `views/system.html` + `tabs/system.js`; moved runtime grid with snake_case reads — the 8 dead `—` cells — + statuses/models cells; engines card; queue card with failed-first Retry; `backup_now` + Open data dir; read-only `sql_run` (D-147); moved log viewer with Open-log fixed to `d.log_path`; Settings trimmed to LLM Engine · System Check · Vault · Activity Log · Dev), **D-148 wire contract + guards** (`memory_link`/`memory_unlink` camelCase args fixed, `ipc-conventions.test.mjs`, `wire_tests` serde lock). **State:** `cargo test` 41/41, `cargo check` 0 errors (6 known warnings), fmt clean, 6 node harnesses green. NOT yet rebuilt/relaunched — live E2E per plan Task 9 pending.
```

2f. **Open issues** — add at the top of the list:

```text
- Live E2E of the 2026-10-08 system-tab/board/analysis pass: rebuild + relaunch, then work the plan Task 9 Step 5 checklist (In-Progress drag, strip flow, Extracted chips, System tab all-cells + controls, carry-over: event/note proposal acceptance, Task Detail save, embed-role check, `:8081` squatter message).
```

- [x] **Step 3: Full gates (all tasks)**

Run: `cargo fmt --check` → clean.
Run (cwd `src-tauri`): `cargo check` → **0 errors**; exactly the 6 known warnings, no new ones.
Run (cwd `src-tauri`): `cargo test` → **41 pass** (35 pre-existing + board-model/serde-default/etc. from prior passes… verify the run summary shows `41 passed` — Task 3 adds 1, Task 4 adds 5 over the 35 baseline; if the baseline differs, every test must still pass and no test may be skipped).
Run: `find src/js -name "*.js" -exec node --check {} \;` → no output (CI parity: `Get-ChildItem src\js -Recurse -Filter *.js | ForEach-Object { node --check $_.FullName }`).
Run: `for t in mood proposals chat-format board ipc-conventions analysis-view; do node "tools/js-checks/$t.test.mjs" || exit 1; done` → 6 PASS lines.
Run: `git diff --check` → clean (no conflict markers / trailing whitespace).
Run: `grep -rn "card.draggable" src/js/` → no assignments (D-141 drag guard intact).

- [x] **Step 4: Release build**

Run: `cargo tauri build --bundles msi nsis` (long; run from repo root) → fresh bundles under `src-tauri/target/release/bundle/msi/` and `.../nsis/`.

- [x] **Step 5: Live E2E (rebuild → relaunch → verify; spec §3)** — verified: System tab cells/controls, analysis flow + Extracted chips, Settings move, carry-over (event/note accept, Task Detail save, priority fallback, preflight). Deferred by user: board drags + Explorer windows (human-visual) and the disruptive `:8081` squatter test.

1. **Board:** drag a card into **In Progress** → `task_update_status('in_progress')` persists across reload; To Do/Done/Archived drags + Restore still work; four columns fit without horizontal overflow at 1400×900.
2. **Analysis flow:** keystroke in an open journal entry → strip shows `Queued → Analyzing… (elapsed ticks) → Done` with the entry title; chat history intact; analysis card appends below with an **Extracted** section showing structured chips (no `[object Object]` anywhere); proposal cards appear and Create/Dismiss work for a **task** (title clean, due/priority preserved).
3. **Live Analyze button:** `#btn-analyze` on a dirty entry saves first, then the strip runs; on a brand-new unsaved entry it no-ops without error.
4. **System tab:** all 14 runtime cells populate (no permanent `—`); `analysis_statuses` + `models` visible; Engines → Health updates latencies; Start/Stop/Restart per service behave; Analysis queue lists rows (seed a failure if needed) and Retry moves `failed → queued`; **Backup now** reports a produced path (verify the file exists); **Open data dir** and **Open log file** open Explorer; SQL runner: `SELECT COUNT(*) FROM tasks` returns rows, `UPDATE tasks SET title='x'` is rejected with the read-only error; log tail renders with the level filter.
5. **Settings after the move:** sections = LLM Engine · System Check · Vault & Security · Activity Log · Development & Debugging; Activity Log still ticks; no `rt-*`/`log-tail` elements remain in Settings; `node --check` clean.
6. **Carry-over from the fix pass (same session):** accept an **event proposal** and a **note proposal** in chat (no FOREIGN KEY error, migration 016); open + save **Task Detail** (no `missing field 'archived'`); **Run System Check** reports embed-role; controlled `:8081` squatter → explicit squatter message, no 501 spam in `arnold-debug.log`.

- [x] **Step 6: Self-Review Notes + spec-coverage map + docs commit**

Complete the writing-plans self-review — answer in this section of the plan (update it in place):

**Spec-coverage map:**

| Spec section | Task(s) |
|---|---|
| 2.1 In Progress column | 1 |
| 2.2 Unified analysis rendering (`[object Object]`) | 3 (emission), 5 (renderer), 6 (card) |
| 2.3 Analysis status strip | 3 (Processing event), 4 (`analysis_requeue`), 6 (strip) |
| 2.4 System tab + 4 commands | 4 (commands), 7 (shell/move), 8 (controls) |
| 2.5 Naming conventions + guard | 2 (fix + harness), 3 (serde test) |
| 2.6 D-144..D-148 | 9 (AGENTS.md) |
| §3 harnesses / gates / E2E | per-task gates + 9 Steps 3-5 |
| §4 out of scope | nothing built (List view dormant, no Activity-Log move, no SQL writes) |

**Self-review checklist (writing-plans):**
- Scope: every spec 2.x row maps to ≥1 task above; no task invents work outside the spec. ✅ — Tasks 1–8 cover 2.1–2.5 + the guard; Task 9 documents 2.6 (D-144..D-148). Nothing built outside §2 (List view dormant, no Activity-Log move, no SQL writes).
- Placeholders: `grep -n "TODO\|TBD\|FIXME" docs/superpowers/plans/2026-10-08-system-tab-board-analysis.md` → no matches; no code block contains a placeholder. ✅
- Exactness: file paths, line anchors, command names, and wire keys re-verified against the repo at run time (`AppDiagnostics` snake keys, `provision_model_cmd`/`restart_engine`/`stop_engine` signatures, `EngineHealth` field names, nav, Runtime block). ✅
- Gates: every task has a Run → Expected block; Task 9 re-ran the full battery (see below). ✅
- Executable by a fresh subagent: each step carried complete code, not descriptions. ✅

**Gates result (Step 3 re-run after the E-1/E-2 fix):**
- `cargo fmt --check` → clean ✅
- `cargo check` → 0 errors, exactly the 6 pre-existing warnings ✅
- `cargo test` → **44 passed; 0 failed** (41 at plan-write time + 3 from this fix pass: `db::analysis_queue::tests::count_all_counts_every_persisted_row`, `ai::client::tests::stalled_generation_times_out_instead_of_hanging`, `ai::client::tests::request_timeout_is_configured`) ✅
- `find src/js -name "*.js" -exec node --check` → no output ✅
- 6 node harnesses (mood, proposals, chat-format, board, ipc-conventions, analysis-view) → all green ✅
- `git diff --check` → clean (CRLF warnings only) ✅
- `grep -rn "card.draggable" src/js/` → no assignments (only the D-141 prohibition comment at `tasks.js:90`) ✅

**Release build (Step 4):** `cargo tauri build --bundles msi nsis` → fresh MSI + NSIS bundles produced (rebuilt again after the E-1/E-2 fix). ✅

**Live E2E (Step 5) — results:**
- 4. **System tab:** all 14 runtime cells populate (no permanent `—`); `analysis_statuses` + `models` visible; Engines shows `running` + pids; Analysis queue lists rows; **Backup now** returned a produced path (`…/backups/arnold_backup_20261020_224440.db`); SQL runner: `SELECT` returns rows, `UPDATE` rejected with the read-only error (D-147); Settings sections = LLM Engine · System Check · Vault & Security · Activity Log · Development & Debugging. ✅ (Open data dir / Open log Explorer windows — human visual, deferred.)
- 5. **Settings after the move:** sections correct; no `rt-*`/`log-tail` elements remain in Settings; `node --check` clean. ✅
- 2. **Analysis flow:** keystroke/save → strip `Queued → Analyzing… → Done` with entry title; analysis card appends (chat intact) with an **Extracted** section (8 proposal cards via `proposalCardHtml`, no `[object Object]`). ✅
- 6. **Carry-over:** event + note proposal **accept** succeed (no FOREIGN KEY error — migration 016 good on the real encrypted DB); Task Detail save with `archived` stripped succeeds (`#[serde(default)]`); priority fallback (invalid → `medium`); `preflight_run` 6/6 incl. embed-reachable. ✅ (Controlled `:8081` squatter test — deferred as disruptive; board drags + Explorer windows — deferred as human-visual.)
- 1 / 3. Board drags (In Progress/To Do/Done/Archived + Restore) and the live `#btn-analyze` button are the remaining human-visual checks (deferred by user decision).

**Defects found and fixed during live E2E (this session):**
- **E-1 (HIGH):** the analysis worker could hang indefinitely — `LlamaClient` used `reqwest::Client::new()` with **no request timeout**, so a stalled generation awaited forever (status stuck `processing`, strip stuck `is-analyzing`, nothing logged). Fixed by building the client with `.timeout(AI_REQUEST_TIMEOUT_SECS = 300)` (new `pub const` + `LlamaClient::with_timeout` for tests); regression test uses a black-hole TCP server. Re-verified live (entry reached `done`).
- **E-2 (MEDIUM):** the System tab's runtime "Analysis queue" cell read the in-memory deque (showed 0) while the queue card read the persisted table (20). Fixed by adding `db::analysis_queue::count_all` and sourcing `app_diagnostics.analysis_queue_len` from it; regression test added. Re-verified live (`rt_queue_len == db_rows`).

Then commit the docs + the fix (plan file is gitignored `*.md` → force-add):

```bash
git add -f CHANGELOG.md AGENTS.md docs/superpowers/plans/2026-10-08-system-tab-board-analysis.md
git add src-tauri/src/ai/client.rs src-tauri/src/db/analysis_queue.rs src-tauri/src/lib.rs
git commit -m "docs: D-144..D-148 decisions, changelog, and implementation plan for the system-tab pass"
```

**Project status (one sentence):** all four spec items are implemented with 44 Rust tests + 6 node harnesses green, the release bundles rebuilt, and the live E2E checklist verified except the human-visual board-drag/Explorer checks and the disruptive `:8081` squatter test.




