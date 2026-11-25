# Arnold — Workflow Reference

> Single source of truth for every end-to-end workflow in Arnold (user-facing and developer-facing). All paths, commands, and event names match the current codebase on `feat/gpu-default-requirements` @ `45b7170`.

---

## Table of Contents

1. [End-User Workflows](#end-user-workflows)
   - [1.1 First Run & Vault Unlock](#11-first-run--vault-unlock)
   - [1.2 Onboarding (Welcome + Done)](#12-onboarding-welcome--done)
   - [1.3 Daily Journaling](#13-daily-journaling)
   - [1.4 Journal Analysis → Proposals](#14-journal-analysis--proposals)
   - [1.5 Proposal Accept / Dismiss](#15-proposal-accept--dismiss)
   - [1.6 Task Management](#16-task-management)
   - [1.7 Tasks Calendar](#17-tasks-calendar)
   - [1.8 Mood Tracking](#18-mood-tracking)
   - [1.9 AI Chat (Companion)](#19-ai-chat-companion)
   - [1.10 Events & Reminders](#110-events--reminders)
   - [1.11 Memory / Knowledge](#111-memory--knowledge)
   - [1.12 Settings — AI Features Toggle](#112-settings--ai-features-toggle)
   - [1.13 Settings — LLM Engine](#113-settings--llm-engine)
   - [1.14 Settings — System Check (Preflight)](#114-settings--system-check-preflight)
   - [1.15 System Tab](#115-system-tab)
   - [1.16 Vault Password Change](#116-vault-password-change)
   - [1.17 Backup & Restore](#117-backup--restore)
2. [Developer Workflows](#developer-workflows)
   - [2.1 Local Development](#21-local-development)
   - [2.2 Build (MSI + NSIS)](#22-build-msi--nsis)
   - [2.3 Verification Suite](#23-verification-suite)
   - [2.4 Testing on a Clean Machine](#24-testing-on-a-clean-machine)
   - [2.5 Committing Work](#25-committing-work)
   - [2.6 Pushing to GitHub](#26-pushing-to-github)
3. [Event / Data Flow Reference](#event--data-flow-reference)
4. [Open Gates (Manual)](#open-gates-manual)

---

## End-User Workflows

### 1.1 First Run & Vault Unlock

1. Launch `Arnold.exe`.
2. App resolves data dir: `<exe_dir>/data` (override via `ARNOLD_DATA_DIR` env var).
3. SQLite DB opens; migrations run automatically (001–019).
4. If no vault password set → **Unlock Your Vault** modal appears.
5. User enters password → `unlock_vault` command → SQLCipher `PRAGMA key` applied → pool opens.
6. `OnboardingRequired` event fires if `onboarding_complete` setting is empty.
7. App emits `StartupComplete`; tab views load (Journal, Tasks, Mood, System, Settings).

**Key files:** `src-tauri/src/db/init.rs`, `src-tauri/src/db/paths.rs`, `src-tauri/src/db/migrations.rs`, `src/js/app.js`

### 1.2 Onboarding (Welcome + Done)

Reduced to 2 steps (AI setup moved to Settings):

1. **Pane 0 — Welcome:** requirements table (WebView2, VC++ runtime, chat model, embed model) + welcome copy.
2. **Pane 1 — Done:** completion screen.
3. On finish/skip → `onboarding_complete` set to ISO timestamp → `OnboardingRequired` gate cleared.

**Key files:** `src/js/onboarding.js`, `src/js/requirements.js`, `src/index.html`

### 1.3 Daily Journaling

1. User opens Journal tab → `VirtualList` renders entries with keyset cursor pagination (`created_at < :cursor`).
2. User writes plain-text entry in the textarea (no Markdown — D-07/D-42).
3. On save → `journal_create` / `journal_update` → DB write → `journal_analysis_queued` event fires **immediately** (before 3s debounce).
4. Analysis worker picks up the entry → runs LLM analysis → emits `JournalAnalysisProcessing` → `AiProposalsReady { conversation_id: entry_id, count }`.
5. Status strip in AI sidebar shows progress; on completion, "Done" + "Extracted" chips appear.

**Key files:** `src/js/tabs/journal.js`, `src-tauri/src/ai/analysis.rs`, `src/js/ai-sidebar.js`

### 1.4 Journal Analysis → Proposals

1. Analysis worker extracts tasks, facts, events, notes from the entry.
2. `RawAnalysis` parsed via `clean_json_string` (strips markdown fences, extracts `{...}`).
3. Proposed items written to `proposed_inbox` table.
4. `AiProposalsReady` event emitted with count.
5. AI sidebar renders:
   - **Tasks** as interactive proposal cards (title, due_date, priority)
   - **Notes/facts/events** as a text summary ("I've saved N notes and M facts to memory")
6. User reviews each proposal in the chat sidebar or Review Inbox.

**Key files:** `src-tauri/src/ai/analysis.rs`, `src/js/analysis-view.js`, `src/js/proposals.js`

### 1.5 Proposal Accept / Dismiss

1. User clicks **Create** (accept) or **Dismiss** on a proposal card.
2. Frontend calls `proposed_action(id, "accept"|"dismiss")`.
3. Backend transaction:
   - **Accept (task):** creates new task OR mutates existing (update/complete) via `apply_task_proposal`; routes `archived` status through `update_task_status`.
   - **Accept (fact/note/event):** writes to knowledge_items / events / notes.
   - **Dismiss:** marks proposal `dismissed` (no DB write).
   - Idempotent: repeated accept is no-op; dismissed cannot be accepted.
4. On task mutation → `TaskChanged { task_id, action }` event emitted.
5. Tasks tab auto-refreshes via `handleTaskChanged` (toast + re-fetch).
6. Proposal marked `accepted` with `accepted_task_id`.

**Key files:** `src-tauri/src/lib.rs` (proposed_action), `src/db/tasks.rs` (apply_task_proposal), `src/js/tabs/tasks.js`

### 1.6 Task Management

**Views:** List / Board (Kanban) / Calendar.

**Board columns:** To Do → In Progress → Done → Archived (D-143/D-144).
- Drag-and-drop via SortableJS. In-column reorder is DOM-only (no sort column).
- `card.draggable` is NOT set (forces pointer-based DnD, not native HTML5).

**Operations:**
- Create task (title, priority, due_date, subtasks up to 2 levels, tags, labels, category, project, energy, context)
- Update task (title, priority, due_date, status, archived flag)
- Complete task → status `done`, sets `completed_at`
- Archive → status `done` + `archived=1` (recurrence skipped); Restore clears flag
- Delete (soft delete) → trash
- Priority normalization: trim/lowercase, fallback to `medium` with warning
- Subtask depth max 2 (enforced in `task_create` handler)

**Key files:** `src/js/tabs/tasks.js`, `src/js/tabs/board-model.js`, `src-tauri/src/db/tasks.rs`

### 1.7 Tasks Calendar

1. Month grid + Unscheduled tray.
2. Drag tray→day sets `due_date`; day→day reschedules; day→tray clears.
3. Chip click opens Task Detail panel.
4. `+N more` chip for overflow days.
5. Day keys are local `YYYY-MM-DD` (never `toISOString().slice`).
6. Zero backend change — reuses `task_update` / `task_update_status`.

**Key files:** `src/js/tabs/tasks-calendar.js`

### 1.8 Mood Tracking

1. **Dual source:** manual check-ins (`mood_checkins` table) + AI-inferred dominant mood from `journal_entries.emotions`.
2. Manual wins on conflict (upsert via `ON CONFLICT DO UPDATE`).
3. Month grid view + Chart.js trend line.
4. `mood_checkin` / `mood_list` / `mood_delete` commands.
5. Mood context (last 7 days) injected into AI system prompt as `## Mood Context`.

**Key files:** `src/js/tabs/mood.js`, `src-tauri/src/db/mood.rs`

### 1.9 AI Chat (Companion)

1. Right sidebar (420px, always mounted, collapsible per-tab).
2. User sends message → `ai_chat(conversation_id, message)` → returns conversation_id.
3. System prompt composed from 8 blocks (persona, mood context, tasks, events, memory).
4. LLM streams tokens via SSE → `AiToken` / `AiReasoningToken` events.
5. Tool calls: assistant emits `tool_calls` with `id` → tool result carries matching `tool_call_id` → both persisted (`ai_messages.tool_call_id`, migration 017) and replayed via `replay_messages`.
6. Confirmation-gated mutations: AI proposes → user accepts/dismisses in Review Inbox.
7. All AI writes require explicit YES/NO with 300s timeout auto-cancel.

**Key files:** `src/js/ai-sidebar.js`, `src-tauri/src/ai/client.rs`, `src-tauri/src/ai/tools.rs`, `src-tauri/src/ai/prompt.rs`

### 1.10 Events & Reminders

1. Events stored with RFC3339 `start_at` / `end_at`, recurrence rules, reminders.
2. AI can create/update/delete events via proposals (requires real `id` from `list_events`).
3. Scheduler polls every 60s for reminders, 3600s for recurrence (skips missed occurrences).
4. `list_events` tool (read-only) for AI context.
5. Event links in chat: `[Event: Title](event:ID)` → opens Event Detail panel.

**Key files:** `src-tauri/src/db/events.rs`, `src-tauri/src/scheduler/reminders.rs`, `src-tauri/src/scheduler/recurrence.rs`

### 1.11 Memory / Knowledge

1. Knowledge items: facts, notes, information (onboarding persona answers stored as `kind=information`, `created_by=onboarding`, `confidence=1.0`).
2. AI tools: `list_memory`, `create_memory`, `update_memory`, `delete_memory`, `link_memory` (all mutations require confirmation).
3. Semantic search: journal vectors (embeddings) + keyword recall.
4. Memory UI: sort by update time; prompt ranking separate.

**Key files:** `src-tauri/src/db/knowledge.rs`, `src-tauri/src/ai/keywords.rs`

### 1.12 Settings — AI Features Toggle

1. Master toggle: `ai_enabled` setting (default `false`).
2. When disabled: engine provisioning skipped, AI UI hidden, "AI features are disabled" notice shown.
3. When enabled: triggers engine provisioning, reveals AI config (engine, models, GPU).
4. Onboarding no longer includes AI steps — all AI config lives here.

**Key files:** `src/js/tabs/settings.js`, `src-tauri/src/lib.rs` (conditional provisioning)

### 1.13 Settings — LLM Engine

1. Engine path: bundled `engines/llama-server.exe` preferred; saved override supported.
2. Model import: user imports `.gguf` files (chat + embed roles).
3. GPU layers: `auto` (default) / `all` / `0–400`; normalized with fallback to `auto` + warning.
4. Context size: default 16384.
5. Chat port 8080, embed port 8081.
6. Inline status: `Current: GPU - N/M layers offloaded` (or partial/CPU).
7. Restart/Stop reaps stray `llama-server.exe` processes on ports.

**Key files:** `src-tauri/src/ai/engine.rs`, `src-tauri/src/ai/probe/llama_cpp.rs`, `src-tauri/src/ai/ports.rs`

### 1.14 Settings — System Check (Preflight)

6 checks: data-dir writability, DB open, VC++ runtime, GPU availability, chat model, embed model. Returns `PreflightReport` (healthy/warnings). Triggered by `#btn-run-system-check`.

**Key files:** `src-tauri/src/preflight.rs` (or lib.rs), `src/js/tabs/settings.js`

### 1.15 System Tab

- Runtime grid: data_dir, db_path, db_size_bytes, log_path, journal_entries, memory_facts, analysis_queue_len, models, analysis_statuses.
- Engines card: per-service GPU/partial/CPU row + requirements table.
- Queue card: failed-first, Retry → `analysis_requeue`.
- DB backup: `backup_now` + Open data dir.
- SQL Runner: read-only (`SELECT`/`WITH`/`PRAGMA`/`EXPLAIN`, 500-row cap, comment-stripped keyword gate + `sqlite3_stmt_readonly` backstop).
- Log viewer: Open log (fixed to `d.log_path`).

**Key files:** `src/js/tabs/system.js`, `src/views/system.html`

### 1.16 Vault Password Change

1. Requires unlock + current password verification (`VaultInvalidPassword` on mismatch).
2. `PRAGMA rekey` on keyed connection (via `query_row` — returns a row).
3. AES-GCM content re-encrypted with new key (best-effort, skip failures).
4. Pool/mem-mirror/graph swapped to new key LAST.
5. Same salt retained (D-114).

**Key files:** `src-tauri/src/lib.rs` (change_vault_password)

### 1.17 Backup & Restore

- Backup on open + on close + scheduled (D-99).
- Uses `rusqlite::backup::Backup` API (never `fs::copy` — D-98).
- JSON export + ZIP + import.
- Plaintext→SQLCipher migration runs in `unlock_vault` (`ensure_encrypted`).

**Key files:** `src-tauri/src/backup/manual.rs`, `src-tauri/src/backup/auto.rs`, `src-tauri/src/backup/export.rs`

---

## Developer Workflows

### 2.1 Local Development

```bash
# Prerequisites: Rust 1.77+ MSVC, Node.js, WebView2, VC++ runtime
# Stage the llama.cpp runtime at repo root:
#   llama-b10952-bin-win-cuda-x64/

cargo tauri dev
```

Data dir: `<exe_dir>/data` (or `ARNOLD_DATA_DIR` override for tests).

### 2.2 Build (MSI + NSIS)

```bash
cargo tauri build --bundles msi nsis
powershell -ExecutionPolicy Bypass -File tools/verify-build.ps1
```

- Compile: ~3 min. Bundling: ~2 min (WebView2 offline installer download on first run).
- Output: `src-tauri/target/release/bundle/msi/Arnold_0.1.0_x64_en-US.msi` (~731 MiB) + `nsis/Arnold_0.1.0_x64-setup.exe` (~698 MiB).
- Runtime source folder is gitignored — must be staged per build workspace.

### 2.3 Verification Suite

```bash
# Rust
cargo test --manifest-path src-tauri/Cargo.toml      # 123 tests
cargo check --manifest-path src-tauri/Cargo.toml      # 0 errors (7 dead-code warnings baseline)
cargo fmt --check --manifest-path src-tauri/Cargo.toml

# Frontend (Node harnesses)
Get-ChildItem tools/js-checks -Filter *.test.mjs | ForEach-Object { node $_.FullName }
```

Harnesses: `analysis-view`, `board-model`, `chat-format`, `components`, `design`, `engine-status`, `ipc-conventions`, `mood`, `proposals`, `requirements`, `task-tags`.

### 2.4 Testing on a Clean Machine

1. Install MSI on a machine without VC++ runtime / CUDA → verify `msvc_runtime_present()` detection.
2. Run `tools/smoke-check.ps1` (process, :8080/:8081 health, PIDs, log-pattern scan).
3. Walk `docs/testing/2026-10-29-e2e-checklist.md` (70 items).

### 2.5 Committing Work

- Run full verification suite first (2.3).
- Conventional commit messages.
- Update `AGENTS.md` + `CHANGELOG.md` for notable changes.
- Never commit the runtime source folder or build artifacts.

### 2.6 Pushing to GitHub

```bash
git push -u origin <branch>
```

- Feature branches: `feat/<name>`.
- Do not merge to `main` until live GPU smoke test passes.
- GitHub Releases for installers (not committed to git).

---

## Event / Data Flow Reference

### AppEvent channel (`"app_event"`, single listener in `app.js`)

| Event | Payload | Trigger |
|---|---|---|
| `JournalAnalysisQueued` | `entry_id` | On journal save (immediate, before debounce) |
| `JournalAnalysisProcessing` | `entry_id` | Worker starts analysis |
| `JournalAnalysisCompleted` | `entry_id`, `result` | Analysis done |
| `JournalAnalysisError` | `entry_id`, `error` | Analysis failed |
| `AiProposalsReady` | `conversation_id`, `count` | After proposed_inbox rows persisted |
| `TaskChanged` | `task_id`, `action` | On proposal accept (create/update/complete) |
| `OnboardingRequired` | — | Startup when `onboarding_complete` unset |
| `ModelProvisioned` | `model` | Provisioning success |
| `ModelProvisionFailed` | `model`, `reason` | Provisioning failure |
| `EngineRuntimeStatus` | `chat`, `embed` | After engine lifecycle change |
| `AiToken` / `AiReasoningToken` | `conversation_id`, `token` | LLM streaming |
| `AiResponseComplete` | `conversation_id` | Stream finished |
| `LogEvent` | `timestamp`, `level`, `source`, `message` | Tracing output |

### Data flow: Journal → Task

```
journal_create/update
  → journal_analysis_queued (immediate)
  → analysis worker (3s debounce)
  → LLM analysis (non-streaming JSON mode)
  → proposed_inbox rows (tasks, facts, events, notes)
  → AiProposalsReady { count }
  → AI sidebar: tasks as cards, others as text summary
  → user accepts/dismisses
  → proposed_action → DB write → TaskChanged event
  → tasks tab auto-refresh
```

### Data flow: AI Chat tool round-trip

```
ai_chat(conv_id, message)
  → system prompt (8 blocks) + replayed history (tool_calls + tool_call_id)
  → LLM streams → tool_call with id
  → ToolExecutor executes → ToolResult (JSON envelope, truncated)
  → tool result persisted with tool_call_id
  → LLM continues → final reply
  → AiResponseComplete
```

---

## Open Gates (Manual)

These require real hardware or human verification — not automatable in CI:

1. **Live GPU smoke test** — NVIDIA GPU + CUDA drivers; verify System tab rows, Settings GPU line, CPU-fallback toast, port-clearing.
2. **MSI smoke test on clean machine** — no VC++ runtime, no CUDA; verify `msvc_runtime_present()` detection and graceful failure.
3. **Live multi-turn E2E** — AI tool-calling against a real Qwythos chat template (multi-turn tool round-trip).
4. **Visual pass** — premium UI system (animated icons, elevation, `hj-button` variants, `hj-metric-card.setValue` count-pop).
5. **Board drags + Explorer windows** — human-visual verification of drag-and-drop and "Open data dir"/"Open log" buttons.
6. **Disruptive `:8081` squatter test** — controlled test of embed-port conflict messaging.

---

## Key Decisions (D-160 through D-167)

| Decision | Summary |
|---|---|
| D-160 | Offline role contract: any instruct-tuned GGUF accepted; no URL-fetch/web-search tool; event/note mutations proposal-only |
| D-161 | `EngineProbe` trait — only place that knows engine CLI flags/log format |
| D-162 | GPU layers default `auto`; VC++ runtime detected, never bundled |
| D-163 | Onboarding reduced to welcome + done; AI setup moved to Settings |
| D-164 | `ai_enabled` setting (default false) + conditional provisioning + AI UI hide when disabled |
| D-165 | AI sidebar: tasks-only cards, notes/facts/events as text summary |
| D-166 | `task_changed` event → tasks tab auto-refresh on AI proposal accept |
| D-167 | Notes/facts/events are internal (saved without interactive cards); counts only in analysis card |

---

*Generated from `AGENTS.md` + codebase state on `feat/gpu-default-requirements` @ `45b7170` (build verified 2026-11-14).*