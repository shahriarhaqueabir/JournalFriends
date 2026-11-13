# AI Sidebar Streamlining & Optional AI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce onboarding to 2 steps (no AI), move all AI configuration to a dedicated Settings section with an enable/disable toggle, streamline the AI sidebar to show only tasks as interactive cards (notes/facts as text), and auto-refresh the tasks tab when AI creates tasks.

**Architecture:** Onboarding loses all AI steps. Settings gains an "AI Features" section with a master toggle that gates engine provisioning and AI UI visibility. The AI sidebar renderer separates tasks (interactive cards) from notes/facts/events (text summary). A new `task_changed` event auto-refreshes the tasks tab on AI tool calls.

**Tech Stack:** Rust (Tauri v2), vanilla JS/CSS, SQLite

**Spec:** `docs/superpowers/specs/2026-10-30-ai-sidebar-streamlining-design.md`

---

## File Structure

**Modify:**
- `src/index.html` — onboarding panes, settings AI section
- `src/js/onboarding.js` — simplify to welcome + done
- `src/js/tabs/settings.js` — AI toggle + conditional config
- `src/js/ai-sidebar.js` — tasks-only cards, text summary
- `src/js/analysis-view.js` — update renderer
- `src/js/tabs/tasks.js` — listen for task_changed
- `src/js/app.js` — route task_changed event
- `src-tauri/src/lib.rs` — ai_enabled setting, conditional provisioning
- `src-tauri/src/ai/tools.rs` — emit task_changed on mutations
- `src-tauri/src/events.rs` — add TaskChanged variant

---

### Task 1: Simplify onboarding to welcome + done

**Files:**
- Modify: `src/index.html` (onboarding panes)
- Modify: `src/js/onboarding.js`

- [ ] **Step 1: Update onboarding panes in index.html**

Remove panes 1-5 (engine, chat GGUF, embed GGUF, system check, persona). Keep pane 0 (welcome) and pane 6 (done). Update TOTAL_STEPS to 2.

- [ ] **Step 2: Simplify onboarding.js**

Remove all step-specific functions (refreshEnginePath, saveExePath, importModel, runEngineCheck, persistPersona). Keep refreshRequirements, completeOnboarding, showOnboarding, goTo. Update TOTAL_STEPS to 2. Update wire() to only handle next/done.

- [ ] **Step 3: Verify**

Run: `node --check src/js/onboarding.js`
Expected: no output.

- [ ] **Step 4: Commit**

```bash
git add src/index.html src/js/onboarding.js
git commit -m "feat(onboarding): reduce to welcome + done, remove AI steps"
```

---

### Task 2: Add ai_enabled setting and conditional provisioning

**Files:**
- Modify: `src-tauri/src/db/settings.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add ai_enabled seed**

In settings.rs, add `("ai_enabled", "false")` to the seed list.

- [ ] **Step 2: Conditionally provision in lib.rs**

In the startup provisioning path (line ~430), check `ai_enabled` setting. If false, skip provisioning and emit nothing. If true, proceed as before.

- [ ] **Step 3: Add Tauri command for toggling**

Add `ai_enabled_get` and `ai_enabled_set` commands (or reuse setting_get/set).

- [ ] **Step 4: Verify**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && cargo check --manifest-path src-tauri/Cargo.toml`
Expected: all tests pass, 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db/settings.rs src-tauri/src/lib.rs
git commit -m "feat(ai): add ai_enabled setting and conditional provisioning"
```

---

### Task 3: Add dedicated AI Features section to Settings

**Files:**
- Modify: `src/index.html`
- Modify: `src/js/tabs/settings.js`

- [ ] **Step 1: Add AI Features section to index.html**

Add a new section before the LLM Engine section with:
- Master toggle: `<input type="checkbox" id="ai-enabled">`
- Container for AI config (engine, models, GPU) that shows/hides based on toggle

- [ ] **Step 2: Add toggle logic to settings.js**

Add `initAiFeaturesSection()` that:
- Reads `ai_enabled` setting
- Shows/hides AI config container
- On toggle change, persists setting and shows/hides config
- When enabling, triggers engine provisioning

- [ ] **Step 3: Verify**

Run: `node --check src/js/tabs/settings.js`
Expected: no output.

- [ ] **Step 4: Commit**

```bash
git add src/index.html src/js/tabs/settings.js
git commit -m "feat(settings): add dedicated AI Features section with enable toggle"
```

---

### Task 4: Streamline AI sidebar — tasks as cards, notes/facts as text

**Files:**
- Modify: `src/js/analysis-view.js`
- Modify: `src/js/ai-sidebar.js`

- [ ] **Step 1: Update analysis-view.js renderer**

Modify `extractedItemsHtml` to render:
- Tasks as interactive proposal cards (existing behavior)
- Notes, facts, events as a text summary block ("I've saved N notes and M facts to memory")

- [ ] **Step 2: Update ai-sidebar.js**

Ensure the analysis card state machine uses the new renderer. When AI extracts items, show tasks as cards and other items as text.

- [ ] **Step 3: Verify**

Run: `node --check src/js/analysis-view.js && node --check src/js/ai-sidebar.js`
Expected: no output.

- [ ] **Step 4: Commit**

```bash
git add src/js/analysis-view.js src/js/ai-sidebar.js
git commit -m "feat(ai-sidebar): show only tasks as cards, notes/facts as text summary"
```

---

### Task 5: Add task_changed event and auto-refresh

**Files:**
- Modify: `src-tauri/src/events.rs`
- Modify: `src-tauri/src/ai/tools.rs`
- Modify: `src/js/tabs/tasks.js`
- Modify: `src/js/app.js`

- [ ] **Step 1: Add TaskChanged event variant**

In events.rs, add:
```rust
TaskChanged {
    task_id: String,
    action: String, // "created", "updated", "completed", "deleted"
},
```

- [ ] **Step 2: Emit task_changed in tools.rs**

In ToolExecutor, when create_task/update_task/complete_task succeeds, emit TaskChanged event.

- [ ] **Step 3: Listen in tasks.js**

Add event listener that re-fetches tasks when TaskChanged is received.

- [ ] **Step 4: Route in app.js**

Route task_changed event to tasks tab refresh.

- [ ] **Step 5: Verify**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && cargo check --manifest-path src-tauri/Cargo.toml`
Expected: all tests pass, 0 errors.

Run: `node --check src/js/tabs/tasks.js && node --check src/js/app.js`
Expected: no output.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/events.rs src-tauri/src/ai/tools.rs src/js/tabs/tasks.js src/js/app.js
git commit -m "feat(ai): auto-refresh tasks tab on AI tool calls"
```

---

### Task 6: Hide AI UI when disabled

**Files:**
- Modify: `src/js/ai-sidebar.js`
- Modify: `src/js/app.js`

- [ ] **Step 1: Check ai_enabled on startup**

In app.js, read `ai_enabled` setting. If false, show "AI is disabled" message in sidebar instead of chat.

- [ ] **Step 2: Update ai-sidebar**

When AI is disabled, show a message: "AI features are disabled. Enable them in Settings → AI Features."

- [ ] **Step 3: Verify**

Run: `node --check src/js/ai-sidebar.js && node --check src/js/app.js`
Expected: no output.

- [ ] **Step 4: Commit**

```bash
git add src/js/ai-sidebar.js src/js/app.js
git commit -m "feat(ai): hide AI UI when disabled, show enable prompt"
```

---

### Task 7: Full verification and docs

- [ ] **Step 1: Run full test suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && cargo check --manifest-path src-tauri/Cargo.toml`
Expected: all tests pass, 0 errors.

Run: `Get-ChildItem tools\js-checks -Filter *.test.mjs | ForEach-Object { node $_.FullName }`
Expected: all harnesses pass.

- [ ] **Step 2: Update AGENTS.md and CHANGELOG.md**

Add D-163 through D-167 decisions. Update latest code pass.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "docs: record AI sidebar streamlining decisions"
```
