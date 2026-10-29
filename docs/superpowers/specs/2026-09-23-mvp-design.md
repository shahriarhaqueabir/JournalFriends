# Working MVP — Design Spec

**Date:** 2026-09-23
**Status:** Approved for planning (user-approved design; phases + tests + code relationships incorporated)
**Branch:** `wip/ollama-to-llamacpp-migration`
**Baseline:** head `6c6693c` (uncommitted `src/js/app.js` `loadView` caching change pending decision)
**Related decisions:** D-19, D-23, D-95, D-104, D-112, D-133, D-134–D-136, D-138, D-139
**Relates to:** `src/index.html` (nav + tab views), `src/js/app.js`, `src/js/ai-sidebar.js`, `src/js/tabs/*`, `src/styles/*`, `src-tauri/src/ai/prompt.rs`, `src-tauri/src/ai/analysis.rs`, `src-tauri/src/db/*`, `src-tauri/src/lib.rs`, `src-tauri/src/migrations/*`, `src-tauri/error.rs`, `src-tauri/events.rs`

---

## 1. Problem

The app is feature-complete across 9 sprints (~139 decisions), but there is **no working, verified MVP**. Standing open items: manual in-app smoke never ran (D-139, AI Control Center Phase A), no E2E against a built binary, and the test suite was **deleted** in Sprint 9 (all `#[cfg(test)]` blocks, `tests-e2e/`, CI `cargo test`). The user wants a lean, reliable surface covering seven explicit requirements, shipped and verified, built in phases with unbiased tests.

## 2. Goals

Ship a **working MVP** on the existing codebase that delivers:

1. **AI chat** with a caring, thoughtful, attentive Companion persona grounded in journal, tasks, and mood context.
2. **Journal writing** that works (editor, entries list, save-on-change).
3. **Mood tracker** — dual-source (manual daily check-in + AI-inferred per entry).
4. **Task organiser** that works (list/board/calendar).
5. **Auto mood + task extraction** from journal content, with user confirmation preserved (D-19).
6. **Settings, buttons, functions** that work (LLM Engine card, vault, backup, system check).
7. **Hardened, sanitised, secure** surface (escape-first rendering, CSP, no secrets, no new vulnerable deps).

Deliverable: a **release build** (installer + portable exe) that the user smoke-tests end-to-end on this machine (bundled engine + GGUFs already present in `%APPDATA%/HawkwardJournals/engines|models`).

## 3. Scope

**In scope:** lean tab set (strip-by-hide), Mood tracker (new migration + backend + UI), Companion persona rewrite (`prompt.rs`), in-chat extraction approval cards, rebuild of a test skeleton (Rust regression guards + JS harness), hardening pass, release build.

**Out of scope:** deleting hidden surfaces/code, new AI tools, web search (B-03), personas dropdown in Settings, Events/Reminders UI, Reports/Memory/About surfaces (hidden, code preserved), the AI Control Center identity card UI (hidden; persona facts persist in `knowledge_items` and remain injected).

## 4. MVP Surface & Structure

Sidebar navigation becomes:

- **Workspace:** Journal · Tasks · Mood
- **System:** Settings

Hidden (nav items removed; views + backend remain in code, unreachable — `loadView` fires only on demand): Events, AI (Control Center tab), Memory, Reports, Review Inbox, About.

The right AI chat sidebar stays **always mounted and visible** (D-23/D-78). The pending `loadView` caching change in `src/js/app.js` (uses `dataset.viewLoaded`) is adopted into baseline if Phase 1 gates pass — it is a safe, unrelated optimization.

## 5. Mood Tracker (dual-source)

### 5.1 Data model — migration `014_mood_checkins.sql`

Skewed by the runner (`db/migrations.rs` wraps every migration in a rusqlite transaction and **rejects** explicit `BEGIN/COMMIT` — D-97 handled there), so the SQL file is bare DDL:

```sql
CREATE TABLE IF NOT EXISTS mood_checkins (
  id TEXT PRIMARY KEY,
  mood TEXT NOT NULL,
  note TEXT DEFAULT '',
  checkin_date TEXT NOT NULL,          -- local YYYY-MM-DD (D-133 discipline)
  created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mood_checkins_date ON mood_checkins(checkin_date);
```

One check-in per day; re-check-in **updates in place** (upsert on `checkin_date` via `ON CONFLICT(checkin_date) DO UPDATE`). Registered as `Migration { version: 14, name: "mood_checkins" }` in `MIGRATIONS`.

Allowed moods (chips): `calm, focused, joyful, anxious, frustrated, sad, energetic` (extensible; stored lowercase). Inferred moods come from the existing `journal_entries.emotions` JSON array → dominant mood per day via `json_each` (same pattern as `db/reports.rs::get_mood_trend_report`).

### 5.2 Backend — `src-tauri/src/db/mood.rs` + commands

- `db/mood.rs`: `checkin(conn, mood, note, date) -> Result<()>` (upsert), `list_range(conn, start, end) -> Result<Vec<MoodDay>>`, `delete(conn, date) -> Result<()>`; `merge_dominant(list, journal_dominant)` — manual wins on same day.
- New Tauri commands (private `async fn` per F-011): `mood_checkin`, `mood_list`, `mood_delete`. All use the vault-unlocked pool via `db::pool::with_conn`. Registered in `generate_handler!`.
- Plain query handlers; all return `Result<T, AppError>` per convention.

### 5.3 Frontend — Mood tab

- `#tab-mood` markup in `src/index.html` + nav button.
- New `src/js/tabs/mood.js`: month grid (mirrors Tasks Calendar pattern), a "How are you feeling?" 6-chip quick check-in + optional note + save, this-month trend chart (Chart.js already vendored at `src/assets/libs/chart.umd.min.js`).
- New `src/styles/mood.css`. One consistent mood→color map shared by grid + chart.
- Day display: manual mood chip; AI-inferred shown as a small dot/tooltip when both exist for a day. Local `YYYY-MM-DD` day keys only.

## 6. Companion Persona (rewrite of `ai/prompt.rs`)

Rewrite the persona block to a **Companion**: warm, reflective, attentive. Instructed to:

- Use persona facts from the knowledge base (`knowledge_items`): name, what the user does, typical day, goals, constraints, preferences.
- Notice and gently reflect mood shifts over the week using the injected pulse + dominant moods (and, post-Phase 2, manual check-ins).
- Connect journal→tasks→mood threads (e.g., "this task has been weighing on you since Tuesday").
- Validate feelings, ask open questions, never lecture.

All existing injections preserved: compact tasks (D-94), events next-3-days, 7-day pulse, dominant mood, memory facts, `ai_voice` (Phase A). Signature may gain a mood-context parameter; call sites in `lib.rs` updated. Unit test asserts block order/presence + a context-budget sanity bound (prompt must stay well under 16384, D-93).

## 7. In-chat Extraction Approval

Reuse the existing pipeline untouched: `proposed_inbox` + `proposed_action` (accept/dismiss; already handles task/fact/memory/note item types; pending-count event already emitted at `lib.rs:964`).

- `ai-sidebar.js`: on proposal events, render a **confirm card** inline in the latest AI bubble — “I noticed this — Task: 'Pay electric bill' — [Create] [Dismiss]”.
- Buttons call `proposed_action` with `{ id, action: "accept" | "dismiss" }` → identical backend writes, D-19 preserved.
- Pending-count badge (currently `#inbox-badge`) relocates to the AI chat header.
- All card title/content rendered escape-first via `escapeHtml` (sanitization discipline).

## 8. Settings, Verification, Security

- Settings tab fully functional, untouched: LLM Engine card (status dots, model pickers, GGUF import, exe path, GPU layers, context), Vault (password change), Backup, System check.
- Hardening pass: XSS sweep of all new rendering paths (mood chips, confirm cards, chart labels) escape-first; secret scan; `cargo audit`; CSP dict unchanged; no new deps.

## 9. Phased Delivery & Gates

Each phase independently shippable; each failing gate stops the phase.

- **Phase 1 — Green baseline + test skeleton.** `cargo check` 0 errors, `cargo fmt --check`, `node --check`, release build on current head. Rebuild additive Rust regression guards + run existing `tools/js-checks` harness. Decide adopt/revert pending `app.js` `loadView` change (recommend adopt). **Commit.**
- **Phase 2 — Mood tracker (backend → frontend).** Migration `014` + `db/mood.rs` + 3 commands with DB-level tests (upsert, delete, merge manual-wins). Then Mood tab UI + CSS + nav. JS harness tests for pure logic (day keys, merge). **Commit.**
- **Phase 3 — Companion persona (`prompt.rs`).** Signature-aware rewrite, call-site update, prompt-composition unit test + budget bound. **Commit.**
- **Phase 4 — UI strip + in-chat confirm cards.** Strip nav buttons; wire confirm cards + badge move in `ai-sidebar.js`. JS harness tests for card renderer (escape-first). **Commit.**
- **Phase 5 — Hardening + ship.** XSS sweep, secret scan, `cargo audit`, all gates, fresh **release build** (MSI + NSIS + portable). Final gate: **user smoke test** — Journal → analyze → extraction card → create task → mood views → settings.

## 10. Code Relationship Map (risks & dependencies)

| Change | Depends on | Risks / guards |
|--------|-----------|----------------|
| Migration `014` | Sequential runner; must follow `013` | Order enforced by runner; table auto-encrypted by SQLCipher (no special handling) |
| `db/mood.rs` + commands | vault-unlocked pool (`with_conn`) | Must follow `with_conn` + `Result<T, AppError>`; registered after Phase A commands in `generate_handler!` |
| Mood merge query | `journal_entries.emotions` JSON (exists) | Reuse `json_each` dominant-mood pattern from `db/reports.rs` — do not invent a second one |
| `prompt.rs` persona | `ai_voice` threading (Phase A) + domain injections | Pure change; add signature param carefully — one call site in `lib.rs`; budget test guards bloat |
| Confirm cards | `proposed_inbox` + `proposed_action` (backend untouched) | Frontend-only re-skin; D-19 preserved |
| UI strip | app.js tab router + `loadView` | Removing nav buttons never activates hidden views; identity-card save path dies but persona facts persist in `knowledge_items` (accepted trade-off) |
| New commands | `lib.rs` command block | F-011: private `async fn`, no `pub` in crate root |

## 11. Test Strategy (unbiased)

Sprint 9 deleted the suite; rebuild it **before** feature work so failures are attributable:

1. **Phase 1** establishes the skeleton: additive Rust `#[cfg(test)]` regression guards + the `tools/js-checks` node harness (existing `chat-format.test.mjs` pattern).
2. Each phase adds a **test with the behavior**: DB-level tests for mood upsert/delete/merge; prompt-composition test for persona; JS harness tests for mood/pure logic and the confirm-card renderer.
3. Gates run the full suite each phase: `cargo test` (rebuilding), `cargo check`, `cargo fmt --check`, `node --check`, plus per-phase harness run.
4. Final gate = **human smoke test** on a fresh release build. No "works" claim without it.

## 12. Environment Facts (verified 2026-09-23)

- Bundle: `%APPDATA%/HawkwardJournals/engines/llama-server.exe` + CUDA runtime present.
- Models: chat `Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M.gguf`; embed `nomic-embed-text-v2-moe.Q4_K_M.gguf`.
- Live DB: `%APPDATA%/HawkwardJournals/hawkward.db` (real data, WAL). `data/` in repo is empty (placeholder). Rate: engine verified working 2026-09-19; user confirmed "the AI works" 2026-09-15.
- Toolchain: cargo 1.95.0.
- `docs/superpowers/specs/` is gitignored → doc commits need `git add -f` (F-003 precedent).