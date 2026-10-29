# Journal Companion Completion — Design

**Date:** 2026-10-28  
**Status:** Approved for planning  
**Scope:** Complete the remaining journal-companion, planning, reflection, and reliability work. This is a phased program, not one broad patch.

## Product Goal

HawkwardJournalAI is first a journaling app. A user brain-dumps thoughts and feelings; the local AI reflects and helps unwind them, analyzes entries for useful tasks and insights, and helps organize the user's plans and schedule. Journal data and model inference stay on-device; the product has no accounts or cloud sync.

## Locked Product Constraints

- **No internet access from the app or AI.** Remove the AI `fetch_url` tool as well as keeping B-03 web search out of scope. Keep only loopback HTTP to the app-managed llama.cpp servers. Retain local “Jump to Source” links; do not build external research citations.
- **Offline Windows installation.** Replace the WebView2 download bootstrapper with Tauri's bundled offline installer. It adds about 127 MB but does not require internet at install time.
- **Manual model selection; no model downloads.** The app presents file pickers for local GGUF files and may show recommendation names only. It never downloads model weights.
- **Qwythos is the fixed chat/reasoning role.** The user manually selects the local `Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M` GGUF; use it for chat, journal analysis, and tool orchestration. Do not offer another model as the chat role.
- **Nomic embeddings are only for semantic journal search.** Generating/storing journal-entry vectors and embedding a chat query to retrieve journal entries are in scope. Do not use the embedding model for chat, analysis, task planning, or memory operations.
- **Confirmation remains mandatory (D-19).** AI mutations become proposals; no tool writes tasks, events, notes, or memories directly. User acceptance in the Review Inbox is the write boundary.
- **Training answers use `knowledge_items` (D-138).** Do not resurrect `user_profile` or fine-tune model weights. User answers are editable and explicitly saved.
- **Use existing app patterns and design tokens.** Make the smallest UI changes needed; preserve working flows and the right-side chat companion.

## Existing Capabilities to Preserve

- Plain-text journal, keyset pagination, and background per-entry analysis.
- Per-entry task/fact/event/note proposals, with user review and acceptance.
- Task list/board/calendar and task/memory proposal tools.
- Manual mood check-ins plus journal-inferred mood, existing reports, and the Insights dashboard.
- Events/Notes CRUD and a basic calendar view already exist in source; the work should complete and expose these flows rather than rebuild them.
- Local encrypted vault, backups, export/import, System diagnostics, and read-only SQL tools.

## Phased Design

### Phase 1 — AI Role Contract and Journal-to-Plan Actions

1. Make model setup explicit: ask the user to manually browse to/import the required Qwythos GGUF for chat and a Nomic GGUF for embeddings. Show only the recommended names, without a download link or network call. Clearly label the roles in onboarding and Settings; do not offer a non-Qwythos model as the chat role.
2. Preserve one role-specific client contract: chat, analysis, and tool calls use the chat model; embedding requests are limited to journal-vector indexing and semantic journal retrieval.
3. Clarify the System tab model status: show the active Qwythos chat model and active embedding model separately from the inventory of imported GGUF files.
4. Extend the conversational tool set to read notes and to propose create/update/delete operations for events and notes, alongside the existing task and memory tools. Resolve real item IDs before proposing updates/deletes.
5. Remove `fetch_url` from the tool schema and executor so model output cannot trigger an external request. Local llama-server calls remain loopback-only.
6. Route every new mutation through `proposed_inbox` and `proposed_action`; preview the change and support accept/dismiss. Keep each acceptance transactional and idempotent where the existing proposal model supports it.
7. Retain local journal/task/event source links. No web-search or external-source citation surface is added.

**Acceptance:** tests prove the chat role accepts the manually selected Qwythos GGUF, and that Nomic embedding calls are only used for journal search/indexing; `fetch_url` is absent and direct external requests are unavailable to the AI; all event/note chat mutations appear as pending proposals; accepting applies exactly the requested change once, repeated acceptance creates no duplicate, dismissing leaves data unchanged, invalid or unknown IDs are rejected, and no mutation bypasses confirmation.

### Phase 2 — Schedule and Reminder Workflow

- Make the existing Events/Notes/Calendar surface reachable from the primary product navigation and consistent with the current design system.
- Complete event creation/edit/deletion, month navigation, event-to-detail behavior, and reminder setting/notification; use local-time display and timezone-safe day keys.
- Keep notes available beside schedule items without turning the journal into a rich-text editor.

**Acceptance:** a user can create/edit/delete an event and note, set and receive a reminder, and use the calendar to inspect and open scheduled events; board/calendar interactions work in the built Windows app.

### Phase 3 — Reflection, Insights, and Training

- **B-01 Training Zone:** an explicitly user-started guided Q&A that saves user answers as editable `knowledge_items` after an explicit save. Reuse existing Memory editing and deduplicate by stable question/title. It is profile/context capture, not model training.
- **B-02 Journal Insights:** add cross-entry pattern observations to the existing Insights surface (e.g. recurring mood/activity/task patterns and a weekly summary), generated by Qwythos from local journal, mood, and task data. Ground observations in local source entries and expose local jump links. Keep insights read-only unless the user chooses a confirmation-gated save to Memory.
- Do not duplicate all injected prompt context in tool results; keep the context budget bounded and make the distinction between supplied context and live tool results clear.

**Acceptance:** Training answers are saved/edited in Memory and survive restart; Insights summarize more than a single entry, provide source links, and never claim external research; no insight is persisted to memory without user confirmation.

### Phase 4 — Reliability and Product Verification

- Harden chat/embed engine restart and recovery; ensure spawned llama-server processes are stopped/replaced without strays. Keep Qwythos as the chat role and Nomic as the journal-search embedding role.
- Complete the Journal `VirtualList` abstraction while preserving keyset pagination.
- Add regression/E2E coverage for the journal → analysis → proposal → acceptance loop, chat tool round-trips, schedule reminders, model role routing, and fresh portable startup. Finish the pending visual pass of the UI and board/calendar interactions.

**Acceptance:** failed engines recover with actionable status, no duplicate server process remains after restart, journal pagination remains stable, and a clean portable launch works with no internet and creates/uses only its app-local data directory.

## Explicitly Out of Scope

- Web search, direct-URL fetching from AI, search-provider configuration, model downloads, cloud sync/accounts, and remote LLM services. The embedded WebView2 offline installer is a local release resource, not a runtime download.
- Automatic AI writes, automatic fine-tuning, or resurrecting the legacy `user_profile` table.
- Replacing the current journal editor with Markdown or rich text.
- Changing MSI/NSIS install scope or granting write ACLs to protected install directories; the portable archive is the supported path for executable-relative data outside the user profile.

## Delivery Strategy

Implement one independently testable phase at a time, in the order above. Each phase gets a focused implementation plan and verification gate; do not begin the next phase until its tests/build and relevant UI flow are verified. The existing runtime directory is a local build prerequisite and remains separately staged because it is ignored by Git.
