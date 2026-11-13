# Arnold — Manual E2E Checklist (build of 2026-10-29)

Everything below is the accumulated verification debt from the last four code passes
(rebrand + BYOM, Phase 1 offline AI contract, agent-architecture remediation, premium UI
system). Work top to bottom; each item is independent unless it says "restart first".

Report format per item: **PASS / FAIL / BLOCKED** + one sentence + log line if it failed.

---

## 0. What is staged for you

| Thing | Path |
|---|---|
| Release exe | `src-tauri\target\release\arnold.exe` |
| MSI | `src-tauri\target\release\bundle\msi\Arnold_0.1.0_x64_en-US.msi` |
| NSIS | `src-tauri\target\release\bundle\nsis\Arnold_0.1.0_x64-setup.exe` |
| Staged llama.cpp runtime (build 10952) | `llama-b10952-bin-win-cuda-x64\` |
| Chat GGUF (Qwythos, 5.24 GiB) | `llama-b10952-bin-win-cuda-x64\models\Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M.gguf` |
| Embed GGUF (Nomic, 0.32 GiB) | `llama-b10952-bin-win-cuda-x64\models\nomic-embed-text-v2-moe.Q4_K_M.gguf` |
| Engine binary | `llama-b10952-bin-win-cuda-x64\llama-server.exe` |
| Fresh test profile | `test-data\fresh\` (empty, isolated data root) |
| Upgrade test profile | `test-data\upgrade\` (copy of `%APPDATA%\ArnoldJournals\arnold.db*` renamed to `arnold.db*`, both GGUFs hardlinked into `models\{chat,embed}`) |
| Launcher | `tools\test-run.ps1 -Profile fresh \| -Profile upgrade` |

`test-data\*` is isolated via `ARNOLD_DATA_DIR`, so nothing here touches your real app data.
The upgrade profile is a **copy** — your real `%APPDATA%\ArnoldJournals` is untouched.

Launch:

```powershell
.\tools\test-run.ps1 -Profile upgrade     # Run A: legacy vault, migrations 015/016/017
.\tools\test-run.ps1 -Profile fresh       # Run B: clean first-run + onboarding
```

Log for each profile: `test-data\<profile>\arnold-debug.log`
DB for each profile: `test-data\<profile>\arnold.db` (+ `.salt`, `-wal`, `-shm`)

**Read-only DB verification** goes through **System → SQL Runner** (D-147 blocks writes):
`SELECT name FROM sqlite_master WHERE type='table';`
`SELECT version FROM schema_migrations ORDER BY version;`

---

## 1. Run A — upgrade path (legacy encrypted DB)

Launch `.\tools\test-run.ps1 -Profile upgrade`, unlock with your **existing** vault password.

- [ ] **A1 Unlock succeeds with the old password** (salt + SQLCipher key survive the `arnold.db` → `arnold.db` rename).
- [ ] **A2 No data loss** — journal entries visible, tasks still on the board, mood history present.
- [ ] **A3 Migrations reached the top** — System → SQL Runner: `SELECT version FROM schema_migrations ORDER BY version;` returns `... 015, 016, 017` (017 = `ai_messages.tool_call_id`).
- [ ] **A4 Analysis queue survives** — System → Analysis Queue counts match your real backlog; no `FOREIGN KEY constraint failed` in the log.
- [ ] **A5 Log is clean of the four historic incidents:**

```powershell
Select-String -Path test-data\upgrade\arnold-debug.log -Pattern 'FOREIGN KEY|missing field .archived.|CHECK constraint failed: priority|501'
```

Expected: **no matches**. Any match = FAIL, paste the line.

---

## 2. Run B — fresh first-run + onboarding

Launch `.\tools\test-run.ps1 -Profile fresh`.

- [ ] **B1 Vault create** — password dialog appears, password + confirm, no "file is not a database".
- [ ] **B2 Onboarding wizard auto-opens** (fresh `onboarding_complete` setting is empty).
- [ ] **B3 Engine step** — accept the bundled engine (`llama-server.exe`) or pick the staged one; path persists (`llama_exe_path`).
- [ ] **B4 Chat GGUF import** — pick the Qwythos GGUF → "Imported Qwythos…".
- [ ] **B5 Chat-role guard (negative)** — *restart the wizard only if needed*: try importing the **Nomic** GGUF as the **chat** model → must be **rejected** ("embedding model" / unsupported). Then import Qwythos again.
- [ ] **B6 Embed GGUF import** — pick Nomic → imported.
- [ ] **B7 Persona step** — answer the six questions (or Skip). Answers become `knowledge_items` with `created_by = onboarding`, `confidence = 1.0`.
  Verify: System → SQL Runner → `SELECT title, category, created_by FROM knowledge_items;`
- [ ] **B8 System check** — 6 checks render; engine check reaches PASS/WARN (not a hang). Both servers start.
- [ ] **B9 Finish** — data dir path shown, `onboarding_complete` written; wizard does not reappear after restart.

---

## 3. Vault lifecycle (Run B)

- [ ] **C1 Wrong password rejected** (`VaultInvalidPassword`, no crash, no data loss).
- [ ] **C2 Change password** — Settings → Vault: current/new/confirm → success toast.
- [ ] **C3 Restart** → unlock with the **new** password works, **old** password rejected (salt is not rotated, D-114).
- [ ] **C4 Post-rekey write** — create a journal entry after the change and reopen it (AES-GCM content re-encrypted).

---

## 4. Journal → analysis pipeline (Run B)

- [ ] **D1 Save an entry with a clear commitment**, e.g. *"I need to call the dentist tomorrow afternoon and buy cat food."*
- [ ] **D2 `journal_analysis_queued` fires immediately** (status strip appears before the 3 s debounce).
- [ ] **D3 Status strip** goes Processing → **Done**; elapsed ticker moves; Retry button appears only on failure.
- [ ] **D4 Extracted chips render** (no `[object Object]`) — D-145.
- [ ] **D5 Proposal cards appear in the assistant sidebar** for the extracted task(s), each with **Create** as the confirm label.
- [ ] **D6 Accept one** → task appears on the Tasks board (`todo`), card flips to accepted.
- [ ] **D7 Repeat-accept** the same card (if the button is still reachable) → **no duplicate task** (idempotent, D-160).
- [ ] **D8 Dismiss a different card** → nothing written.
- [ ] **D9 Priorities** — the accepted task's priority is one of `low|medium|high|urgent`; any bogus AI priority is normalized to `medium` with a `tracing::warn` (not a CHECK failure).

---

## 5. Proposal-only AI writes (D-19 / D-160) — the highest-risk area

Chat prompt to use for each row, then accept the card. Confirm labels come from the payload type.

| # | Ask the assistant | Card label | Expect |
|---|---|---|---|
| E1 | "Add an event called Dentist on 2026-11-03 at 14:00" | **Create** | `events` row exists; no FK error |
| E2 | "Move that dentist event to 15:30" | **Update** | same row updated, not a new row |
| E3 | "Cancel the dentist event" | **Delete** | row gone |
| E4 | "Save a note: vet records are in the blue folder" | **Create** | `notes` row exists |
| E5 | "Change that note to say the green folder" | **Update** | same row updated |
| E6 | "Delete that note" | **Delete** | row gone |
| E7 | "Rename task X to Y" (real task from `list_tasks`) | **Update** | the **existing** task is patched — **no duplicate** (D-154) |
| E8 | "Mark task X done" | **Complete** | status `done`, appears in Done column |
| E9 | "Archive task X" | **Update/Complete** | `status = done` **and** `archived = 1` → Archived column |

- [ ] **E10 Dismiss before accept** → never applies.
- [ ] **E11 Accept, then dismiss** → writes nothing (no delete/unarchive).
- [ ] **E12 Stale ID** — ask the AI to update an event/note that was deleted in between → clean `NotFound` message, no panic, no partial write.

SQL spot-checks: `SELECT id,title,start_at FROM events;` · `SELECT id,content FROM notes;` · `SELECT id,title,status,archived FROM tasks;`

---

## 6. Tasks surface

- [ ] **F1 View toggle** — List / Board / Calendar all render.
- [ ] **F2 Board columns** — To Do · In Progress · Done · Archived (4 columns, D-144).
- [ ] **F3 Drag** To Do → In Progress → Done → Archived; each drop persists (reload the tab to confirm).
- [ ] **F4 Restore** from Archived → back to an active column; `archived` cleared.
- [ ] **F5 Task Detail** opens and **saves** — no `missing field 'archived'` (the 2026-10-06 incident).
- [ ] **F6 Subtask depth** — a third level is refused (two levels max).
- [ ] **F7 Calendar** — drag a tray chip onto a day (sets `due_date`), day → day (reschedules), day → tray (clears); chip click opens Task Detail; local `YYYY-MM-DD` keys (no timezone day shift); `+N more` overflow.
- [ ] **F8 Recurrence** — a repeating task produces the next instance without duplicating on restart.

---

## 7. Mood

- [ ] **G1 Check-in chips** write today's mood; grid shows the cell.
- [ ] **G2 AI-inferred moods** from entry `emotions` fill days with no manual check-in (manual wins).
- [ ] **G3 Trend chart** renders (Chart.js, offline under CSP).
- [ ] **G4 Delete / merge** a day works and the note field is soft-deleted, not dropped.

---

## 8. AI chat quality (live Qwythos)

- [ ] **H1 Multi-turn tool round-trip** — "what are my open tasks?" → `list_tasks` → accurate answer; then "mark the first one done" → proposal with a **real id**.
- [ ] **H2 No blank bubbles** — tool-call turns do not render empty assistant bubbles.
- [ ] **H3 Thinking panel** — reasoning shows in the amber collapsible block; it is **not** persisted (reload the conversation → gone, D-139).
- [ ] **H4 Reasoning-only turn** — force a turn that produces only reasoning → you get a safe generic reply, never raw reasoning.
- [ ] **H5 Tool-loop exhaustion** — the reply is a user-facing fallback, not a blank persisted row.
- [ ] **H6 Journal recall** — "what did I write about the dentist?" → snippets + **Jump to Source** link opens the entry.
- [ ] **H7 Memory facts** — persona answers and accepted memories are recalled; prompt caps at 5 facts × 80 chars, 3 journal snippets, 5 tasks (M1).
- [ ] **H8 No network** — the prompt says the AI has no internet; asking it to fetch a URL must be refused (no `fetch_url` tool exists).
- [ ] **H9 Error rendering** — kill the engine mid-request → error banner shows text, never HTML injection.

---

## 9. System tab

- [ ] **I1 Runtime grid** — every cell populated (no `—` from snake_case/camelCase drift, D-148): versions, ports, paths, model roles.
- [ ] **I2 AI Engines card** — chat/embed status, ports :8080/:8081, GPU layers, context; Restart Engine works.
- [ ] **I3 Analysis Queue card** — counts match the DB; **failed-first** ordering; Retry requeues.
- [ ] **I4 Database card** — `backup_now` writes a `backups\` file (Backup API, not `fs::copy`); Open data dir / Open log open Explorer at the right paths (the "Open log" bug used `d.log_path`).
- [ ] **I5 SQL Runner** — `SELECT` works (≤500 rows); `INSERT/UPDATE/DELETE/PRAGMA rekey` are **rejected**; a comment-prefixed write is still rejected.
- [ ] **I6 Log Viewer** — tails `arnold-debug.log`, spans get colored classes.

---

## 10. Engines, ports, restart (the 2026-10-06 + 2026-10-29 fixes)

- [ ] **J1 Both servers healthy** — `Invoke-RestMethod http://127.0.0.1:8080/health` and `:8081/health` return OK.
- [ ] **J2 Chat works**, `POST /v1/chat/completions` returns a completion.
- [ ] **J3 Embeddings work** — `POST /v1/embeddings` returns a vector (the `-e` → `--embeddings` flag bug).
- [ ] **J4 Restart Engine leaves no stray child** (2026-10-29 fix) — after restart:
  `Get-Process llama-server | Select-Object Id,StartTime` → exactly 2 processes, the old ones gone.
- [ ] **J5 Port squatter** — disruptive test: start a **chat-mode** server on :8081 manually, then restart engines → the app reports a friendly role/port message and does **not** spam 501 into the log.
- [ ] **J6 Missing model** — point `llama_chat_model` at a file that does not exist → typed `Unavailable` reason, app stays usable.

---

## 11. Premium UI sweep (2026-10-23 pass)

- [ ] **K1 Icons** — `<hj-icon>` renders everywhere (no missing glyphs); hover animates the two-tone icons; `prefers-reduced-motion` disables animation.
- [ ] **K2 Elevation** — one light direction (top-left); cards/popovers/modals step through `--elev-0..3`; no ad-hoc double shadows.
- [ ] **K3 Buttons** — primary/secondary/ghost/danger, disabled + keyboard (Enter/Space) all work on the System engine controls, Settings check rows and Onboarding.
- [ ] **K4 Metric cards** — Insights values count-pop on load; no layout shift.
- [ ] **K5 Fields** — label/help/error on Settings forms; invalid input sets `aria-invalid` + `role="alert"`.
- [ ] **K6 Layout** — no regressions after the button/icon sweep: assistant sidebar collapse state persists across tabs; 1200 px min width holds.
- [ ] **K7 Design guard** — `node tools\js-checks\design.test.mjs` stays green.

---

## 12. Installers

- [ ] **L1 MSI** installs (expect ~731 MiB; it embeds the offline WebView2 runtime).
- [ ] **L2 NSIS** installs (~698 MiB), per-user.
- [ ] **L3 Offline WebView2** — disable networking (or pull the cable) before installing on a machine without WebView2 → setup completes from the bundled runtime, no download attempt.
- [ ] **L4 Installed app** — engine resource resolves from the install dir (`engines\llama-server.exe`), data lands in `<install dir>\data`, first-run onboarding appears.
- [ ] **L5 Uninstall** → no orphan `llama-server.exe`; note whether `data\` is left behind.

---

## 13. Known UX gaps to confirm (not failures, decide if they should be fixed)

- [ ] **M1 Memory tab, Review Inbox, Reports, Events and AI tabs have no visible nav entry** (D-141 hide-not-delete). Confirm this is intended: proposals are handled inline in the assistant sidebar and Memory is only inspectable through System → SQL Runner. If you want them reachable again, that is a small nav change.
- [ ] **M2 Events tab** is still a calendar placeholder — only reachable by `.click()` on a hidden nav item, so effectively dormant.

---

## 14. Sign-off

- [ ] `cargo test` 85/85, `cargo fmt --check` clean, `cargo check` 0 errors (6 baseline warnings), 9/9 Node harnesses — re-run after any code change.
- [ ] Log clean for: `panic`, `FOREIGN KEY`, `501`, `CHECK constraint failed`, `missing field`.
- [ ] Both profiles restart cleanly with no duplicate children and no re-run onboarding.

Fill this in when you are done:

```
A1..A5 : 
B1..B9 : 
C1..C4 : 
D1..D9 : 
E1..E12: 
F1..F8 : 
G1..G4 : 
H1..H9 : 
I1..I6 : 
J1..J6 : 
K1..K7 : 
L1..L5 : 
M1..M2 : 
```
