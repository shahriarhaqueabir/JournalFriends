# AI Control Center — Phase A Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the empty AI tab (`#tab-ai`) into the AI Control Center shell with a working Identity card (persona + `ai_voice`), and remove the dead `src/js/tabs/ai.js` chat module.

**Architecture:** Frontend-first: `#tab-ai` markup in `index.html` is replaced with five `settings-section` cards; a new controller `src/js/tabs/ai-controls.js` renders the persona form (reusing `persona.js`), pre-fills existing onboarding answers, and saves `ai_voice` + persona via existing commands. Backend: single new setting `ai_voice` seeded in `db/settings.rs`, threaded into `get_chat_system_prompt` as a new `&str` param. No new Tauri commands in this phase (bulk `save_ai_controls` arrives in Phase B once many keys exist).

**Tech Stack:** Tauri v2 / Rust, vanilla JS ESM (no bundler), SQLite via `app_settings`, existing plain-text HTML/CSS.

## Global Constraints

- No dependencies added (D-42); all UI uses existing `settings-section` / `persona-field` / `text-input-control` classes.
- `cargo check` must finish with 0 errors; `cargo fmt --check` clean.
- `node --check` clean on every touched JS file (ESM syntax validated, no execution).
- No test suite exists (Sprint 9 removed all `#[cfg(test)]`); verification = `cargo check` + `node --check` + in-app smoke.
- All settings writes go through `setting_set` (auto-audited via `AppEvent::LogEvent`, lib.rs:2350).
- `ai_voice` is an AI-behavior setting and lives in `app_settings`; persona answers remain `knowledge_items` (`created_by=onboarding`, D-138).
- Write code with NO comments unless they document a decision or non-obvious invariant.
- Commit each task separately with a conventional message.

---

### Task 1: Seed `ai_voice` setting

**Files:**
- Modify: `src-tauri/src/db/settings.rs:19` (add seed row)

**Interfaces:**
- Produces: `app_settings` row `("ai_voice", "")`; readable via existing `setting_get`/`get_setting`.

- [ ] **Step 1: Add the seed**

In `src-tauri/src/db/settings.rs`, add to the `defaults` array, directly above `("llama_api_key", "")`:

```rust
        ("ai_voice", ""),
```

- [ ] **Step 2: Verify compile**

Run: `cargo check` (from `src-tauri/`)
Expected: 0 errors, no warnings introduced.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/db/settings.rs
git commit -m "feat: seed ai_voice setting for AI control center"
```

---

### Task 2: Thread `ai_voice` into the chat system prompt

**Files:**
- Modify: `src-tauri/src/ai/prompt.rs:31-54` (signature + Role block injection)
- Modify: `src-tauri/src/lib.rs:859-865` (call site — read `ai_voice`, pass it)

**Interfaces:**
- Consumes: `ai_voice` setting from Task 1.
- Produces: `get_chat_system_prompt(facts, journal_context, task_context, event_context, weekly_review, ai_voice: &str)` — one new trailing `&str` argument; when non-empty the Role section gains a voice-guidance line. Only one call site exists (lib.rs:859).

- [ ] **Step 1: Extend the signature and inject voice**

In `src-tauri/src/ai/prompt.rs`, change the function signature (line 31) from

```rust
pub fn get_chat_system_prompt(
    facts: &[crate::db::knowledge::KnowledgeItem],
    journal_context: &[crate::db::journal::JournalContextSnippet],
    task_context: &[crate::db::tasks::Task],
    event_context: &[crate::db::events::Event],
    weekly_review: Option<&crate::db::reports::WeeklyReviewReport>,
) -> String {
```

to

```rust
pub fn get_chat_system_prompt(
    facts: &[crate::db::knowledge::KnowledgeItem],
    journal_context: &[crate::db::journal::JournalContextSnippet],
    task_context: &[crate::db::tasks::Task],
    event_context: &[crate::db::events::Event],
    weekly_review: Option<&crate::db::reports::WeeklyReviewReport>,
    ai_voice: &str,
) -> String {
```

Then, immediately after the closing `);` of the `format!` (which ends at the line with `now.format("%l:%M %p")`), and BEFORE the `if let Some(rv) = weekly_review` block, insert:

```rust
    let voice = ai_voice.trim();
    if !voice.is_empty() {
        prompt.push_str(&format!(
            "\nVoice guidance from the user: {}",
            voice
        ));
    }
```

- [ ] **Step 2: Update the single call site**

In `src-tauri/src/lib.rs:859`, before the `get_chat_system_prompt` call, add a read of the setting (mirror the `weekly_review` block pattern at lines 852-857):

```rust
    let ai_voice = {
        let pool = pool.clone();
        db::pool::with_conn(pool, move |conn| db::settings::get_setting(conn, "ai_voice"))
            .await
            .ok()
            .flatten()
            .unwrap_or_default()
    };
```

Then change the call (line 859) from

```rust
    let system_prompt = crate::ai::prompt::get_chat_system_prompt(
        &facts,
        &journal_context,
        &task_context,
        &event_context,
        weekly_review.as_ref(),
    );
```

to

```rust
    let system_prompt = crate::ai::prompt::get_chat_system_prompt(
        &facts,
        &journal_context,
        &task_context,
        &event_context,
        weekly_review.as_ref(),
        &ai_voice,
    );
```

- [ ] **Step 3: Verify compile + format**

Run: `cargo check && cargo fmt --check` (from `src-tauri/`)
Expected: 0 errors, `fmt --check` clean.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/ai/prompt.rs src-tauri/src/lib.rs
git commit -m "feat: inject configured AI voice into chat system prompt"
```

---

### Task 3: Replace `#tab-ai` with the Control Center shell + Identity card

**Files:**
- Modify: `src/index.html:350-355` (replace the empty-state block)

**Interfaces:**
- Produces: static markup `#tab-ai` → `div.settings-view-layout` containing header + 5 `section.settings-section` cards; Identity card has `#ai-voice-input` textarea, `#ai-persona-form` div, `#btn-save-ai-identity` button, `#ai-identity-status` span; cards 2-5 carry placeholder copy. Reuses icons already available in the vendored Lucide build.

- [ ] **Step 1: Replace the AI tab markup**

In `src/index.html`, replace the entire block at lines 350-355 (from `<div id="tab-ai" class="tab-view">` through its closing `</div>` before `<div id="tab-memory"`) with:

```html
    <div id="tab-ai" class="tab-view">
      <div class="settings-view-layout">
        <header class="settings-header">
          <h2>AI</h2>
          <p>Your local AI partner — who it is, what it knows, and how it works. Chat lives in the Thinking Partner sidebar.</p>
        </header>

        <section class="settings-section" id="ai-card-identity">
          <div class="settings-section-header">
            <div class="settings-section-icon"><i data-lucide="message-circle"></i></div>
            <div class="settings-section-title">
              <h3>Identity</h3>
              <p>Who the AI is and how it speaks to you.</p>
            </div>
          </div>
          <div class="settings-section-body">
            <div class="form-group">
              <label for="ai-voice-input">AI Voice</label>
              <textarea id="ai-voice-input" rows="3" class="text-input-control" placeholder="Tone, formality, coaching style — e.g. 'warm, direct, a little funny'" autocomplete="off"></textarea>
              <p class="settings-hint">Applies to the AI's conversation voice on your next message.</p>
            </div>
            <div class="form-group">
              <label>Persona</label>
              <div id="ai-persona-form" class="persona-form"></div>
              <p class="settings-hint">Your answers are stored on-device and help the AI remember who you are. Also editable in the Memory tab.</p>
            </div>
            <div class="form-group">
              <button id="btn-save-ai-identity" class="btn-primary">Save Identity</button>
              <span id="ai-identity-status" class="settings-hint"></span>
            </div>
          </div>
        </section>

        <section class="settings-section" id="ai-card-knowledge">
          <div class="settings-section-header">
            <div class="settings-section-icon"><i data-lucide="book-open"></i></div>
            <div class="settings-section-title">
              <h3>Knowledge</h3>
              <p>What the AI knows about you.</p>
            </div>
          </div>
          <div class="settings-section-body"><p class="settings-hint">Coming in a later phase.</p></div>
        </section>

        <section class="settings-section" id="ai-card-skills">
          <div class="settings-section-header">
            <div class="settings-section-icon"><i data-lucide="bot"></i></div>
            <div class="settings-section-title">
              <h3>Skills & Tools</h3>
              <p>Capabilities and function calls the AI may use.</p>
            </div>
          </div>
          <div class="settings-section-body"><p class="settings-hint">Coming in a later phase.</p></div>
        </section>

        <section class="settings-section" id="ai-card-workflows">
          <div class="settings-section-header">
            <div class="settings-section-icon"><i data-lucide="git-branch"></i></div>
            <div class="settings-section-title">
              <h3>Workflows</h3>
              <p>Auto-analysis, context budgets, and scheduling.</p>
            </div>
          </div>
          <div class="settings-section-body"><p class="settings-hint">Coming in a later phase.</p></div>
        </section>

        <section class="settings-section" id="ai-card-engine">
          <div class="settings-section-header">
            <div class="settings-section-icon"><i data-lucide="cpu"></i></div>
            <div class="settings-section-title">
              <h3>Engine &amp; Performance</h3>
              <p>Models, inference parameters, and server tuning.</p>
            </div>
          </div>
          <div class="settings-section-body"><p class="settings-hint">Coming in a later phase.</p></div>
        </section>
      </div>
    </div>
```

- [ ] **Step 2: Verify markup**

Run: `node --check` is not applicable to HTML; verify the block is well-formed by confirming `#tab-ai` has a single matching `</div>` for its opening tag and the `#tab-memory` sibling follows intact. Also confirm `git diff src/index.html` shows only the intended replacement.

- [ ] **Step 3: Commit**

```bash
git add src/index.html
git commit -m "feat: AI Control Center shell with Identity card in AI tab"
```

---

### Task 4: AI Control Center controller (`ai-controls.js`)

**Files:**
- Create: `src/js/tabs/ai-controls.js`
- Test: `node --check src/js/tabs/ai-controls.js`

**Interfaces:**
- Consumes: Task 3 markup ids (`ai-voice-input`, `ai-persona-form`, `btn-save-ai-identity`, `ai-identity-status`); `persona.js` exports `personaQuestions`, `renderPersonaForm`, `collectPersonaAnswers`, `savePersonaAnswers`; `./ipc.js` `invoke`.
- Produces: `initAiControls()` — renders the persona form, pre-fills saved persona answers + `ai_voice`, wires the Save button. Exported module invoked by app.js (`window._aiControlsInitialized` guard).

- [ ] **Step 1: Write the module**

Create `src/js/tabs/ai-controls.js`:

```js
import { invoke } from "../ipc.js";
import {
  personaQuestions,
  renderPersonaForm,
  collectPersonaAnswers,
  savePersonaAnswers,
} from "../persona.js";

const PREFIX = "ai-persona";

export function initAiControls() {
  if (window._aiControlsInitialized) return;
  window._aiControlsInitialized = true;

  renderPersonaForm(document.getElementById("ai-persona-form"), PREFIX);
  void loadIdentity();
  wireSave();
}

async function loadIdentity() {
  const [voice, items] = await Promise.all([
    invoke("setting_get", { key: "ai_voice" }),
    invoke("memory_list", { kind: null, category: null, search: null }),
  ]);

  const voiceEl = document.getElementById("ai-voice-input");
  if (voiceEl) voiceEl.value = (voice && typeof voice === "string" ? voice : "") || "";

  const byTitle = new Map(
    (items || [])
      .filter((i) => i.created_by === "onboarding")
      .map((i) => [i.title, i.content])
  );
  for (const q of personaQuestions()) {
    const el = document.getElementById(`${PREFIX}-${q.key}`);
    if (el && byTitle.has(q.title)) el.value = byTitle.get(q.title) || "";
  }
}

function wireSave() {
  const saveBtn = document.getElementById("btn-save-ai-identity");
  const statusEl = document.getElementById("ai-identity-status");
  const voiceEl = document.getElementById("ai-voice-input");
  if (!saveBtn || !statusEl || !voiceEl) return;

  saveBtn.addEventListener("click", async () => {
    saveBtn.disabled = true;
    statusEl.textContent = "Saving…";
    try {
      const answers = collectPersonaAnswers(PREFIX);
      const res = await savePersonaAnswers(answers);
      await invoke("setting_set", {
        key: "ai_voice",
        value: voiceEl.value.trim(),
      });
      statusEl.textContent = `Saved — ${res.saved} new, ${res.updated} updated persona answers. Voice applies on your next message.`;
    } catch (e) {
      console.error("ai controls save failed", e);
      statusEl.textContent = "Save failed — see console.";
    } finally {
      saveBtn.disabled = false;
    }
  });
}
```

- [ ] **Step 2: Syntax check**

Run: `node --check src/js/tabs/ai-controls.js`
Expected: no output, exit code 0.

- [ ] **Step 3: Commit**

```bash
git add src/js/tabs/ai-controls.js
git commit -m "feat: AI control center controller (identity card)"
```

---

### Task 5: Wire AI tab init in app.js + remove dead `ai.js`

**Files:**
- Modify: `src/js/app.js` (add `data-tab="ai"` lazy-import handler; remove nothing else)
- Delete: `src/js/tabs/ai.js` (dead — targets `#chat-messages`/`#chat-input`/`#btn-send-chat` which exist nowhere; not imported by any module)

**Interfaces:**
- Consumes: `initAiControls()` from Task 4.
- Produces: clicking the AI nav item initializes the controller exactly once (`window._aiControlsInitialized`).

- [ ] **Step 1: Add the AI tab handler**

In `src/js/app.js`, after the existing `// Refresh events on tab switch` handler (after line 186), add:

```js
// Load and init AI Control Center when clicked
document.querySelector('[data-tab="ai"]').addEventListener('click', async () => {
  if (!window._aiControlsInitialized) {
    const m = await import('./tabs/ai-controls.js');
    m.initAiControls();
  }
});
```

NOTE: do NOT set `window._aiControlsInitialized` in this handler — `initAiControls()` owns the flag (it sets it at the top of its body). Pre-setting it here would make `initAiControls()` return early and the controller never wire up.

- [ ] **Step 2: Delete the dead AI chat module**

Remove the file `src/js/tabs/ai.js`.

- [ ] **Step 3: Verify no dangling references**

Run: `grep -rn "tabs/ai\.js\|AiChat\|window\.aiChat\|getElementById('chat-messages')\|getElementById('chat-input')\|getElementById('btn-send-chat')" src/`
Expected: zero matches. NOTE: the right sidebar uses `ai-chat-messages`/`ai-chat-input` (with the `ai-` prefix) — do not confuse those; the exact `chat-messages`/`chat-input`/`btn-send-chat` ids must be gone.

- [ ] **Step 4: Syntax check**

Run: `node --check src/js/app.js`
Expected: no output, exit code 0.

- [ ] **Step 5: Commit**

```bash
git add src/js/app.js && git rm src/js/tabs/ai.js
git commit -m "chore: wire AI control center init, remove dead ai chat module"
```

---

### Task 6: Phase A verification pass

**Files:**
- Verify: `src/index.html`, `src/js/app.js`, `src/js/tabs/ai-controls.js`, `src-tauri/src/db/settings.rs`, `src-tauri/src/ai/prompt.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Backend gates**

Run: `cargo check && cargo fmt --check` (from `src-tauri/`)
Expected: 0 errors; `fmt --check` clean.

- [ ] **Step 2: Frontend gates**

Run: `node --check src/js/app.js && node --check src/js/tabs/ai-controls.js && node --check src/js/persona.js`
Expected: no output, exit code 0 for each.

- [ ] **Step 3: Manual in-app smoke**

Using `cargo tauri dev` (or the built app) with the vault unlocked and llama-server running:
1. Open the **AI** tab → five cards render; Identity card has Textarea + 6 persona inputs + Save button.
2. If onboarding persona answers exist, they are pre-filled in the inputs; `ai_voice` textarea shows any saved value.
3. Type a new voice (e.g. "formal and concise") + edit one persona answer → **Save Identity** → status shows "Saved — 0 new, 1 updated".
4. Send a chat message in the Thinking Partner sidebar → reply reflects the new voice guidance and updated persona fact.
5. Confirm the Settings tab and right sidebar are unaffected.
Record outcome in the task checklist.

- [ ] **Step 4: Report phase status**

Report: phase complete, exact commands run + outputs, any smoke deviations. Update `CHANGELOG.md` and `AGENTS.md` (file structure / conventions) per the repo's session-end checklist.