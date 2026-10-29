# Phase 1 — AI Role Contract and Journal-to-Plan Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline) or superpowers:subagent-driven-development (agent-per-task) to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Qwythos as the manually selected chat/analysis/tool model, reserve Nomic embeddings for semantic journal retrieval, remove external AI networking, and let chat propose event/note changes through user confirmation.

**Architecture:** Preserve the two local llama.cpp roles and keep all application writes behind `proposed_inbox`/`proposed_action`. Validate the chat model at import/selection boundaries, remove the AI URL-fetch path and its only direct URL dependency, and add typed proposal application helpers for events and notes. Windows release packaging uses the Tauri WebView2 offline installer and bundled runtime resources.

**Tech Stack:** Rust, Tauri v2, rusqlite/SQLite, serde_json, vanilla JavaScript, Node harnesses, local llama.cpp.

---

## File Map

- `src-tauri/src/ai/models.rs`: GGUF role validation and chat-store filtering.
- `src-tauri/src/ai/tools.rs`: tool schemas/executor, read-only note listing, proposal creation; remove `fetch_url` and its outbound HTTP client.
- `src-tauri/src/ai/prompt.rs`: Qwythos role and confirmation/tool policy.
- `src-tauri/src/db/events.rs`: transactional event/note proposal application helpers and unit tests.
- `src-tauri/src/lib.rs`: validate `llama_chat_model`, route accepted event/note proposal actions, preserve the existing task/memory paths.
- `src-tauri/Cargo.toml` and `Cargo.lock`: remove the direct `url` dependency if no other direct source use remains; `reqwest` stays for loopback llama-server API calls.
- `src-tauri/tauri.conf.json`: use the WebView2 offline installer; keep the staged llama runtime resource mapping.
- `src/js/onboarding.js`, `src/index.html`, `src/js/tabs/settings.js`: clearly label the manual Qwythos chat GGUF and Nomic semantic-search GGUF choices and recommendation names; do not add download UI.
- `tools/js-checks`: extend existing model/proposal harnesses only where frontend behavior needs coverage.
- `README.md`, `CHANGELOG.md`, `AGENTS.md`: record the offline policy, role contract, and verified behavior.

## Task 1: Enforce Manual Qwythos/Nomic Model Roles

**Files:** `src-tauri/src/ai/models.rs`, `src-tauri/src/lib.rs`, `src/js/onboarding.js`, `src/index.html`, `src/js/tabs/settings.js`, `tools/js-checks/components.test.mjs`.

- [ ] **Step 1: Write failing ModelStore tests.** Add this test shape to `src-tauri/src/ai/models.rs`; create three tiny temporary `.gguf` files named `Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M.gguf`, `nomic-embed-text-v2-moe.Q4_K_M.gguf`, and `other-chat.gguf`, import them to a fresh `ModelStore`, and delete only that UUID-named test directory:

```rust
#[test]
fn chat_import_requires_the_recommended_qwythos_file_but_embed_import_is_manual() {
    let root = std::env::temp_dir().join(format!("model-role-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let q = root.join("Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M.gguf");
    let n = root.join("nomic-embed-text-v2-moe.Q4_K_M.gguf");
    let other = root.join("other-chat.gguf");
    for file in [&q, &n, &other] { std::fs::write(file, b"test").unwrap(); }
    let store = ModelStore::new(root.join("models"));

    assert!(store.import_model(&q, "chat").is_ok());
    assert!(store.import_model(&other, "chat").is_err());
    assert!(store.import_model(&n, "embed").is_ok());
    assert_eq!(store.scan_chat().iter().map(|m| m.filename.as_str()).collect::<Vec<_>>(),
               vec!["Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M.gguf"]);
    std::fs::remove_dir_all(root).unwrap();
}
```

- [ ] **Step 2: Verify RED.** Run `cargo test model_role_`; expected: the non-Qwythos chat-import assertion fails against the current unrestricted `ModelStore::import_model`.
- [ ] **Step 3: Implement the role guard.** Define `const CHAT_MODEL_STEM: &str = "Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M"`; validate `target_type == "chat"` and the source file stem before copying. Filter `scan_chat` by that stem and make `resolve_chat_path` reject any other file. Leave embedding GGUF selection manual and label Nomic as the journal-search recommendation.
- [ ] **Step 4: Enforce settings and onboarding.** In the `import_model` Tauri command and the `setting_set` path for `llama_chat_model`, reject unsupported chat filenames. Update onboarding and Settings copy to say “Select a local GGUF”; show the Qwythos and Nomic names as recommendations only, with no URL or download action.
- [ ] **Step 5: Verify GREEN.** Run `cargo test model_role_` and the extended `components.test.mjs`; expected: Qwythos imports, non-Qwythos chat imports are rejected, and embed imports remain manual.

## Task 2: Enforce the No-Internet Runtime Boundary

**Files:** `src-tauri/src/ai/tools.rs`, `src-tauri/src/ai/prompt.rs`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `README.md`.

- [ ] **Step 1: Write a failing tool-surface test.** Add this test to the existing `ai::tools` test module:

```rust
#[test]
fn ai_tool_schema_has_no_external_fetch_tool() {
    assert!(!get_tools().iter().any(|tool| tool["function"]["name"] == "fetch_url"));
}
```

- [ ] **Step 2: Verify RED.** Run `cargo test ai_tool_schema_has_no_external_fetch_tool`; expected: the assertion fails while `get_tools()` still exposes `fetch_url`.
- [ ] **Step 3: Remove the external AI fetch path.** Remove `fetch_url` from `get_tools()` and the executor match; delete `exec_fetch_url`, its `reqwest::Client` field/builders, body reader, URL parser, DNS/IP guard, and fetch-only tests. Keep `reqwest` for the local `127.0.0.1` llama.cpp client. Remove `url = "2"` from direct dependencies if repository search confirms no other direct `url::` use; regenerate the lockfile with Cargo.
- [ ] **Step 4: Set offline WebView2 installation.** Change `src-tauri/tauri.conf.json` to:

```json
"webviewInstallMode": { "type": "offlineInstaller" }
```

This bundles the WebView2 offline installer; it must not use `downloadBootstrapper` or make a setup-time network request.

- [ ] **Step 5: Verify GREEN and package config.** Run `cargo test ai_tool_schema_has_no_external_fetch`, `cargo fmt --check`, and `cargo tauri build --bundles msi nsis`. Expected: the tool is absent and both installers include the offline WebView2 installer.

## Task 3: Add Proposal-Gated Event and Note Chat Tools

**Files:** `src-tauri/src/ai/tools.rs`, `src-tauri/src/ai/prompt.rs`, `src-tauri/src/db/events.rs`, `src-tauri/src/lib.rs`, `tools/js-checks/proposals.test.mjs`.

- [ ] **Step 1: Add failing DB proposal tests.** In `db/events.rs`, test `apply_event_proposal` for create/update/soft-delete and `apply_note_proposal` for create/update/soft-delete. Use an in-memory DB with the existing migrations; assert updates preserve omitted fields, deletion hides the item from list calls, and missing IDs return `NotFound`.
- [ ] **Step 2: Verify RED.** Run `cargo test apply_event_proposal` and `cargo test apply_note_proposal`; expected: compilation/test failures because proposal-application helpers do not exist.
- [ ] **Step 3: Implement DB proposal helpers.** Add exact signatures `apply_event_proposal(conn: &Connection, action: &str, data: &serde_json::Value, source_entry_id: Option<&str>) -> Result<String, AppError>` and `apply_note_proposal(conn: &Connection, action: &str, data: &serde_json::Value, source_entry_id: Option<&str>) -> Result<String, AppError>`. Accept `create`, `update`, and `delete`; require `title` and RFC3339 `start_at` for event creation, `content` for note creation, and a real `id` for updates/deletes. Use existing `create_*`, `get_*`, `update_*`, and `soft_delete_*` functions; preserve optional fields when omitted and allow explicit null/empty values to clear them. Each test exercises one action and reads the resulting row with `get_event`/`get_note` or verifies a soft-deleted item is absent.
- [ ] **Step 4: Add failing AI tool schema tests.** Assert `get_tools()` exposes `list_notes`, `create_event`, `update_event`, `delete_event`, `create_note`, `update_note`, and `delete_note` with their required fields. Assert mutating tools are described as proposals requiring Review Inbox confirmation.
- [ ] **Step 5: Implement ToolExecutor methods.** Add read-only `list_notes` with a hard cap of 50. Add the six event/note proposal methods, validate required arguments, set `data.type` markers (`event_create`, `event_update`, `event_delete`, `note_create`, `note_update`, `note_delete`), and call `insert_proposed_item` with `item_type` `event` or `note`. For example, an event update proposal stores `{ "type": "event_update", "id": "event-123", "title": "Dentist" }`; `event-123` must come from `list_events`, not model invention.
- [ ] **Step 6: Route acceptance transactionally and idempotently.** Extend the `proposed_action` event/note branches in `lib.rs` to dispatch the new type markers to the DB helpers, then mark the proposal accepted with the created/updated/deleted record ID. Preserve legacy analysis `type: "event"`/`type: "note"` payloads as create actions. Before applying any accept action, check proposal status: apply only `pending`; repeated `accepted` is a no-op; `dismissed` cannot be accepted. Dismiss continues to write nothing.
- [ ] **Step 7: Update prompt and proposal presentation.** In `prompt.rs`, instruct Qwythos to use `list_events`/`list_notes` for real IDs, propose all mutations, and never claim a mutation succeeded before user acceptance. Extend the proposal-card harness only if fields need new rendering; keep it escape-first.
- [ ] **Step 8: Verify GREEN.** Run the focused DB/tool tests, then `cargo test`, all `tools/js-checks/*.test.mjs`, and `cargo fmt --check`. Expected: all new proposal actions pass and all existing task/memory/analysis proposal tests remain green.

## Task 4: Final Phase-1 Release Verification and Documentation

**Files:** `README.md`, `CHANGELOG.md`, `AGENTS.md`.

- [ ] **Step 1: Document the role and network policy.** State that the user manually chooses a local Qwythos chat GGUF and an embedding GGUF; Qwythos handles chat/analysis/tools, embeddings are only for semantic journal search, no model downloads/search/fetch are performed, and WebView2 is bundled offline.
- [ ] **Step 2: Run the final release gates.** Run `cargo test`, `cargo fmt --check`, every Node harness, and `cargo tauri build --bundles msi nsis`. Verify `llama-server.exe --version` from the packaged `engines/` resource; do not launch or migrate the user's vault during this phase without an explicit request.
- [ ] **Step 3: Record exact evidence.** Update the changelog and current build/open-issues status with test counts, installer result, offline installer behavior, and the requirement to stage the ignored `llama-b10952-bin-win-cuda-x64` source folder on clean build machines.

## Scope Boundaries

- Phase 1 does not implement B-01, B-02, the Events/Reminders UI polish, engine auto-recovery, or the remaining VirtualList/E2E work; those are later phases from the approved design.
- Do not add `web_search`, direct URL fetching, model downloads, outbound network calls, new database tables, or unconfirmed writes.
- Do not copy, migrate, or delete legacy AppData. The user's selected clean-start behavior and app-local root from the preceding phase remain in effect.
