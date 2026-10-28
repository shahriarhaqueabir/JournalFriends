# 🎨 Arnold — Offline-First Desktop Productivity

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Tauri Version](https://img.shields.io/badge/Tauri-v2-blue.svg)](https://v2.tauri.app/)
[![Rust Version](https://img.shields.io/badge/Rust-1.77%2B-orange.svg)](https://www.rust-lang.org/)
![AI Engine: llama.cpp](https://img.shields.io/badge/AI_Engine-llama.cpp-green.svg)

**Arnold** is a private, offline-first Windows desktop application built with **Tauri v2**. It seamlessly combines a plain-text journal, a full task manager, and a local AI assistant powered by a local **llama.cpp** engine using OpenAI-compatible HTTP on ports 8080/8081, allowing for secure, local processing of your personal data.

---

## 📸 Showcase

> *Screenshots coming soon — including the AI Thinking Partner sidebar, Journal Analysis pipeline, and the new Future Awareness schedule view.*

---

## 🚀 Key Features

- **Private by Design**: All data stays on your machine in a single SQLite file.
- **Journal-to-Task Pipeline**: Background AI analysis extracts actionable tasks from your journal entries.
- **Local AI**: Powered by local **llama.cpp** (no internet, no subscriptions).
- **Task Management**: Full kanban/list views with subtasks and recurring reminders.
- **Offline-First**: Zero dependencies on cloud services or external APIs (except the local llama.cpp engine).

---

## 🛠️ Tech Stack

| Layer | Technology |
| --- | --- |
| **Desktop Shell** | [Tauri v2](https://v2.tauri.app/) |
| **Backend** | [Rust](https://www.rust-lang.org/) (Stable 1.77+, MSVC toolchain) |
| **Frontend** | Vanilla HTML5, CSS3 (Modern Grid), JS (ES2022) |
| **AI Engine** | llama.cpp (local, chat :8080 + embed :8081) |
| **Database** | SQLite via [rusqlite](https://github.com/rusqlite/rusqlite) (bundled-full) |
| **Frontend Libs** | Chart.js 4.x, SortableJS, Floating UI, Lucide Icons (all vendored locally; no CDN) |
| **Async Runtime** | Tokio |

---

## 📦 Getting Started

### Requirements

| Requirement | Needed for | Notes |
| --- | --- | --- |
| **Windows 10/11** | Runtime | WebView2 runtime is required to render the UI. The installer bundles Microsoft's **offline** WebView2 installer (`webviewInstallMode: offlineInstaller`), so setup never downloads anything. |
| **llama-server (llama.cpp)** | Runtime (AI) | Windows release bundles include the llama-server runtime and its DLLs. The app manages its lifecycle (start/stop/health); `llama_exe_path` remains available as an override. |
| **AI models** | Runtime (AI) | Two local GGUF files, selected manually — nothing is downloaded. The chat role takes **any instruct-tuned GGUF** (Qwythos-9B-Claude-Mythos-5-1M-Q4_K_M recommended - the model the prompts and tools are tuned against); it handles chat, journal analysis, and tools. Embedding models are rejected here. The embedding role is only used to index and semantically retrieve journal entries (**`nomic-embed-text-v2-moe`** recommended). Files are copied into `<app directory>/data/models/chat/` and `models/embed/`. |
| **Rust 1.77+ (MSVC)** | Building only | Not required for end users — only to build from source. Install via [rustup.rs](https://rustup.rs/), plus the [Tauri v2 Windows prerequisites](https://v2.tauri.app/start/prerequisites/) (WebView2 SDK, MSVC Build Tools). |

### Initializing the AI

Windows release bundles include the llama-server runtime. From source, stage the complete `llama-b10952-bin-win-cuda-x64` runtime folder at the repository root before `cargo tauri build`; the folder is ignored by Git and must be supplied to each build workspace. Choose the chat and embedding GGUFs manually through the first-run wizard or Settings → LLM Engine (file pickers only — there is no download action). Provisioning starts both local services without internet access.

**Model roles:** the chat role accepts any instruct-tuned GGUF (Qwythos recommended, embedding models rejected). The embedding GGUF (Nomic recommended) is reserved for semantic journal search and vector indexing; it is never used for chat or analysis.

**No external networking:** the AI has no URL-fetch, web-search, or download tool, and the app makes no outbound requests. All HTTP is loopback to the app-managed llama.cpp servers, and the UI's Content-Security-Policy allows no remote origins.

The app runs one chat service and one embedding service. The System tab's Models cell is an inventory by import folder, not the active selection; the active chat and embedding models are selected separately in Settings. A GGUF can appear in both inventory roles if imported into both folders.

### Data Location

By default, the database, logs, models, backups, exports, and attachments live under `<directory containing the app executable>/data/`. `ARNOLD_DATA_DIR` overrides this for tests or custom setups. Existing `%APPDATA%/Arnold` data is not migrated or deleted; the app starts a fresh local vault when the new data directory is empty.

Paths follow the executable's actual install location. The NSIS `currentUser` installer places the app under `%LOCALAPPDATA%`, so its adjacent data folder is still under the user profile. To keep data outside the profile, place the app in a writable non-user directory; installing under protected `Program Files` is not writable for normal users without additional installer permissions.

### Portable Bundle

After staging the llama.cpp runtime folder and running `cargo tauri build`, run `powershell -ExecutionPolicy Bypass -File bundle-portable.ps1`. This creates `src-tauri/target/release/bundle/portable/Arnold-<version>-portable.zip`, containing the app, its library DLL, and bundled engine runtime. Extract the complete folder to a writable non-user location; the app creates its `data/` directory on first launch.

### Running the App

```bash
# Clone the repository
git clone https://github.com/shahriarhaqueabir/arnold.git
cd arnold

# Run in development mode
cargo tauri dev
```

---

## 🏗️ Project Structure

- `src-tauri/`: Rust backend, database migrations, and AI orchestrator.
- `src/`: Vanilla JS frontend, layout grid, and UI components.
- `AgentDocs/`: Detailed system specifications and architectural blueprints.
- `data/`: Local database storage (created on first run).

---

## 🔒 Security & Privacy

- **No Cloud**: There is no "account" and no "sync." Your data is yours.
- **Offline by construction**: no web-search, URL-fetch, or model-download tool exists; the only HTTP traffic is loopback to the local llama.cpp servers.
- **Local Analytics**: AI processing happens entirely on your CPU/GPU via llama.cpp.
- **Confirmation-gated writes**: every AI change to a task, event, note, or memory is a proposal the user accepts in the Review Inbox; the AI never writes directly.
- **Audit Logs**: All database mutations are tracked locally for your review.

---

## 📜 License

MIT License - Copyright (c) 2026 Arnold.
