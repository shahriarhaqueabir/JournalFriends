# GPU-by-Default & Requirements Disclosure — Design Spec

**Date:** 2026-10-30
**Status:** Approved (brainstorming complete), spec written
**Driver:** "I just want it faster on GPU — why must I set it?" + "the app should say what else I need to download"
**Predecessor:** D-128 (bundled engine/models), D-160 (offline role contract), `2026-10-08-system-tab-board-analysis-design.md` (System tab)

---

## 1. Context & Motivation

The user asked why GPU layers must be set manually, and why the app never says what
else has to be installed. Investigation (all verified, not assumed):

**It is already on the GPU.** `llama_ngl` seeds `"99"` (`db/settings.rs:16`), with
fallbacks at `lib.rs:320` and `lib.rs:2346`; `chat_args()` and `start_embed()` both
pass `-ngl <value>` (`ai/engine.rs:44-45`, `:140-141`). Measured on the dev machine:
loading the 5.24 GiB chat model moved VRAM `2349 → 7583 MiB` (+5234 MiB ≈ model size),
and `-lv 4` reports `offloaded 33/33 layers to GPU`.

**So the real defects are elsewhere:**

1. **The pin is legacy.** `-ngl 99` was copied from the runtime's manual launcher
   (`llama-b10952-bin-win-cuda-x64/start-llama-server-on-gpu.bat`). In the bundled
   build 10952, `-ngl` defaults to `auto` and `--fit` (default `on`) sizes unset
   arguments to VRAM. An explicit `99` overrides both, so a model larger than free
   VRAM fails to load instead of partially offloading.
2. **Nothing proves GPU use.** `start_chat`/`start_embed` spawn with
   `Stdio::piped()` and never read the pipes (`ai/engine.rs:112-113`, `:144-145`).
   Every offload diagnostic is discarded. `preflight.rs` has no GPU awareness
   (no `gpu`/`ngl`/`vram` identifier anywhere). System → AI Engines shows ports and
   health only, so "GPU" and "CPU" look identical to the user.
3. **That unread pipe is also a latent hang.** llama-server writes per-request
   `slot print_timing` lines to stderr; once the ~64 KB pipe buffer fills the child
   blocks on write and stops answering.
4. **Undisclosed dependencies.** Bundled in the installer (`tauri.conf.json`
   `bundle.resources`): `llama-server.exe`, `ggml-cuda.dll`, `cublas64_13.dll`
   (51 MB), `cublasLt64_13.dll` (460 MB), plus the offline WebView2 installer.
   **Not** bundled and never mentioned in-app: the NVIDIA driver, and the
   **Microsoft Visual C++ 2015–2022 x64 runtime** — the bundled binaries import
   `vcruntime140.dll`, `vcruntime140_1.dll` and `msvcp140.dll` (verified by scanning
   the PE import strings of `llama-server.exe`, `llama-server-impl.dll`,
   `ggml-cuda.dll`, `llama.dll`). On a clean machine without the redistributable,
   `llama-server.exe` fails at load with `0xc0000135` and the app only reports
   "engine failed to start". The chat/embed GGUF files are user-supplied by design
   (D-116/D-117).

**Verified detection surface** (bundled build 10952, `-lv 4` → 228 lines / 18 KB,
no DEBUG flood; `-lv 5` is 183 KB and unnecessary):

```
- CUDA0 : NVIDIA GeForce RTX 3060 (12287 MiB, 11255 MiB free)
llama_prepare_model_devices: using device CUDA0 (NVIDIA GeForce RTX 3060) (0000:01:00.0) - 11255 MiB free
load_tensors: offloaded 33/33 layers to GPU
load_tensors:   CPU_Mapped model buffer size =   545.62 MiB
load_tensors:        CUDA0 model buffer size =  4812.25 MiB
llama_kv_cache:      CUDA0 KV buffer size =    64.00 MiB
```

At the default verbosity (3) **none** of these lines are emitted — only `srv`/`cmn`
lines — so "no offload lines in the log" must never be treated as CPU evidence.

## 2. Decisions (from brainstorming)

| #   | Question                         | Decision                                                                                                                                                                 |
| --- | -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Q1  | No-GPU behaviour                 | **Warn, keep running on CPU.** Never block; always tell the truth.                                                                                                       |
| Q2  | GPU layers control               | **Keep the field, default `auto`, explain it.** Text input accepting `auto` / `all` / `0` / `N`; help text "auto = use as much GPU as fits. 0 = CPU only."               |
| Q3  | Where status + requirements live | **System → AI Engines + onboarding step 0 + Settings → LLM Engine inline status.**                                                                                       |
| Q4  | MSVC runtime                     | **Detect and report only.** No bundling, no packaging change.                                                                                                            |
| Q5  | Default value correction         | Default is **`auto`** (llama.cpp's own default, fit-aware), _not_ `all` — `all` is still a hard pin that fails on oversized models. `all` remains accepted in the field. |

## 3. Goals / Non-goals

**Goals**

- Zero configuration yields full GPU offload; the user never has to know what `-ngl` is.
- The app states, in three places, whether the engine is on GPU, partially offloaded, or CPU-only.
- The requirements list (bundled vs. external vs. user-supplied) is visible during onboarding and afterwards.
- Remove the piped-stderr hang risk while gaining the diagnostics.
- No behaviour change on machines where everything already works.

**Non-goals**

- Model downloading or any network fetch (violates D-160 offline boundary).
- Multi-GPU split tuning (`--split-mode`, `--tensor-split`, `--main-gpu`).
- Vulkan/ROCm/Metal backends.
- Bundling or installing the VC++ redistributable (Q4).
- Changing `--jinja`, `-c`, `--api-key`, `--no-warmup`, ports, or the proposal/confirmation model.

## 4. Design

### 4.1 Engine arguments — stop pinning a number

`ai/engine.rs`:

- `EngineConfig.ngl: u32` → `gpu_layers: String`.
- New pure `normalize_gpu_layers(raw: &str) -> String`: trims/lowercases; accepts
  `auto`, `all`, or an integer `0..=400`; anything else (empty, `""`, `"abc"`,
  `"999"`) returns `auto` and emits `tracing::warn!` — same choke-point pattern as
  `db::tasks::normalize_priority`.
- `chat_args()` and `start_embed()` emit `-ngl <normalized>` (still explicit, so a
  swapped-in older binary cannot regress to CPU — same rationale as explicit `--jinja`).
- `--fit` is left at its default (`on`) and is **not** passed; nothing else changes.

Settings/read paths: `lib.rs:320` and `lib.rs:2346` read `llama_ngl` as a `String`
(default `"auto"`, then `normalize_gpu_layers`). `settings.rs` seed becomes
`("llama_ngl", "auto")`.

**Migration** `src-tauri/migrations/018_gpu_layers_auto.sql` — data only, no schema
change: rewrite the legacy seeded value so existing installs stop hard-pinning.

```sql
UPDATE app_settings SET value = 'auto' WHERE key = 'llama_ngl' AND value = '99';
```

The runner wraps each file in a rusqlite transaction and strips an outer
`BEGIN;`/`COMMIT;` pair, so the file may follow the 016/017 style. (Note: the
"Conventions" section of `AGENTS.md` still claims migrations must contain
BEGIN/COMMIT while the "File Structure" section and `migrations.rs` say the runner
supplies it — the code is authoritative; flagged for a docs fix, not part of this work.)

### 4.2 Stderr drain (fixes the hang, supplies the diagnostics)

New `ai/engine_log.rs`:

- `spawn_log_reader<R: AsyncRead + Unpin>(stream, label, ring, log_path) -> JoinHandle<()>`:
  reads lines, strips ANSI escapes, pushes into a `Mutex<VecDeque<String>>` capped at
  200 lines, appends to `<data>/logs/llama-<label>.log`, and feeds each line to the parser.
- Log rotation: before appending, if the file exceeds 2 MiB, rename to `.1` (single
  generation) and start fresh. Failures are logged via `tracing` and never fatal.
- Both servers spawn with `-lv 4` added to their args and `Stdio::piped()` for
  stderr/stdout, with both streams drained (stdout too — it is currently piped and unread).
- `stop_chat`/`stop_embed` abort the reader tasks after `kill()`/`wait()`.

### 4.3 Parsing contract

`ai/engine_info.rs`, all pure and unit-tested:

| Field                  | Source line (regex, ANSI-stripped)               |
| ---------------------- | ------------------------------------------------ |
| device id + name       | `using device (\S+) \(([^)]+)\)`                 |
| VRAM total/free        | `- (\w+) +: (.+?) \((\d+) MiB, (\d+) MiB free\)` |
| layers offloaded/total | `offloaded (\d+)/(\d+) layers to GPU`            |
| model buffer           | `(\w+) model buffer size =\s+([\d.]+) MiB`       |
| KV buffer              | `(\w+) KV buffer size =\s+([\d.]+) MiB`          |

Derived state:

- `Backend::Gpu` — device line present and `offloaded n/n` with `n > 0`.
- `Backend::Partial` — device line present and `0 < n < total`.
- `Backend::Cpu` — no device line, or `offloaded 0/n`.
- `Backend::Unknown` — engine not started yet / lines not seen.

Parsers are incremental (`EngineInfoAccumulator::feed(&str)`), so partial logs still
yield useful state. ANSI stripping is mandatory: `--log-file` output contains colour
escapes even though a redirected pipe does not.

### 4.4 State + wire contract (D-148)

```rust
#[derive(Debug, Clone, Serialize, Default)]
pub struct EngineRuntimeInfo {
    pub backend: String,          // "gpu" | "partial" | "cpu" | "unknown"
    pub device: Option<String>,   // "CUDA0"
    pub device_name: Option<String>, // "NVIDIA GeForce RTX 3060"
    pub layers_offloaded: u32,
    pub layers_total: u32,
    pub model_mib: Option<f64>,
    pub kv_mib: Option<f64>,
    pub vram_total_mib: Option<u64>,
    pub vram_free_mib: Option<u64>,
}
```

- Stored per service on `LlamaEngine` (`chat_info()`, `embed_info()`), surfaced by a
  new command `engine_runtime_info` returning `{ chat: EngineRuntimeInfo, embed: EngineRuntimeInfo }`.
- Args: none (so no camelCase concerns). Return and event payloads are snake_case
  per D-148; `wire_tests` gains a serde-shape assertion.
- New `AppEvent::EngineRuntimeStatus { chat: EngineRuntimeInfo, embed: EngineRuntimeInfo }`
  emitted after provisioning/restart completes (and on the first parse that resolves
  `unknown` → known), so the UI updates without polling.
- Existing `LlamaEngineStarted`/`LlamaEngineFailed` are unchanged.

### 4.5 Truth in the UI

- **System → AI Engines** (`views/system.html`, `tabs/system.js`): one new `GPU` row
  per service — `RTX 3060 · 33/33 layers · 4.8 GiB VRAM` / `partial: 18/33 layers` /
  `CPU only — no CUDA device` / `—` while unknown. Warning styling (existing
  `hj-badge` warn variant) for `partial` and `cpu`. Reads `engine_runtime_info` on
  tab load and on `EngineRuntimeStatus`.
- **Settings → LLM Engine**: `#llama-ngl` becomes `type="text"` with
  `value="auto"`, `pattern` validation mirrored in JS, and a `field-help` line that
  explains the values; below it an inline status line with the same wording as the
  System row.
- **Toast on provision** (`notifications.js`): only when `backend != gpu` —
  `Running on CPU (no CUDA device detected) — chat will be slow.` or
  `Partial GPU offload: 18/33 layers in VRAM (model larger than free VRAM).`
  Silent when fully on GPU (no new noise for the working case).

### 4.6 Requirements disclosure

One shared source of truth: `src/js/requirements.js` exporting
`requirementRows({ gpu, runtime })` so onboarding and System render identical content.

| Requirement                                | Bundled?             | Display                                                               |
| ------------------------------------------ | -------------------- | --------------------------------------------------------------------- |
| llama.cpp engine + CUDA libraries          | ✅ bundled           | `Included`                                                            |
| WebView2 runtime                           | ✅ bundled (offline) | `Included`                                                            |
| NVIDIA driver                              | ❌ external          | `Detected — RTX 3060` / `Not detected — GPU disabled, running on CPU` |
| Microsoft Visual C++ 2015–2022 x64 runtime | ❌ external          | `Detected` / `Missing — engine cannot start` + download link          |
| Chat model (GGUF, instruct-tuned)          | ❌ you supply        | `Imported: <filename>` / `Not imported`                               |
| Embedding model (GGUF)                     | ❌ you supply        | `Imported: <filename>` / `Not imported (semantic search disabled)`    |

- **Onboarding step 0** gains the table above the existing welcome copy, rendered from
  the same module; no new wizard step (Q3).
- **System** gains a `Requirements` card under the engines card.

### 4.7 Preflight

Two new checks in `preflight_run` (`preflight.rs`), following check 3's shape
(`PreflightCheck { id, label, status, message, actionable }`):

1. **`gpu_offload`** — run `llama-server.exe --list-devices` (fast, no model load;
   ~200 ms), parse `CUDA0: <name> (<total> MiB, <free> MiB free)`. Pass with the device
   name; Warn (`actionable: true`) with "No CUDA device — Arnold will run on CPU" when
   none. If the binary cannot be executed at all, Fail with the spawn error (this is
   also where a missing MSVC runtime surfaces first).
2. **`msvc_runtime`** — `LoadLibraryW("vcruntime140.dll")` and `("msvcp140.dll")`
   (via the `windows-sys`/`windows` crate already in the tree if present, else a thin
   `extern "system"` declaration). Pass when both load; Warn with the Microsoft
   redistributable link when not.

`PreflightReport.healthy` semantics are unchanged (warnings do not fail the report).

## 5. Data flow

```
provision_model / restart_engine
  → LlamaEngine::start_chat/start_embed  (-ngl <normalized>, -lv 4, piped stdio)
  → spawn_log_reader (stdout + stderr) → ring buffer + <data>/logs/llama-*.log
                                      → EngineInfoAccumulator::feed
  → LlamaEngine::chat_info()/embed_info()  (Mutex<EngineRuntimeInfo>)
  → AppEvent::EngineRuntimeStatus  ──► app.js single listener
  → engine_runtime_info command    ──► System GPU row / Settings inline status
  → toast on non-gpu backend
```

## 6. Error handling & edge cases

| Case                            | Behaviour                                                                                  |
| ------------------------------- | ------------------------------------------------------------------------------------------ |
| No CUDA device                  | `backend: cpu`, warn in System/Settings, one toast, engine still runs (Q1).                |
| Driver too old for CUDA 13      | CUDA init fails → same as no device; log line captured verbatim in `llama-chat.log`.       |
| Model larger than VRAM          | `auto` + `--fit` offloads what fits → `partial` with the real ratio.                       |
| `--list-devices` fails to spawn | Preflight `gpu_offload` Fail with the OS error; requirements card shows runtime `Missing`. |
| Log file unwritable             | Reader still parses in memory; file write errors are `tracing`-only.                       |
| Invalid `llama_ngl` value       | `normalize_gpu_layers` → `auto` + warn; never blocks engine start.                         |
| Engine stopped                  | `backend: unknown`, row renders `—`.                                                       |
| Embed server                    | Same treatment; its own row and log file.                                                  |

## 7. Testing

**Rust unit tests**

- `normalize_gpu_layers`: `"auto"→auto`, `"ALL"→all`, `"0"→0`, `"99"→99`,
  `""→auto`, `"abc"→auto`, `"999"→auto` (out of range).
- `chat_args_*`: default config emits `-ngl auto`; explicit `0` emits `-ngl 0`;
  invalid input emits `-ngl auto`. Existing `--jinja`/`-c`/`--api-key` assertions stay.
- Parser: full-GPU fixture (the five real lines in §1) → `backend: gpu`,
  `33/33`, `model_mib ≈ 4812.25`; `offloaded 0/33` → `cpu`; missing device line →
  `cpu`; no lines → `unknown`; ANSI-coloured input parses identically.
- `--list-devices` parser against the real two-line output (device present / absent).
- Ring buffer caps at 200 lines; rotation renames at the threshold.
- `wire_tests`: `EngineRuntimeInfo` serialises snake_case.
- Migration 018: legacy `'99'` → `'auto'`; a deliberate `'40'` is untouched.

**Node harnesses** (`tools/js-checks/`)

- `requirements.test.mjs`: row rendering for all four status permutations (gpu/cpu ×
  runtime present/missing), escape-first.
- `engine-status.test.mjs`: the three status strings for System + Settings.
- `design.test.mjs` must stay green for any new UI.

**Manual verification**

1. Provision → `nvidia-smi --query-gpu=memory.used` delta ≈ model size; System shows
   `RTX 3060 · 33/33`.
2. Set GPU layers `0` → restart → System shows `CPU only`, toast appears, VRAM delta ≈ 0.
3. Rename `vcruntime140.dll` access (or test on a clean VM) → requirements shows
   `Missing`, preflight warns, engine start error is legible.
4. 300+ chat turns → server stays responsive (stderr drain regression check).

## 8. Risks

- **Stderr parsing is a log-format dependency.** Mitigation: parsers are tolerant and
  `Unknown`/`Cpu` are non-fatal; `--list-devices` provides an independent device check;
  a build bump only degrades the row to `—`, never breaks the engine.
- **`-lv 4` raises log volume.** Bounded by the 200-line ring and 2 MiB rotation.
- **`auto` changes load behaviour** for users who deliberately pinned a number; the
  migration only rewrites the seeded `99`, so deliberate values survive.
- **`LoadLibrary` probing** can produce false "missing" if the DLL is app-local only;
  check both the process search path and `engines/` before declaring it missing.

## 9. Out of scope

Model download/management UI, multi-GPU tuning, alternative backends, VC++
redistributable bundling, changing the 18-tool AI surface, and any change to the
proposal/confirmation flow.

## 10. Files touched (estimate)

- `src-tauri/src/ai/engine.rs` — `gpu_layers`, `-lv 4`, reader spawn, `chat_info`/`embed_info`
- `src-tauri/src/ai/engine_log.rs` (new) — reader, ring buffer, rotation, ANSI strip
- `src-tauri/src/ai/engine_info.rs` (new) — accumulator + parsers
- `src-tauri/src/events.rs` — `EngineRuntimeStatus`
- `src-tauri/src/lib.rs` — settings reads, `engine_runtime_info`, `wire_tests`, spawn wiring
- `src-tauri/src/preflight.rs` — `gpu_offload`, `msvc_runtime`
- `src-tauri/src/db/settings.rs` — seed `auto`
- `src-tauri/migrations/018_gpu_layers_auto.sql` (new)
- `src/index.html` — `#llama-ngl` → text + help + inline status; requirements table (onboarding pane 0)
- `src/js/tabs/settings.js`, `src/js/tabs/system.js`, `src/js/onboarding.js`, `src/js/notifications.js`
- `src/js/requirements.js` (new), `src/styles/surfaces/system.css`
- `tools/js-checks/requirements.test.mjs` (new), `tools/js-checks/engine-status.test.mjs` (new)
