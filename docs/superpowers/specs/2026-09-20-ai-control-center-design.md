# AI Control Center — Design Spec

**Date:** 2026-09-20
**Status:** Approved for planning
**Branch:** `wip/ollama-to-llamacpp-migration`
**Related decisions:** D-19, D-95, D-116–D-128, D-138, D-139
**Relates to:** `ai/prompt.rs`, `ai/tools.rs`, `ai/client.rs`, `ai/analysis.rs`, `ai/engine.rs`, `db/settings.rs`, `lib.rs`, `src/js/tabs/ai.js`, `src/index.html`, `src/js/persona.js`

---

## 1. Problem

The app's AI behavior is today controlled only by the Settings → "LLM Engine" card (models, GPU layers, context, ports), a one-time onboarding persona questionnaire, and a Memory tab knowledge base. There is no single place to manage **who the AI is**, **what it knows**, **what it can do**, **how it works**, or **how it performs**. Twelve tools are always on (including the network `fetch_url`), capabilities such as Calendar Vision / Productivity Pulse are hardcoded in the prompt, and inference parameters (temperature/top-p/max tokens) don't exist anywhere.

## 2. Goals

Provide a dedicated "AI Control Center" inside the existing AI tab so a user can:

- Set and edit the AI's **persona** (the 6 onboarding questions) and **voice** (how it speaks)
- See a **snapshot of what the AI knows** about them
- Enable/disable **skills** (capabilities) and **tools** (function calls) — with network `fetch_url` off by default
- Control **workflows**: auto-analyze on save, context budgets, weekly report, re-analyze-all
- Manage **engine + inference performance**: relocated engine card plus temperature/top-p/max-tokens

Offline-first and user-driven (D-116/D-126): all controls persist locally in `app_settings`; nothing phones out except an explicitly re-enabled `fetch_url`.

## 3. Scope

- **In scope:** new Controls sub-tab UI, new settings seeds, tool filtering + explicit denial, prompt.rs budget/voice/skill gating, inference params through the chat request, `reanalyze_all_journals`, relocation of the LLM Engine card out of Settings.
- **Out of scope:** batch "fidelity" re-analysis slider, web search (B-03), runtime model hot-swap beyond existing `restart_engine`, changing the right-sidebar chat, new DB tables.

## 4. Structure

The AI tab (`#tab-ai`, currently an empty placeholder) becomes the single **AI Control Center** — a stacked-card panel styled like existing `settings-section` cards, controller `src/js/tabs/ai-controls.js`. Five cards:

  1. Identity
  2. Knowledge
  3. Skills & Tools
  4. Workflows
  5. Engine & Performance

There is no chat inside the AI tab: chat stays in the always-mounted right sidebar (`ai-sidebar.js`), per D-23. `src/js/tabs/ai.js` is dead code (targets `#chat-messages` etc. which do not exist) and is removed.

Any active control changes apply on the next chat turn (sent once a "Save" button is pressed) or on the next analysis run; no live mid-conversation rebinding. Exception: model/GGUF/GPU-layers/context/port changes already require an engine restart — that behavior is preserved from the existing engine card (the Controls "Save" persists them; `restart_engine` applies them).

## 5. Controls by card

### 5.1 Identity

- **AI voice** (new, `app_settings` key `ai_voice`): free-text field ("How should Hawkward AI sound? Tone, formality, coaching style"). Injected into `prompt.rs` "## Role" block. This is an AI-behavior setting, distinct from persona answers (which are user facts).
- **Persona** (reuse): the 6 onboarding questions rendered via existing `persona.js::renderPersonaForm`, editable inline, saved via `savePersonaAnswers` (dedupe by question title, `created_by=onboarding`, per D-138). Remains `knowledge_items` so Memory-tab editing stays coherent.

### 5.2 Knowledge

- Read-only snapshot from `memory_list`, no CRUD (Memory tab stays the editor; include a "Manage in Memory tab →" tab-switch button):
  - Counts by kind (memory / information / knowledge) + `persona` flagged items
  - "What the AI actually sees": top items by confidence that get injected per chat (10-fact budget shown live); note that snippet/task/event caps are set in Workflows

### 5.3 Skills & Tools

**Skills** (capability toggles, `skill_<name>` persisted):

| Skill | Default | Backend behavior gated |
|---|---|---|
| Journal Analysis | on | analysis worker + on-save auto-analysis (`lib.rs` worker / `journal_analysis_queued`) |
| Memory Recall | on | `## Knowledge Base` block injection in `prompt.rs` |
| Calendar Vision | on | `## Upcoming events` block injection |
| Productivity Pulse | on | `## Productivity Pulse` block injection |
| Deep Citations | on | jump-to-source links in replies (D-139 formatting) — gates frontend link rendering in `chat-format.js` |
| Thinking | on | `ai_reasoning_token` → thinking panel (D-139) — gates SSE reasoning extraction/emission (off = reasoning tokens are discarded, not streamed) |

**Tools** (per-tool toggles, `tool_<name>` persisted), grouped:

- Task mgmt: `create_task`, `update_task`, `complete_task`, `list_tasks`
- Memory: `list_memory`, `create_memory`, `update_memory`, `delete_memory`, `link_memory`
- Reading: `search_journal`, `list_events`
- Network: `fetch_url` — **default OFF** (privacy: offline-first app must not silently phone out; re-enabling shows a "this enables internet access" note)

`get_tools()` in `ai/tools.rs:541` becomes `get_tools(&ToolsEnabled)`; disabled tools are filtered from the schema **and** when the model calls a disabled tool, `tool_execute` returns an explicit denial result string (`tool_request_denied: <name> is disabled in AI settings`) that becomes a normal assistant turn — the model never silently claims an action it can't perform.

### 5.4 Workflows

- **Auto-analyze on save** toggle (`analysis_auto`): gates the on-save/`journal_analysis_queued` analysis trigger, *distinct* from the Journal Analysis *skill* (which gates the whole worker).
- **Re-analyze all** button: clears the `last_hashes` dedupe cache in `ai/analysis.rs` and re-queues all entries through the analysis worker; shows "N entries queued". Backed by new `reanalyze_all_journals` command.
- **Context budget** — numeric inputs overriding hardcoded caps in `prompt.rs`:
  - Facts injected per chat (default 10, range 0–30) → `ai_ctx_facts`
  - Journal snippets (default 5, range 0–20) → `ai_ctx_snippets`
  - Open tasks (default 8, range 0–30) → `ai_ctx_tasks`
  - Events horizon days (default 3, range 1–14) → `ai_ctx_event_days`
- **Weekly report** toggle (`weekly_report_enabled`): gates the Monday scheduler in `scheduler/weekly_report.rs` from emitting the productivity pulse. Relationship to the Productivity Pulse *skill*: the pulse only exists if `weekly_report_enabled` is on (scheduler produces it); the Pulse *skill* separately gates whether that data is injected into the prompt. Turning the skill off without the scheduler still keeps reviews logging-time; turning the scheduler off means no pulse data exists to inject.

### 5.5 Engine & Performance

Relocated existing Settings "LLM Engine" card content (models, GGUF import, exe path, GPU layers, context size, ports, restart, status), **plus** new inference knobs:

| Knob | Range | Default | Plumbing |
|---|---|---|---|
| Temperature | 0.0–2.0 step 0.1 | 0.7 | `ChatRequest` + request body |
| Top-p | 0.0–1.0 step 0.05 | 0.95 | `ChatRequest` + request body |
| Max tokens | 64–8192 | 2048 | `ChatRequest` + request body |

Settings tab's LLM Engine card is **removed** (single source of truth); replaced by a pointer row ("AI engine controls live in the AI tab →").

## 6. Backend plumbing

**New settings seeds** (`db/settings.rs`): `ai_voice`, `skill_journal_analysis`, `skill_memory_recall`, `skill_calendar_vision`, `skill_productivity_pulse`, `skill_deep_citations`, `skill_thinking`, `tool_create_task`, `tool_update_task`, `tool_complete_task`, `tool_list_tasks`, `tool_list_memory`, `tool_create_memory`, `tool_update_memory`, `tool_delete_memory`, `tool_link_memory`, `tool_search_journal`, `tool_list_events`, `tool_fetch_url`, `analysis_auto`, `weekly_report_enabled`, `ai_ctx_facts`, `ai_ctx_snippets`, `ai_ctx_tasks`, `ai_ctx_event_days`, `ai_temperature`, `ai_top_p`, `ai_max_tokens`.

**New / changed Tauri commands**:
- `save_ai_controls` — bulk single-trip write of all New control values (avoids 20 `setting_set` round-trips)
- `ai_controls_load` — returns one flat object of all control values for the frontend
- `reanalyze_all_journals` — clears hash-dedupe cache, re-queues entries, returns queued count
- `get_tools(&ToolsEnabled)` — tool filter + denial result string
- `prompt.rs` — reads `ai_voice` + skill flags + budgets instead of hardcoded caps (`.take(10)` etc.)
- `client.rs` — `ChatRequest` gains `temperature` / `top_p` / `max_tokens`
- analysis worker — gates on `skill_journal_analysis` and `analysis_auto`

No new DB tables.

## 7. Dependencies & risks

- `prompt.rs` `get_chat_system_prompt` is passed data slices; giving it settings access is the main signature change. Mitigation: pass a small `PromptPrefs` value struct (voice, skill flags, budgets) computed once in `lib.rs` — keeps `prompt.rs` pure.
- Tool denial string must stay visible-and-returned as a tool result so the model surfaces it (test in smoke).
- `ai_controls_save` must be audited (LogEvent on set) per existing convention.
- Relocating engine markup out of Settings touches `settings.js` `initLLMEngineSection`; ensure tab-switch refresh (`window.refreshAiSettings`) still targets the right DOM ids that now live in the Controls sub-tab.

## 8. Phasing (each independently committable)

- **Phase A** — Control Center shell (card stack + controller wiring into `#tab-ai`) + Identity card (persona reuse + `ai_voice`); remove dead `tabs/ai.js`
- **Phase B** — Skills & Tools card (settings seeds, `get_tools` filter + denial, defaults, network warning)
- **Phase C** — Workflows card (budgets, auto-analyze, weekly toggle, `reanalyze_all_journals`)
- **Phase D** — Engine card relocation + inference knobs; Settings row cleanup

Each phase: commit → `node --check` new JS → `cargo check` → designated in-app smoke (toggle control, verify injection/denial/queue behavior in a test chat).

## 9. Acceptance criteria

1. The AI tab renders the five-card Control Center (no chat duplicate; sidebar chat untouched, D-23).
2. Editing persona questions + voice, then chatting, produces replies reflecting the new voice (prompt injection verified).
3. Disabling a tool removes it from the schema; calling it returns the explicit denial string surfaced to the user.
4. `fetch_url` is disabled by default; enabling it shows the network warning.
5. Turning off Journal Analysis prevents new analyses; turning off auto-analyze keeps the worker but stops on-save trigger.
6. Re-analyze-all re-queues all entries and reports an accurate count.
7. Context-budget inputs change injection counts (verified via prompt dump or reply behavior).
8. Temperature/top-p/max-tokens reach the llama-server request and persist across restart.
9. Settings tab no longer contains the engine card; shows the pointer row instead.
10. All changes pass `cargo check` (0 errors) and `node --check`; no regressions in chat, analysis, Memory tab, or existing Settings cards.