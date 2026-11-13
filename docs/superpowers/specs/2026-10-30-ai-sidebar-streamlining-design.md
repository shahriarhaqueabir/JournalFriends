# AI Sidebar Streamlining & Optional AI — Design Spec

**Date:** 2026-10-30
**Status:** Approved (brainstorming complete)
**Driver:** User feedback — onboarding is too noisy with AI options; AI sidebar shows too much (notes/facts/events) instead of just tasks; tasks tab doesn't auto-refresh when AI creates tasks; app should be usable without AI.

---

## 1. Problem Statement

The current onboarding wizard has 7 steps, 4 of which are AI-related (engine binary, chat GGUF, embed GGUF, system check). This creates friction for users who just want a journal/task manager. The AI sidebar shows all extracted items (tasks, notes, facts, events) as interactive proposal cards, which is overwhelming. When the AI creates tasks via tool calls during chat, the tasks tab doesn't refresh automatically. The app should be fully usable without AI enabled.

## 2. Design Decisions

### D-163: Onboarding is AI-free
Onboarding reduces to 3 steps: Welcome → Vault password → Done. No engine, model, or GPU configuration. AI setup moves entirely to Settings. The app works immediately after onboarding without AI.

### D-164: AI lives in a dedicated Settings section
A new "AI Features" section in Settings (separate from the existing "LLM Engine" card) contains:
- Enable/disable AI toggle (master switch)
- When enabled: engine status, model import, GPU layers, system check
- When disabled: all AI UI is hidden; no engine provisioning on startup
- The existing "LLM Engine" card becomes part of this section or is replaced by it

### D-165: AI sidebar shows only tasks as interactive cards
When the AI analyzes a journal entry:
- **Tasks**: shown as interactive proposal cards (Create/Dismiss) — same as current
- **Notes, facts, events**: summarized as plain text ("I've saved 2 notes and 1 fact to memory for future context"). No interactive cards.
- User can chat with the AI to adjust before any tool calls are made
- Once tool calls are executed, the respective output location auto-refreshes

### D-166: Auto-refresh on AI tool calls
When the AI creates/updates/deletes a task (via tool call during chat), the tasks tab automatically refreshes to show the new state. A brief toast confirms the action. No manual refresh needed.

### D-167: Notes and facts are internal
Notes and facts extracted by the AI are saved to the knowledge base silently. They are never shown as interactive cards. The user sees a text summary only.

## 3. Scope

### In scope
- Onboarding reduction (remove AI steps)
- Settings: dedicated AI section with enable/disable toggle
- AI sidebar: tasks-only cards, text summary for notes/facts/events
- Auto-refresh tasks tab on AI tool calls
- Hide AI UI when disabled

### Out of scope
- Changing the AI analysis pipeline itself
- Changing the proposal/confirmation model for tasks
- New AI tools or capabilities

## 4. Architecture

### 4.1 Onboarding changes
- Remove steps 1-6 (engine, chat GGUF, embed GGUF, system check, persona)
- Keep step 0 (welcome) + vault password (currently part of unlock flow)
- Onboarding completes after vault creation
- `onboarding_complete` setting still written

### 4.2 Settings changes
- New "AI Features" section at top of Settings (or replace LLM Engine card)
- Master toggle: `ai_enabled` setting (default: false)
- When toggle is on:
  - Show engine status, model import, GPU layers, system check
  - Provision engine on next startup
- When toggle is off:
  - Hide all AI config
  - Don't provision engine
  - AI sidebar shows "AI is disabled. Enable in Settings." message

### 4.3 AI sidebar changes
- Analysis results: tasks → interactive cards, notes/facts/events → text summary
- The `extractedItemsHtml` renderer (analysis-view.js) already separates tasks from other items
- Modify to render tasks as proposal cards, other items as text
- When AI chat creates a task via tool call, emit an event that triggers tasks tab refresh

### 4.4 Auto-refresh mechanism
- AI tool calls that mutate tasks emit `task_changed` event
- Tasks tab listens for this event and re-fetches
- Toast confirms "Task created/updated/deleted"

## 5. Files to modify

### Onboarding
- `src/index.html` — reduce onboarding steps
- `src/js/onboarding.js` — simplify to welcome + vault only

### Settings
- `src/index.html` — add AI Features section
- `src/js/tabs/settings.js` — add enable/disable toggle, conditionally show AI config

### AI Sidebar
- `src/js/ai-sidebar.js` — tasks-only cards, text summary for notes/facts
- `src/js/analysis-view.js` — update renderer
- `src/js/tabs/tasks.js` — listen for task_changed event

### Backend
- `src-tauri/src/lib.rs` — add `ai_enabled` setting, conditionally provision
- `src-tauri/src/ai/tools.rs` — emit task_changed event on task mutations
- `src-tauri/src/events.rs` — add TaskChanged event variant

## 6. Testing

- Onboarding completes in 2 steps without AI
- Settings AI toggle enables/disables AI features
- AI sidebar shows tasks as cards, notes/facts as text
- Tasks tab auto-refreshes when AI creates a task
- App works fully without AI enabled
