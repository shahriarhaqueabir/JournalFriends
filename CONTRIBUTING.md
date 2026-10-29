# Contributing to Arnold

Thanks for your interest in contributing. Arnold is a private, offline-first
desktop app, so contributions are welcome but should respect that constraint.

## Before You Start

- Read `AGENTS.md` for the project's conventions, locked decisions (D-01..D-160),
  and the session checklists. It is the source of truth for how the codebase is
  shaped.
- Search existing issues before opening a new one.

## Development Setup

1. Install the [Tauri v2 Windows prerequisites](https://v2.tauri.app/start/prerequisites/)
   (Rust 1.77+ MSVC, WebView2 SDK, MSVC Build Tools).
2. Stage the llama.cpp runtime folder (`llama-b10952-bin-win-cuda-x64`) at the
   repository root. It is gitignored and must be supplied locally.
3. `cargo tauri dev` to run in development mode.

## Code Standards

- Rust: `cargo fmt` and `cargo check` must pass; `cargo test` for the crate.
- Frontend: `node --check` on every changed JS file; the Node harnesses in
  `tools/js-checks/` must pass.
- All `#[tauri::command]` handlers return `Result<T, AppError>`. No panics.
- All DB writes go through the write mutex. All events go through the `AppEvent`
  enum. All paths resolve through `AppPaths`.
- Top-level command args are camelCase (D-148); nested payloads/returns are
  snake_case.

## Pull Requests

- Keep PRs focused on one concern.
- Include tests for new behavior.
- Update `CHANGELOG.md` and `AGENTS.md` when behavior or conventions change.
- Conventional commit messages (`feat:`, `fix:`, `docs:`, ...).

## What We Will Not Merge

- Anything that adds network access to the AI (web search, URL fetch, download).
- Anything that stores data outside the local SQLite vault.
- Anything that requires a cloud account or API key.
