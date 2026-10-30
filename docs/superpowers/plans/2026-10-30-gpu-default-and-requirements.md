# GPU-by-Default & Requirements Disclosure — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make full GPU offload the zero-config default, prove and display whether the engine is actually on the GPU, and disclose every dependency the installer does not ship — without naming llama.cpp anywhere outside one probe module.

**Architecture:** A new `ai::probe` module owns all engine-specific knowledge (CLI flags, log format, device listing) behind a six-method `EngineProbe` trait; `LlamaCppProbe` is the only implementation. `ai::engine_log` drains both piped stdio streams (fixing a latent hang) and feeds each stderr line into the probe. `LlamaEngine` exposes `chat_info()`/`embed_info()` returning the engine-neutral `EngineRuntimeInfo` wire struct, surfaced by a new `engine_runtime_info` command and an `EngineRuntimeStatus` event. A `engine_requirements` command returns device/runtime/model facts for one shared requirements table rendered in onboarding and System. Preflight gains `gpu_offload` and `msvc_runtime` checks.

**Tech Stack:** Rust (Tauri v2, tokio, regex, rusqlite), vanilla ES-module frontend (no bundler), Node `assert` harnesses under `tools/js-checks/`.

**Spec:** `docs/superpowers/specs/2026-10-30-gpu-default-and-requirements-design.md`

---

## File Structure

**Create**

| File                                           | Responsibility                                                                                             |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/ai/probe/mod.rs`                | `EngineProbe` trait, `DeviceInfo`, `ProbeError`, `EngineRuntimeInfo`, backend constants, `default_probe()` |
| `src-tauri/src/ai/probe/llama_cpp.rs`          | Everything llama.cpp: `normalize_gpu_layers`, spawn args, `--list-devices` parsing, `-lv 4` log parsing    |
| `src-tauri/src/ai/engine_log.rs`               | ANSI strip, capped ring buffer, log rotation, stdout drain, stderr→probe reader                            |
| `src-tauri/src/runtime_env.rs`                 | `msvc_runtime_present()` (Windows DLL load probe)                                                          |
| `src-tauri/migrations/018_gpu_layers_auto.sql` | Data-only: legacy `llama_ngl = '99'` → `'auto'`                                                            |
| `src/js/requirements.js`                       | `requirementRows()` + `requirementsTableHtml()` — single source of truth for both surfaces                 |
| `src/js/engine-status.js`                      | `backendLabel()`, `inlineStatus()`, `gpuWarningOnce()` — pure, node-testable                               |
| `tools/js-checks/requirements.test.mjs`        | Harness for `requirements.js`                                                                              |
| `tools/js-checks/engine-status.test.mjs`       | Harness for `engine-status.js`                                                                             |

**Modify**

| File                                             | Change                                                                                                                                              |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/ai/mod.rs`                        | `pub mod engine_log; pub mod probe;`                                                                                                                |
| `src-tauri/src/ai/engine.rs`                     | `gpu_layers: String`, `log_dir`, probe fields, `chat_args`/`embed_args` take a probe, reader spawn/abort, `chat_info`/`embed_info`/`last_log_lines` |
| `src-tauri/src/events.rs`                        | `EngineRuntimeStatus { chat, embed }`                                                                                                               |
| `src-tauri/src/lib.rs`                           | settings read as `String`, `EngineConfig` at 2 sites, `engine_runtime_info` + `engine_requirements` commands, `wire_tests`                          |
| `src-tauri/src/provision.rs`                     | emit `EngineRuntimeStatus`, log the stderr tail on failure                                                                                          |
| `src-tauri/src/preflight.rs`                     | `gpu_offload` + `msvc_runtime` checks                                                                                                               |
| `src-tauri/src/db/settings.rs`                   | seed `("llama_ngl", "auto")`                                                                                                                        |
| `src-tauri/src/db/migrations.rs`                 | migration-018 test                                                                                                                                  |
| `src/index.html`                                 | `#llama-ngl` → text input + help + `#llama-ngl-status`; onboarding pane 0 requirements container                                                    |
| `src/js/tabs/settings.js`                        | `auto` default, inline status                                                                                                                       |
| `src/js/tabs/system.js`, `src/views/system.html` | engine runtime table + Requirements card                                                                                                            |
| `src/js/onboarding.js`                           | render requirements into pane 0                                                                                                                     |
| `src/js/app.js`                                  | `engine_runtime_status` → System refresh + chat GPU-fallback toast                                                                                  |
| `src/styles/surfaces/system.css`                 | `.requirements-table`, `.req-state`                                                                                                                 |
| `AGENTS.md`, `CHANGELOG.md`                      | decisions D-161/D-162, change log                                                                                                                   |

**Ordering note:** Tasks 1–4 build pure, independently testable units. Task 5 wires them into the engine. Tasks 6–8 expose them. Tasks 9–12 are the frontend. Task 13 is docs + full verification.

---

### Task 1: Probe types and trait

**Files:**

- Create: `src-tauri/src/ai/probe/mod.rs`
- Modify: `src-tauri/src/ai/mod.rs`

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/src/ai/probe/mod.rs` (create the file with only this block for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_runtime_info_is_unknown_and_serialises_snake_case() {
        let info = EngineRuntimeInfo::default();
        assert_eq!(info.backend, BACKEND_UNKNOWN);
        let v = serde_json::to_value(&info).expect("must serialise");
        for key in [
            "engine",
            "backend",
            "device",
            "device_name",
            "layers_offloaded",
            "layers_total",
            "model_mib",
            "kv_mib",
            "vram_total_mib",
            "vram_free_mib",
        ] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        assert!(v.get("deviceName").is_none(), "camelCase leak");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml probe::tests`
Expected: FAIL — `cannot find type EngineRuntimeInfo`, `unresolved module ai::probe`.

- [ ] **Step 3: Write minimal implementation**

Add to `src-tauri/src/ai/mod.rs`, alphabetically between `pub mod engine;` and
`pub mod keywords;`:

```rust
pub mod probe;
```

Add **only** `probe` here. `pub mod engine_log;` belongs to Task 4 — declaring a
module whose file does not exist yet is a hard `E0583` compile error, so this task
would fail its own verification step.

Write the top of `src-tauri/src/ai/probe/mod.rs` (above the test module):

```rust
//! Engine-neutral probe boundary.
//!
//! Everything the app knows about a specific inference engine lives behind
//! `EngineProbe`. The UI, preflight, events, and the wire contract only ever
//! see `EngineRuntimeInfo` / `DeviceInfo`, so replacing llama.cpp with a newer
//! runtime later is an added module, not a rewrite (D-161).

use serde::Serialize;
use std::ffi::OsString;
use std::path::Path;

/// `EngineRuntimeInfo.backend` values. Plain constants (not an enum) so the
/// wire stays a string any future engine can also produce.
pub const BACKEND_GPU: &str = "gpu";
pub const BACKEND_PARTIAL: &str = "partial";
pub const BACKEND_CPU: &str = "cpu";
pub const BACKEND_UNKNOWN: &str = "unknown";

/// One accelerator the engine can use.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub total_mib: u64,
    pub free_mib: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeError(pub String);

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ProbeError {}

/// Stable, engine-neutral runtime description. This is the wire contract
/// (D-148: snake_case). `engine` is the probe's label, never a hardcoded
/// string in the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineRuntimeInfo {
    pub engine: String,
    /// One of `BACKEND_GPU` / `BACKEND_PARTIAL` / `BACKEND_CPU` / `BACKEND_UNKNOWN`.
    pub backend: String,
    pub device: Option<String>,
    pub device_name: Option<String>,
    pub layers_offloaded: u32,
    pub layers_total: u32,
    pub model_mib: Option<f64>,
    pub kv_mib: Option<f64>,
    pub vram_total_mib: Option<u64>,
    pub vram_free_mib: Option<u64>,
}

impl Default for EngineRuntimeInfo {
    fn default() -> Self {
        Self {
            engine: String::new(),
            backend: BACKEND_UNKNOWN.to_string(),
            device: None,
            device_name: None,
            layers_offloaded: 0,
            layers_total: 0,
            model_mib: None,
            kv_mib: None,
            vram_total_mib: None,
            vram_free_mib: None,
        }
    }
}

/// One engine implementation. Six methods is the whole contract — it must not
/// grow past what the UI actually reads.
pub trait EngineProbe: Send + Sync {
    /// Human name shown in the requirements table ("llama.cpp").
    fn engine_label(&self) -> &'static str;
    /// Spawn args this engine needs for GPU + diagnostics.
    fn gpu_args(&self, gpu_layers: &str) -> Vec<OsString>;
    /// Enumerate accelerators without loading a model.
    fn list_devices(&self, exe: &Path) -> Result<Vec<DeviceInfo>, ProbeError>;
    /// Consume one drained log line. Tolerant: never fails, never panics.
    fn feed(&mut self, line: &str);
    /// Clear parsed state before a fresh start (restart / engine swap).
    fn reset(&mut self);
    /// Current best-known state.
    fn snapshot(&self) -> EngineRuntimeInfo;
}
```

Task 2 creates `ai/probe/llama_cpp.rs` and adds the wiring to this module:

```rust
pub mod llama_cpp;
pub use llama_cpp::{strip_ansi, LlamaCppProbe};

/// The engine the app ships with today.
pub fn default_probe() -> Box<dyn EngineProbe> {
    Box::new(LlamaCppProbe::default())
}
```

`strip_ansi` is re-exported because `ai::engine_log` (Task 4) needs it and must not
reach into a specific engine module. Nothing references the implementation yet, so
Task 1 compiles on its own.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml probe::tests`
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ai/mod.rs src-tauri/src/ai/probe/mod.rs
git commit -m "feat(ai): add engine-neutral probe trait and runtime info wire contract"
```

---

### Task 2: llama.cpp spawn args and GPU-layer normalisation

**Files:**

- Create: `src-tauri/src/ai/probe/llama_cpp.rs`
- Modify: `src-tauri/src/ai/probe/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/ai/probe/llama_cpp.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_gpu_layer_values() {
        assert_eq!(normalize_gpu_layers("auto"), "auto");
        assert_eq!(normalize_gpu_layers("AUTO"), "auto");
        assert_eq!(normalize_gpu_layers(" all "), "all");
        assert_eq!(normalize_gpu_layers("0"), "0");
        assert_eq!(normalize_gpu_layers("99"), "99");
        assert_eq!(normalize_gpu_layers("400"), "400");
        assert_eq!(normalize_gpu_layers(""), "auto");
        assert_eq!(normalize_gpu_layers("abc"), "auto");
        assert_eq!(normalize_gpu_layers("401"), "auto");
        assert_eq!(normalize_gpu_layers("-1"), "auto");
    }

    #[test]
    fn gpu_args_always_pin_ngl_and_raise_verbosity() {
        let probe = LlamaCppProbe::default();
        let args: Vec<String> = probe
            .gpu_args("99")
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["-ngl", "99", "-lv", "4"]);

        let fallback: Vec<String> = probe
            .gpu_args("not-a-number")
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(fallback, vec!["-ngl", "auto", "-lv", "4"]);
    }

    #[test]
    fn engine_label_is_llama_cpp() {
        assert_eq!(LlamaCppProbe::default().engine_label(), "llama.cpp");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml llama_cpp`
Expected: FAIL — `cannot find function normalize_gpu_layers`, `cannot find type LlamaCppProbe`.

- [ ] **Step 3: Write minimal implementation**

Above the test module in `src-tauri/src/ai/probe/llama_cpp.rs`:

```rust
//! llama.cpp implementation of `EngineProbe`.
//!
//! This is the ONLY module allowed to know llama.cpp's CLI flags or its log
//! line formats. A build bump that renames a log line degrades the status row
//! to `unknown`; it never changes engine behaviour or fails a start.

use super::{
    DeviceInfo, EngineProbe, EngineRuntimeInfo, ProbeError, BACKEND_CPU, BACKEND_GPU,
    BACKEND_PARTIAL, BACKEND_UNKNOWN,
};
use regex::Regex;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

const ENGINE_LABEL: &str = "llama.cpp";

/// Verbosity that emits the device / offload / VRAM lines the status row reads.
/// At the default (3) llama.cpp prints none of them, so "no offload lines" is
/// NOT evidence of CPU execution.
const DIAG_VERBOSITY: &str = "4";

/// Highest layer count we accept. Above this we fall back to `auto` rather than
/// forwarding a typo that would make the server refuse to start.
const MAX_GPU_LAYERS: u32 = 400;

/// Choke point for the `llama_ngl` setting: accepts `auto`, `all`, or an
/// integer `0..=400`; anything else becomes `auto` with a warning (same pattern
/// as `db::tasks::normalize_priority`).
pub fn normalize_gpu_layers(raw: &str) -> String {
    let value = raw.trim().to_ascii_lowercase();
    if value == "auto" || value == "all" {
        return value;
    }
    if let Ok(n) = value.parse::<u32>() {
        if n <= MAX_GPU_LAYERS {
            return n.to_string();
        }
    }
    tracing::warn!(
        target: "ai::probe",
        value = %raw,
        "invalid gpu layers value - falling back to auto"
    );
    "auto".to_string()
}

#[derive(Debug, Default)]
pub struct LlamaCppProbe {
    parsed: Parsed,
}

impl EngineProbe for LlamaCppProbe {
    fn engine_label(&self) -> &'static str {
        ENGINE_LABEL
    }

    fn gpu_args(&self, gpu_layers: &str) -> Vec<OsString> {
        vec![
            "-ngl".into(),
            normalize_gpu_layers(gpu_layers).into(),
            "-lv".into(),
            DIAG_VERBOSITY.into(),
        ]
    }
}
```

Then wire the implementation into `src-tauri/src/ai/probe/mod.rs` (above the `pub trait`
block, after the constants):

```rust
pub mod llama_cpp;
pub use llama_cpp::LlamaCppProbe;

/// The engine the app ships with today.
pub fn default_probe() -> Box<dyn EngineProbe> {
    Box::new(LlamaCppProbe::default())
}
```

Re-export **only** `LlamaCppProbe` here. `strip_ansi` is added to this `pub use` in
Task 3, where the function is actually defined — re-exporting a name that does not
exist yet is a compile error.

Also add the placeholder struct the impl needs so the crate compiles at this stage:

```rust
#[derive(Debug, Default)]
struct Parsed {
    saw_load: bool,
    device: Option<String>,
    device_name: Option<String>,
    vram_total: Option<u64>,
    vram_free: Option<u64>,
    layers_offloaded: u32,
    layers_total: u32,
    model_buffers: Vec<(String, f64)>,
    kv_buffers: Vec<(String, f64)>,
}
```

Add stubs for the remaining trait methods so the trait is fully implemented:

```rust
    fn list_devices(&self, _exe: &Path) -> Result<Vec<DeviceInfo>, ProbeError> {
        Ok(Vec::new())
    }

    fn feed(&mut self, _line: &str) {}

    fn reset(&mut self) {
        self.parsed = Parsed::default();
    }

    fn snapshot(&self) -> EngineRuntimeInfo {
        EngineRuntimeInfo {
            engine: ENGINE_LABEL.to_string(),
            ..EngineRuntimeInfo::default()
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml llama_cpp`
Expected: `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ai/probe/llama_cpp.rs src-tauri/src/ai/probe/mod.rs
git commit -m "feat(ai): normalise gpu layers and build llama.cpp spawn args"
```

---

### Task 3: llama.cpp log parsing and device listing

**Files:**

- Modify: `src-tauri/src/ai/probe/llama_cpp.rs`

- [ ] **Step 1: Write the failing test**

Append to the `tests` module:

```rust
    /// The real lines emitted by bundled build 10952 at `-lv 4`.
    const GPU_LOG: &str = "\
- CUDA0 : NVIDIA GeForce RTX 3060 (12287 MiB, 11255 MiB free)
llama_prepare_model_devices: using device CUDA0 (NVIDIA GeForce RTX 3060) (0000:01:00.0) - 11255 MiB free
load_tensors: offloaded 33/33 layers to GPU
load_tensors:   CPU_Mapped model buffer size =   545.62 MiB
load_tensors:        CUDA0 model buffer size =  4812.25 MiB
llama_kv_cache:      CUDA0 KV buffer size =    64.00 MiB";

    fn feed_all(probe: &mut LlamaCppProbe, text: &str) {
        for line in text.lines() {
            probe.feed(line);
        }
    }

    #[test]
    fn full_offload_log_yields_gpu_backend() {
        let mut probe = LlamaCppProbe::default();
        feed_all(&mut probe, GPU_LOG);
        let info = probe.snapshot();
        assert_eq!(info.backend, BACKEND_GPU);
        assert_eq!(info.device.as_deref(), Some("CUDA0"));
        assert_eq!(info.device_name.as_deref(), Some("NVIDIA GeForce RTX 3060"));
        assert_eq!((info.layers_offloaded, info.layers_total), (33, 33));
        assert_eq!(info.model_mib, Some(4812.25));
        assert_eq!(info.kv_mib, Some(64.0));
        assert_eq!(info.vram_total_mib, Some(12287));
        assert_eq!(info.vram_free_mib, Some(11255));
        assert_eq!(info.engine, "llama.cpp");
    }

    #[test]
    fn zero_offload_with_a_device_is_cpu() {
        let mut probe = LlamaCppProbe::default();
        feed_all(
            &mut probe,
            "llama_prepare_model_devices: using device CUDA0 (RTX 3060) (0000:01:00.0) - 900 MiB free\n\
             load_tensors: offloaded 0/33 layers to GPU",
        );
        assert_eq!(probe.snapshot().backend, BACKEND_CPU);
    }

    #[test]
    fn partial_offload_is_partial() {
        let mut probe = LlamaCppProbe::default();
        feed_all(
            &mut probe,
            "llama_prepare_model_devices: using device CUDA0 (RTX 3060) (0000:01:00.0) - 900 MiB free\n\
             load_tensors: offloaded 12/33 layers to GPU",
        );
        let info = probe.snapshot();
        assert_eq!(info.backend, BACKEND_PARTIAL);
        assert_eq!((info.layers_offloaded, info.layers_total), (12, 33));
    }

    #[test]
    fn cpu_only_build_without_a_device_line_is_cpu() {
        let mut probe = LlamaCppProbe::default();
        feed_all(
            &mut probe,
            "load_tensors: loading model tensors, this can take a while...",
        );
        assert_eq!(probe.snapshot().backend, BACKEND_CPU);
    }

    #[test]
    fn no_log_lines_is_unknown() {
        assert_eq!(LlamaCppProbe::default().snapshot().backend, BACKEND_UNKNOWN);
    }

    #[test]
    fn device_known_but_no_offload_line_yet_is_unknown() {
        let mut probe = LlamaCppProbe::default();
        feed_all(
            &mut probe,
            "llama_prepare_model_devices: using device CUDA0 (RTX 3060) (0000:01:00.0) - 900 MiB free",
        );
        assert_eq!(probe.snapshot().backend, BACKEND_UNKNOWN);
    }

    #[test]
    fn ansi_coloured_input_parses_identically() {
        let coloured: String = GPU_LOG
            .lines()
            .map(|l| format!("\u{1b}[32m{}\u{1b}[0m", l))
            .collect::<Vec<_>>()
            .join("\n");
        let mut probe = LlamaCppProbe::default();
        feed_all(&mut probe, &coloured);
        let info = probe.snapshot();
        assert_eq!(info.backend, BACKEND_GPU);
        assert_eq!(info.layers_offloaded, 33);
    }

    #[test]
    fn reset_clears_previous_state() {
        let mut probe = LlamaCppProbe::default();
        feed_all(&mut probe, GPU_LOG);
        assert_eq!(probe.snapshot().backend, BACKEND_GPU);
        probe.reset();
        assert_eq!(probe.snapshot().backend, BACKEND_UNKNOWN);
    }

    #[test]
    fn parses_list_devices_output() {
        let devices = parse_device_list(
            "Available devices:\n  CUDA0: NVIDIA GeForce RTX 3060 (12287 MiB, 11255 MiB free)\n",
        );
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "CUDA0");
        assert_eq!(devices[0].name, "NVIDIA GeForce RTX 3060");
        assert_eq!(devices[0].total_mib, 12287);
        assert_eq!(devices[0].free_mib, 11255);
        assert!(parse_device_list("ggml_cuda_init: no devices found\n").is_empty());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml llama_cpp`
Expected: FAIL — `cannot find function parse_device_list`, and the backend assertions fail with `unknown`.

- [ ] **Step 3: Write minimal implementation**

Add above `pub struct LlamaCppProbe`:

```rust
fn ansi_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new("\u{1b}\\[[0-9;]*[A-Za-z]").expect("valid ansi regex"))
}

/// Strip ANSI colour escapes. `--log-file` output contains them even when a
/// redirected pipe does not, so stripping is mandatory, not cosmetic.
pub fn strip_ansi(line: &str) -> String {
    ansi_re().replace_all(line, "").into_owned()
}

/// Matches both the startup log line (`- CUDA0 : NAME (TOTAL MiB, FREE MiB free)`)
/// and the `--list-devices` row (`  CUDA0: NAME (TOTAL MiB, FREE MiB free)`).
fn device_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\s*-?\s*(\w+)\s*:\s*(.+?)\s*\((\d+) MiB,\s*(\d+) MiB free\)")
            .expect("valid device regex")
    })
}

fn using_device_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"using device (\S+) \(([^)]+)\)").expect("valid device regex"))
}

fn offloaded_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"offloaded (\d+)/(\d+) layers to GPU").expect("valid regex"))
}

fn model_buffer_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\w+) model buffer size =\s+([\d.]+) MiB").expect("valid regex")
    })
}

fn kv_buffer_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\w+) KV buffer size =\s+([\d.]+) MiB").expect("valid regex")
    })
}

/// Pick the buffer belonging to the accelerator, never the `CPU_Mapped` one.
fn pick_buffer(buffers: &[(String, f64)], device: Option<&str>) -> Option<f64> {
    if let Some(dev) = device {
        if let Some((_, value)) = buffers.iter().find(|(id, _)| id == dev) {
            return Some(*value);
        }
    }
    buffers
        .iter()
        .find(|(id, _)| !id.starts_with("CPU"))
        .map(|(_, value)| *value)
        .or_else(|| buffers.first().map(|(_, value)| *value))
}

fn derive_backend(parsed: &Parsed) -> &'static str {
    if !parsed.saw_load {
        return BACKEND_UNKNOWN;
    }
    if parsed.device.is_none() {
        // A CPU-only build never prints a device line.
        return BACKEND_CPU;
    }
    if parsed.layers_total == 0 {
        // Device known, load not finished: do not guess.
        return BACKEND_UNKNOWN;
    }
    if parsed.layers_offloaded == 0 {
        return BACKEND_CPU;
    }
    if parsed.layers_offloaded >= parsed.layers_total {
        return BACKEND_GPU;
    }
    BACKEND_PARTIAL
}

/// Parse `llama-server --list-devices` output (stdout + stderr concatenated).
pub fn parse_device_list(text: &str) -> Vec<DeviceInfo> {
    text.lines()
        .filter_map(|raw| {
            let line = strip_ansi(raw);
            device_line_re().captures(&line).map(|caps| DeviceInfo {
                id: caps[1].to_string(),
                name: caps[2].trim().to_string(),
                total_mib: caps[3].parse().unwrap_or(0),
                free_mib: caps[4].parse().unwrap_or(0),
            })
        })
        .collect()
}
```

Also extend the re-export in `src-tauri/src/ai/probe/mod.rs` so `ai::engine_log`
(Task 4) can strip escapes without reaching into the llama.cpp module:

```rust
pub use llama_cpp::{strip_ansi, LlamaCppProbe};
```

Replace the four stub methods with the real bodies:

```rust
    fn list_devices(&self, exe: &Path) -> Result<Vec<DeviceInfo>, ProbeError> {
        let output = Command::new(exe)
            .arg("--list-devices")
            .output()
            .map_err(|e| ProbeError(format!("could not run {}: {}", exe.display(), e)))?;
        if !output.status.success() {
            return Err(ProbeError(format!(
                "{} --list-devices exited with {}",
                exe.display(),
                output.status
            )));
        }
        // llama.cpp writes the device table to stdout, diagnostics to stderr.
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(parse_device_list(&text))
    }

    fn feed(&mut self, line: &str) {
        let line = strip_ansi(line);
        if line.contains("load_tensors:")
            || line.contains("llama_prepare_model_devices")
            || line.contains("offloaded ")
        {
            self.parsed.saw_load = true;
        }
        if let Some(caps) = using_device_re().captures(&line) {
            self.parsed.device = Some(caps[1].to_string());
            self.parsed.device_name = Some(caps[2].trim().to_string());
        }
        if let Some(caps) = device_line_re().captures(&line) {
            self.parsed.vram_total = caps[3].parse().ok();
            self.parsed.vram_free = caps[4].parse().ok();
        }
        if let Some(caps) = offloaded_re().captures(&line) {
            self.parsed.layers_offloaded = caps[1].parse().unwrap_or(0);
            self.parsed.layers_total = caps[2].parse().unwrap_or(0);
        }
        if let Some(caps) = model_buffer_re().captures(&line) {
            if let Ok(value) = caps[2].parse::<f64>() {
                self.parsed.model_buffers.push((caps[1].to_string(), value));
            }
        }
        if let Some(caps) = kv_buffer_re().captures(&line) {
            if let Ok(value) = caps[2].parse::<f64>() {
                self.parsed.kv_buffers.push((caps[1].to_string(), value));
            }
        }
    }

    fn snapshot(&self) -> EngineRuntimeInfo {
        let parsed = &self.parsed;
        EngineRuntimeInfo {
            engine: ENGINE_LABEL.to_string(),
            backend: derive_backend(parsed).to_string(),
            device: parsed.device.clone(),
            device_name: parsed.device_name.clone(),
            layers_offloaded: parsed.layers_offloaded,
            layers_total: parsed.layers_total,
            model_mib: pick_buffer(&parsed.model_buffers, parsed.device.as_deref()),
            kv_mib: pick_buffer(&parsed.kv_buffers, parsed.device.as_deref()),
            vram_total_mib: parsed.vram_total,
            vram_free_mib: parsed.vram_free,
        }
    }
```

`model_buffer_re` and `kv_buffer_re` cover both buffer lines; `pick_buffer` prefers
the accelerator's entry over `CPU_Mapped`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml llama_cpp`
Expected: `test result: ok. 12 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ai/probe/llama_cpp.rs
git commit -m "feat(ai): parse llama.cpp offload logs and device listings"
```

---

### Task 4: Engine log drain, ring buffer, rotation

**Files:**

- Create: `src-tauri/src/ai/engine_log.rs`

- [ ] **Step 1: Write the failing test**

Create the file with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_caps_at_capacity() {
        let ring = std::sync::Mutex::new(std::collections::VecDeque::new());
        for i in 0..250 {
            push_capped(&ring, format!("line-{}", i));
        }
        let guard = ring.lock().unwrap();
        assert_eq!(guard.len(), RING_CAPACITY);
        assert_eq!(guard.front().map(String::as_str), Some("line-50"));
        assert_eq!(guard.back().map(String::as_str), Some("line-249"));
    }

    #[test]
    fn rotation_renames_an_oversized_log() {
        let dir = std::env::temp_dir().join(format!("arnold-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llama-chat.log");
        std::fs::write(&path, vec![b'x'; (ROTATE_BYTES + 1) as usize]).unwrap();
        rotate_if_needed(&path);
        assert!(!path.exists(), "original must be moved away");
        let mut backup = path.clone().into_os_string();
        backup.push(".1");
        assert!(std::path::PathBuf::from(backup).exists(), ".1 must exist");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rotation_leaves_a_small_log_alone() {
        let dir = std::env::temp_dir().join(format!("arnold-log-small-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llama-embed.log");
        std::fs::write(&path, b"small").unwrap();
        rotate_if_needed(&path);
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn reader_creates_log_directory_and_persists_lines() {
        use tokio::io::AsyncWriteExt;

        let dir = std::env::temp_dir().join(format!("arnold-log-parent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("logs").join("llama-chat.log");
        let (mut writer, reader) = tokio::io::duplex(1024);
        let probe: Arc<AsyncMutex<Box<dyn EngineProbe>>> =
            Arc::new(AsyncMutex::new(crate::ai::probe::default_probe()));
        let task = spawn_engine_log_reader(reader, path.clone(), new_ring(), probe);

        writer.write_all(b"engine startup line\n").await.unwrap();
        drop(writer);
        task.await.unwrap();

        let log = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(log.contains("engine startup line"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml engine_log`
Expected: FAIL — `cannot find function push_capped`, `cannot find value ROTATE_BYTES`.

- [ ] **Step 3: Write minimal implementation**

Add to `src-tauri/src/ai/mod.rs`, alphabetically after `pub mod engine;`:

```rust
pub mod engine_log;
```

Then, above the test module in `src-tauri/src/ai/engine_log.rs`:

```rust
//! Drains the engine's piped stdio.
//!
//! Two problems solved at once: an unread pipe (~64 KB) eventually blocks the
//! child mid-request, and the stderr stream is exactly where llama.cpp reports
//! device / offload / VRAM state.

use crate::ai::probe::strip_ansi;
use crate::ai::probe::EngineProbe;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::sync::Mutex as AsyncMutex;

/// Lines kept in memory for failure diagnostics.
pub const RING_CAPACITY: usize = 200;
/// Single-generation rotation threshold.
pub const ROTATE_BYTES: u64 = 2 * 1024 * 1024;

pub type EngineLogRing = Arc<Mutex<VecDeque<String>>>;

pub fn new_ring() -> EngineLogRing {
    Arc::new(Mutex::new(VecDeque::with_capacity(RING_CAPACITY)))
}

/// Push a line, dropping the oldest when full.
pub fn push_capped(ring: &Mutex<VecDeque<String>>, line: String) {
    if let Ok(mut guard) = ring.lock() {
        if guard.len() >= RING_CAPACITY {
            guard.pop_front();
        }
        guard.push_back(line);
    }
}

/// Move an oversized log aside to `<name>.1` (one generation). Never fatal.
pub fn rotate_if_needed(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > ROTATE_BYTES {
            let mut backup = path.as_os_str().to_owned();
            backup.push(".1");
            if let Err(e) = std::fs::rename(path, PathBuf::from(backup)) {
                tracing::warn!(target: "ai::engine_log", error = %e, "engine log rotation failed");
            }
        }
    }
}

/// Drain a stream that carries no diagnostics (llama-server keeps stdout
/// almost silent, but the pipe must still be read).
pub fn spawn_output_drain<R>(stream: R) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(_)) = lines.next_line().await {}
    })
}

/// Read stderr line by line: append to `log_path`, keep the tail in `ring`,
/// and feed the probe that backs the GPU status row.
pub fn spawn_engine_log_reader<R>(
    stream: R,
    log_path: PathBuf,
    ring: EngineLogRing,
    probe: Arc<AsyncMutex<Box<dyn EngineProbe>>>,
) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        rotate_if_needed(&log_path);
        if let Some(parent) = log_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                tracing::warn!(target: "ai::engine_log", error = %e, "engine log directory could not be created");
            }
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
            .ok();
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(raw)) = lines.next_line().await {
            let line = strip_ansi(&raw);
            if line.trim().is_empty() {
                continue;
            }
            let mut write_failed = false;
            if let Some(handle) = file.as_mut() {
                write_failed = handle
                    .write_all(format!("{}\n", line).as_bytes())
                    .await
                    .is_err();
            }
            if write_failed {
                file = None;
            }
            push_capped(&ring, line.clone());
            probe.lock().await.feed(&line);
        }
    })
}

/// Last `n` lines, oldest first.
pub fn tail(ring: &Mutex<VecDeque<String>>, n: usize) -> Vec<String> {
    match ring.lock() {
        Ok(guard) => guard.iter().rev().take(n).rev().cloned().collect(),
        Err(_) => Vec::new(),
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml engine_log`
Expected: `test result: ok. 4 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ai/engine_log.rs src-tauri/src/ai/mod.rs
git commit -m "feat(ai): drain engine stdio with rotation and a capped log ring"
```

---

### Task 5: Wire the probe and log drain into `LlamaEngine`

**Files:**

- Modify: `src-tauri/src/ai/engine.rs`

- [ ] **Step 1: Write the failing test**

Replace the existing `test_config()` and `chat_args_include_jinja_and_configured_values` test with:

```rust
    fn test_config() -> EngineConfig {
        EngineConfig {
            exe_path: PathBuf::from("llama-server.exe"),
            chat_model: PathBuf::from("chat.gguf"),
            embed_model: PathBuf::from("embed.gguf"),
            chat_port: 18080,
            embed_port: 18081,
            gpu_layers: "auto".into(),
            ctx_size: 16384,
            api_key: "k".into(),
            log_dir: PathBuf::from("logs"),
        }
    }

    fn probe() -> LlamaCppProbe {
        LlamaCppProbe::default()
    }

    #[test]
    fn chat_args_default_to_auto_gpu_layers() {
        let cfg = test_config();
        let args = chat_args(&cfg, &probe());
        assert!(args.iter().any(|a| a.to_string_lossy() == "--jinja"));
        assert_eq!(arg_value(&args, "-m"), "chat.gguf");
        assert_eq!(arg_value(&args, "--host"), "127.0.0.1");
        assert_eq!(arg_value(&args, "--port"), "18080");
        assert_eq!(arg_value(&args, "-ngl"), "auto");
        assert_eq!(arg_value(&args, "-lv"), "4");
        assert_eq!(arg_value(&args, "-c"), "16384");
        assert_eq!(arg_value(&args, "--api-key"), "k");
        assert!(args.iter().any(|a| a.to_string_lossy() == "--no-warmup"));
    }

    #[test]
    fn chat_args_forward_an_explicit_layer_count() {
        let mut cfg = test_config();
        cfg.gpu_layers = "0".into();
        assert_eq!(arg_value(&chat_args(&cfg, &probe()), "-ngl"), "0");
        cfg.gpu_layers = "nonsense".into();
        assert_eq!(arg_value(&chat_args(&cfg, &probe()), "-ngl"), "auto");
    }

    #[test]
    fn embed_args_request_embeddings_and_gpu_layers() {
        let args = embed_args(&test_config(), &probe());
        assert!(args.iter().any(|a| a.to_string_lossy() == "--embeddings"));
        assert_eq!(arg_value(&args, "--pooling"), "cls");
        assert_eq!(arg_value(&args, "-ngl"), "auto");
        assert_eq!(arg_value(&args, "-lv"), "4");
    }

    #[tokio::test]
    async fn fresh_engine_reports_unknown_runtime_state() {
        let engine = LlamaEngine::new(test_config());
        assert_eq!(engine.chat_info().await.backend, "unknown");
        assert_eq!(engine.embed_info().await.backend, "unknown");
        assert!(engine.last_log_lines("chat", 5).await.is_empty());
    }

    #[tokio::test]
    async fn stopped_engine_does_not_report_stale_gpu_state() {
        let engine = LlamaEngine::new(test_config());
        {
            let mut probe = engine.chat_probe.lock().await;
            probe.feed(
                "llama_prepare_model_devices: using device CUDA0 (RTX 3060) (0000:01:00.0) - 900 MiB free",
            );
            probe.feed("load_tensors: offloaded 33/33 layers to GPU");
        }
        // There is no child in this unit test: chat_info must clear the probe
        // instead of returning the previous process's state.
        assert_eq!(engine.chat_info().await.backend, "unknown");
    }
```

Keep `embed_start_error_display_messages` and `engine_exposes_configured_ports` unchanged.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml ai::engine`
Expected: FAIL — `no field gpu_layers`, `no field log_dir`, `chat_args` arity mismatch, `no method chat_info`.

- [ ] **Step 3: Write minimal implementation**

Imports at the top of `src-tauri/src/ai/engine.rs`:

```rust
use crate::ai::engine_log::{self, EngineLogRing};
use crate::ai::probe::{default_probe, EngineProbe, EngineRuntimeInfo};
use base64::Engine as _;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
```

`std::process::Command` becomes `tokio::process::Command`: the child's stdout/stderr
must be `tokio::process::ChildStdout`/`ChildStderr` so `engine_log` can read them
through `AsyncRead`. `std::process::ChildStdout` does not implement `AsyncRead`, so
the two cannot be mixed. This amends D-125 (recorded in Task 14); the reap
guarantee from `9c48199` is preserved — `kill()` then `wait()`.

`EngineConfig`:

```rust
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub exe_path: PathBuf,
    pub chat_model: PathBuf,
    pub embed_model: PathBuf,
    pub chat_port: u16,
    pub embed_port: u16,
    /// Raw `llama_ngl` setting. Normalised by the probe, never here.
    pub gpu_layers: String,
    pub ctx_size: u32,
    pub api_key: String,
    /// Directory for `<data>/logs/llama-<service>.log`.
    pub log_dir: PathBuf,
}
```

`LlamaEngine`:

```rust
pub struct LlamaEngine {
    config: EngineConfig,
    chat_child: Arc<Mutex<Option<Child>>>,
    embed_child: Arc<Mutex<Option<Child>>>,
    chat_probe: Arc<Mutex<Box<dyn EngineProbe>>>,
    embed_probe: Arc<Mutex<Box<dyn EngineProbe>>>,
    chat_ring: EngineLogRing,
    embed_ring: EngineLogRing,
    chat_reader: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    embed_reader: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}
```

`chat_args` / new `embed_args`:

```rust
/// Chat-server spawn arguments. Kept pure so the tool-capable flags are
/// regression-tested without starting a model.
fn chat_args(config: &EngineConfig, probe: &dyn EngineProbe) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::with_capacity(16);
    args.push("-m".into());
    args.push(config.chat_model.clone().into_os_string());
    args.push("--host".into());
    args.push("127.0.0.1".into());
    args.push("--port".into());
    args.push(config.chat_port.to_string().into());
    // Engine-specific GPU flags (llama.cpp: -ngl <value> -lv 4).
    args.extend(probe.gpu_args(&config.gpu_layers));
    args.push("-c".into());
    args.push(config.ctx_size.to_string().into());
    args.push("--api-key".into());
    args.push(config.api_key.clone().into());
    args.push("--no-warmup".into());
    // Tool calling needs a Jinja chat template. Build 10952 enables this by
    // default, but pass it explicitly so an older bundled binary or a changed
    // default cannot silently disable tool calls.
    args.push("--jinja".into());
    args
}

/// Embedding-server spawn arguments (pure, same reason as `chat_args`).
fn embed_args(config: &EngineConfig, probe: &dyn EngineProbe) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::with_capacity(12);
    args.push("-m".into());
    args.push(config.embed_model.clone().into_os_string());
    args.push("--host".into());
    args.push("127.0.0.1".into());
    args.push("--port".into());
    args.push(config.embed_port.to_string().into());
    args.push("--embeddings".into());
    args.push("--pooling".into());
    args.push("cls".into());
    args.extend(probe.gpu_args(&config.gpu_layers));
    args.push("--api-key".into());
    args.push(config.api_key.clone().into());
    args
}
```

`new`:

```rust
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            chat_child: Arc::new(Mutex::new(None)),
            embed_child: Arc::new(Mutex::new(None)),
            chat_probe: Arc::new(Mutex::new(default_probe())),
            embed_probe: Arc::new(Mutex::new(default_probe())),
            chat_ring: engine_log::new_ring(),
            embed_ring: engine_log::new_ring(),
            chat_reader: Arc::new(Mutex::new(None)),
            embed_reader: Arc::new(Mutex::new(None)),
        }
    }
```

`start_chat`:

```rust
    pub async fn start_chat(&self) -> Result<(), String> {
        let mut child = self.chat_child.lock().await;
        if child.is_some() {
            return Ok(());
        }
        if Self::port_in_use(self.config.chat_port) {
            return Err(format!(
                "Chat port {} is already in use by another process. Restart the engine (it reaps stray llama-server processes), use \"Clear port\" in System, or change llama_chat_port in Settings.",
                self.config.chat_port
            ));
        }

        let args = {
            let probe = self.chat_probe.lock().await;
            chat_args(&self.config, probe.as_ref())
        };
        let mut proc = Command::new(&self.config.exe_path)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn chat server: {}", e))?;

        let stdout = proc.stdout.take();
        let stderr = proc.stderr.take();
        *child = Some(proc);
        drop(child);

        self.start_reader("chat", stdout, stderr).await;
        Ok(())
    }

    /// Fresh probe state per start, then drain both pipes.
    async fn start_reader(
        &self,
        service: &'static str,
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
    ) {
        let (probe, ring, slot, file_name) = if service == "chat" {
            (
                self.chat_probe.clone(),
                self.chat_ring.clone(),
                self.chat_reader.clone(),
                "llama-chat.log",
            )
        } else {
            (
                self.embed_probe.clone(),
                self.embed_ring.clone(),
                self.embed_reader.clone(),
                "llama-embed.log",
            )
        };
        probe.lock().await.reset();
        if let Some(handle) = slot.lock().await.take() {
            handle.abort();
        }
        if let Some(out) = stdout {
            engine_log::spawn_output_drain(out);
        }
        if let Some(err) = stderr {
            let handle = engine_log::spawn_engine_log_reader(
                err,
                self.config.log_dir.join(file_name),
                ring,
                probe,
            );
            *slot.lock().await = Some(handle);
        }
    }
```

`start_embed`: replace the inline `.arg(...)` chain with `embed_args`, and add the reader:

```rust
    pub async fn start_embed(&self) -> Result<(), EmbedStartError> {
        let mut child = self.embed_child.lock().await;
        if child.is_some() {
            return Ok(());
        }
        if Self::port_in_use(self.config.embed_port) {
            return Err(EmbedStartError::PortInUse(self.config.embed_port));
        }

        let args = {
            let probe = self.embed_probe.lock().await;
            embed_args(&self.config, probe.as_ref())
        };
        let mut proc = Command::new(&self.config.exe_path)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| EmbedStartError::Spawn(e.to_string()))?;

        let stdout = proc.stdout.take();
        let stderr = proc.stderr.take();
        *child = Some(proc);
        drop(child);

        self.start_reader("embed", stdout, stderr).await;
        Ok(())
    }
```

`stop_chat` / `stop_embed` (abort the reader after the process is reaped; `kill` and
`wait` are both async on a `tokio::process::Child`):

```rust
    pub async fn stop_chat(&self) {
        let proc = { self.chat_child.lock().await.take() };
        if let Some(mut proc) = proc {
            let _ = proc.kill().await;
            // Reap the process so the OS releases the port/PID before we spawn
            // a replacement; otherwise restart_engine can leave a stray child.
            let _ = proc.wait().await;
        }
        if let Some(handle) = self.chat_reader.lock().await.take() {
            handle.abort();
        }
        self.chat_probe.lock().await.reset();
    }

    pub async fn stop_embed(&self) {
        let proc = { self.embed_child.lock().await.take() };
        if let Some(mut proc) = proc {
            let _ = proc.kill().await;
            let _ = proc.wait().await;
        }
        if let Some(handle) = self.embed_reader.lock().await.take() {
            handle.abort();
        }
        self.embed_probe.lock().await.reset();
    }
```

`alive_pid` needs one adjustment: `tokio::process::Child::id()` returns
`Option<u32>` (std's returns `u32`). Replace the body with:

```rust
    async fn alive_pid(&self, slot: &Arc<Mutex<Option<Child>>>) -> Option<u32> {
        let mut child = slot.lock().await;
        match child.as_mut() {
            Some(proc) => match proc.try_wait() {
                Ok(Some(_)) => None,
                Ok(None) => proc.id(),
                Err(_) => proc.id(),
            },
            None => None,
        }
    }
```

`chat_status` / `embed_status` need no change — `try_wait()` keeps the same
`io::Result<Option<ExitStatus>>` shape.

New accessors, after `embed_port()`:

```rust
    /// Engine-neutral runtime state for the chat server.
    pub async fn chat_info(&self) -> EngineRuntimeInfo {
        let mut probe = self.chat_probe.lock().await;
        if !matches!(self.chat_status().await, EngineStatus::Running) {
            probe.reset();
        }
        probe.snapshot()
    }

    /// Engine-neutral runtime state for the embed server.
    pub async fn embed_info(&self) -> EngineRuntimeInfo {
        let mut probe = self.embed_probe.lock().await;
        if !matches!(self.embed_status().await, EngineStatus::Running) {
            probe.reset();
        }
        probe.snapshot()
    }

    /// Last `n` drained log lines for a service (`"chat"` / `"embed"`).
    pub async fn last_log_lines(&self, service: &str, n: usize) -> Vec<String> {
        let ring = if service == "chat" {
            &self.chat_ring
        } else {
            &self.embed_ring
        };
        engine_log::tail(ring, n)
    }
```

Add `use crate::ai::probe::LlamaCppProbe;` inside the test module.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml ai::engine`
Expected: `test result: ok. 7 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/ai/engine.rs
git commit -m "feat(ai): drive the engine from a probe and drain its stdio"
```

---

### Task 6: Event, commands, and settings plumbing

**Files:**

- Modify: `src-tauri/src/events.rs`
- Create: `src-tauri/src/runtime_env.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/provision.rs`
- Modify: `src-tauri/src/db/settings.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod wire_tests` in `src-tauri/src/lib.rs`:

```rust
    #[test]
    fn engine_runtime_info_serializes_snake_case_wire_keys() {
        let snapshot = EngineRuntimeSnapshot {
            chat: crate::ai::probe::EngineRuntimeInfo::default(),
            embed: crate::ai::probe::EngineRuntimeInfo::default(),
        };
        let v = serde_json::to_value(&snapshot).expect("must serialize");
        for key in ["chat", "embed"] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        for key in [
            "engine",
            "backend",
            "device",
            "device_name",
            "layers_offloaded",
            "layers_total",
            "model_mib",
            "kv_mib",
            "vram_total_mib",
            "vram_free_mib",
        ] {
            assert!(v["chat"].get(key).is_some(), "missing nested key: {key}");
        }
        assert!(v["chat"].get("layersOffloaded").is_none(), "camelCase leak");
    }

    #[test]
    fn requirements_report_serializes_snake_case_wire_keys() {
        let report = RequirementsReport {
            engine: "llama.cpp".into(),
            devices: Vec::new(),
            msvc_runtime: true,
            chat_model: "m.gguf".into(),
            embed_model: String::new(),
        };
        let v = serde_json::to_value(&report).expect("must serialize");
        for key in ["engine", "devices", "msvc_runtime", "chat_model", "embed_model"] {
            assert!(v.get(key).is_some(), "missing wire key: {key}");
        }
        assert!(v.get("msvcRuntime").is_none(), "camelCase leak");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml wire_tests`
Expected: FAIL — `cannot find type EngineRuntimeSnapshot`, `cannot find type RequirementsReport`.

- [ ] **Step 3: Write minimal implementation**

`src-tauri/src/events.rs` — add after `LlamaEngineFailed`:

```rust
    /// Engine-neutral GPU/offload state after an engine lifecycle change.
    #[allow(dead_code)]
    EngineRuntimeStatus {
        chat: crate::ai::probe::EngineRuntimeInfo,
        embed: crate::ai::probe::EngineRuntimeInfo,
    },
```

`src-tauri/src/runtime_env.rs` (new):

```rust
//! Host-environment facts the app reports but does not bundle (D-162).

/// True when the Microsoft Visual C++ 2015-2022 x64 runtime is loadable.
///
/// The bundled `llama-server.exe` imports `vcruntime140.dll`,
/// `vcruntime140_1.dll` and `msvcp140.dll`. They are NOT shipped in the
/// installer, so on a clean machine the engine fails to start with no useful
/// message. Loading the DLLs into this process is the only reliable probe.
#[cfg(windows)]
pub fn msvc_runtime_present() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut core::ffi::c_void;
        fn FreeLibrary(module: *mut core::ffi::c_void) -> i32;
    }

    fn loads(dll: &str) -> bool {
        let mut wide: Vec<u16> = dll.encode_utf16().collect();
        wide.push(0);
        // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives the
        // call; a null handle is checked before FreeLibrary.
        unsafe {
            let handle = LoadLibraryW(wide.as_ptr());
            if handle.is_null() {
                return false;
            }
            FreeLibrary(handle);
            true
        }
    }

    loads("vcruntime140.dll") && loads("vcruntime140_1.dll") && loads("msvcp140.dll")
}

#[cfg(not(windows))]
pub fn msvc_runtime_present() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_is_deterministic() {
        let first = msvc_runtime_present();
        assert_eq!(first, msvc_runtime_present());
    }
}
```

Register the module in `src-tauri/src/lib.rs` (keep the list alphabetical, after `mod provision;`):

```rust
mod runtime_env;
```

Add one import to the `use` block in `src-tauri/src/lib.rs` so the trait's methods
resolve on the boxed probe (without it `default_probe().engine_label()` fails with
"trait `EngineProbe` is not in scope"):

```rust
use ai::probe::EngineProbe as _;
```

`src-tauri/src/lib.rs` — replace both `let ngl: u32 = ...` blocks (lines ~320 and ~2346) with:

```rust
        let gpu_layers: String = db::settings::get_setting(&conn, "llama_ngl")
            .ok()
            .flatten()
            .unwrap_or_else(|| "auto".into());
```

Update both `EngineConfig` constructions (lines ~378 and ~2385): replace `ngl,` with

```rust
                gpu_layers,
                log_dir: state.paths.data_dir.join("logs"),
```

(For the second site the binding is `gpu_layers` from the tuple — rename it in the
inner `let ngl: u32 = ...` at `lib.rs:2346` and in the destructuring at
`lib.rs:2324` (`let (chat_model, embed_model, chat_port, embed_port, ngl, ctx_size, exe_path, api_key) = ...`),
and in the `Ok::<_, AppError>((...))` at `lib.rs:2366-2375`. Keep the position, so
the tuple order stays aligned.)

Add the two wire structs and commands near `engine_path_info`:

```rust
/// Live GPU/offload state for both services (D-148: snake_case keys).
#[derive(Debug, Serialize)]
struct EngineRuntimeSnapshot {
    chat: ai::probe::EngineRuntimeInfo,
    embed: ai::probe::EngineRuntimeInfo,
}

/// Report what the running engine is actually doing: GPU, partial, or CPU.
#[tauri::command]
async fn engine_runtime_info(
    state: tauri::State<'_, AppState>,
) -> Result<EngineRuntimeSnapshot, AppError> {
    let engine = state.engine()?;
    Ok(EngineRuntimeSnapshot {
        chat: engine.chat_info().await,
        embed: engine.embed_info().await,
    })
}

/// Everything the requirements table shows: engine label, accelerators,
/// MSVC runtime presence, and the two user-supplied model files.
#[derive(Debug, Serialize)]
struct RequirementsReport {
    engine: String,
    devices: Vec<ai::probe::DeviceInfo>,
    msvc_runtime: bool,
    chat_model: String,
    embed_model: String,
}

/// Requirements disclosure for onboarding and System. Works with a locked
/// vault: model names simply come back empty.
#[tauri::command]
async fn engine_requirements(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RequirementsReport, AppError> {
    let (saved_exe, chat_model, embed_model) = match state.db() {
        Ok(pool) => db::pool::with_conn(pool, |conn| {
            Ok::<_, AppError>((
                db::settings::get_setting(conn, "llama_exe_path")?.unwrap_or_default(),
                db::settings::get_setting(conn, "llama_chat_model")?.unwrap_or_default(),
                db::settings::get_setting(conn, "llama_embed_model")?.unwrap_or_default(),
            ))
        })
        .await
        .unwrap_or_default(),
        Err(_) => (String::new(), String::new(), String::new()),
    };

    let probe = ai::probe::default_probe();
    let engine_label = probe.engine_label().to_string();
    let resource = bundled_engine_resource(&app);
    let resolved = resolve_engine_exe(&state.paths.data_dir, resource.as_deref(), &saved_exe);

    let devices = if resolved.is_empty() {
        Vec::new()
    } else {
        let exe = std::path::PathBuf::from(resolved);
        match tokio::task::spawn_blocking(move || probe.list_devices(&exe)).await {
            Ok(Ok(devices)) => devices,
            Ok(Err(e)) => {
                tracing::warn!(target: "engine", error = %e, "device listing failed");
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    };

    Ok(RequirementsReport {
        engine: engine_label,
        devices,
        msvc_runtime: runtime_env::msvc_runtime_present(),
        chat_model,
        embed_model,
    })
}
```

Register both commands in the `invoke_handler` list next to `engine_path_info`:

```rust
            engine_runtime_info,
            engine_requirements,
```

The user can also restart/stop one service directly from System. In `lib.rs`,
after the existing `LlamaEngineStarted` emit in `restart_engine`, emit the
combined runtime snapshot:

```rust
    crate::events::emit(
        &app,
        crate::events::AppEvent::EngineRuntimeStatus {
            chat: engine.chat_info().await,
            embed: engine.embed_info().await,
        },
    );
```

In `stop_engine`, after the existing `LlamaEngineStopped` emit, add:

```rust
    crate::events::emit(
        &app,
        crate::events::AppEvent::EngineRuntimeStatus {
            chat: engine.chat_info().await,
            embed: engine.embed_info().await,
        },
    );
```

Task 5 resets stopped probes, so this event reports `unknown`, never the last GPU state.

`src-tauri/src/provision.rs` — keep the existing `LlamaEngineStarted` events. After
the embed-role check succeeds (and before the `!chat_ok` / `!embed_ok` failure
returns), publish one combined snapshot:

```rust
    emit(
        app,
        AppEvent::EngineRuntimeStatus {
            chat: engine.chat_info().await,
            embed: engine.embed_info().await,
        },
    );
```

And in both start-failure branches, log the drained tail before returning:

```rust
    if let Err(e) = engine.start_chat().await {
        let reason = format!("Failed to start chat server: {}", e);
        for line in engine.last_log_lines("chat", 20).await {
            tracing::error!(target: "provision", "chat: {}", line);
        }
        emit(
            app,
            AppEvent::LlamaEngineFailed {
                service: "chat".to_string(),
                reason: reason.clone(),
            },
        );
        return ProvisionOutcome::Unavailable { reason };
    }
```

```rust
    if let Err(e) = engine.start_embed().await {
        for line in engine.last_log_lines("embed", 20).await {
            tracing::error!(target: "provision", "embed: {}", line);
        }
        let reason = embed_start_failure_reason(state.llama().ok().as_ref(), e).await;
        emit(
            app,
            AppEvent::LlamaEngineFailed {
                service: "embed".to_string(),
                reason: reason.clone(),
            },
        );
        return ProvisionOutcome::Unavailable { reason };
    }
```

`src-tauri/src/db/settings.rs` line 16:

```rust
        ("llama_ngl", "auto"),
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml wire_tests`
Expected: `test result: ok. 6 passed` (the 4 existing wire-contract tests plus the 2 added here)

Then: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: `error: could not compile` only if a call site was missed — fix any `EngineConfig`/`ngl` mismatch before moving on.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/events.rs src-tauri/src/runtime_env.rs src-tauri/src/lib.rs src-tauri/src/provision.rs src-tauri/src/db/settings.rs
git commit -m "feat(engine): expose runtime status and requirements over IPC"
```

---

### Task 7: Migration 018 — stop hard-pinning GPU layers

**Files:**

- Create: `src-tauri/migrations/018_gpu_layers_auto.sql`
- Modify: `src-tauri/src/db/migrations.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/db/migrations.rs`:

```rust
    #[test]
    fn gpu_layers_migration_rewrites_only_the_legacy_pin() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO app_settings (key, value, updated_at) VALUES ('llama_ngl', '99', 'x')",
            [],
        )
        .unwrap();

        let run = |conn: &mut Connection| {
            let tx = conn.transaction().unwrap();
            let sql = strip_outer_transaction(include_str!(
                "../../migrations/018_gpu_layers_auto.sql"
            ));
            ensure_no_explicit_transactions(&sql, "018").unwrap();
            tx.execute_batch(&sql).unwrap();
            tx.commit().unwrap();
        };

        run(&mut conn);
        let value: String = conn
            .query_row("SELECT value FROM app_settings WHERE key='llama_ngl'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(value, "auto");

        // A deliberate pin must survive re-running the migration.
        conn.execute(
            "UPDATE app_settings SET value='40' WHERE key='llama_ngl'",
            [],
        )
        .unwrap();
        run(&mut conn);
        let kept: String = conn
            .query_row("SELECT value FROM app_settings WHERE key='llama_ngl'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kept, "40");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml gpu_layers_migration`
Expected: FAIL — `couldn't read ../../migrations/018_gpu_layers_auto.sql`.

- [ ] **Step 3: Write minimal implementation**

Create `src-tauri/migrations/018_gpu_layers_auto.sql` (bare statements only — the runner supplies the transaction, D-97):

```sql
-- GPU layers default: 'auto' lets llama.cpp size the offload to available VRAM
-- (--fit defaults to on). The legacy seed hard-pinned 99, which overrides both
-- and makes a model larger than free VRAM fail to load instead of partially
-- offloading. Only the seeded value is rewritten; a deliberate pin survives.
UPDATE app_settings
   SET value = 'auto',
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
 WHERE key = 'llama_ngl'
   AND value = '99';
```

Then register it in the hardcoded `MIGRATIONS` array in
`src-tauri/src/db/migrations.rs` (the runner is a static list, not a directory scan —
appending the file alone would never run it). After the version-17 entry:

```rust
    Migration {
        version: 18,
        name: "gpu_layers_auto",
        sql: include_str!("../../migrations/018_gpu_layers_auto.sql"),
    },
```

`REBUILD_MIGRATIONS` stays empty — this migration is data-only, no table rebuild.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml gpu_layers_migration`
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/migrations/018_gpu_layers_auto.sql src-tauri/src/db/migrations.rs
git commit -m "feat(db): migrate the seeded gpu layer pin to auto"
```

---

### Task 8: Preflight GPU and MSVC checks

**Files:**

- Modify: `src-tauri/src/preflight.rs`

- [ ] **Step 1: Write the failing test**

Add a `#[cfg(test)] mod tests` at the end of `src-tauri/src/preflight.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msvc_check_is_a_warn_or_pass_with_a_message() {
        let check = check_msvc_runtime();
        assert_eq!(check.id, "msvc_runtime");
        assert!(matches!(check.status, CheckStatus::Pass | CheckStatus::Warn));
        assert!(!check.message.is_empty());
        if check.status == CheckStatus::Warn {
            assert!(check.actionable, "a missing runtime needs an action");
        }
    }

    #[test]
    fn gpu_check_reports_the_probe_outcome() {
        let devices = vec![crate::ai::probe::DeviceInfo {
            id: "CUDA0".into(),
            name: "NVIDIA GeForce RTX 3060".into(),
            total_mib: 12287,
            free_mib: 11255,
        }];
        let check = gpu_check_from(Ok(devices));
        assert_eq!(check.id, "gpu_offload");
        assert_eq!(check.status, CheckStatus::Pass);
        assert!(check.message.contains("RTX 3060"));

        let none = gpu_check_from(Ok(Vec::new()));
        assert_eq!(none.status, CheckStatus::Warn);
        assert!(none.actionable);

        let broken = gpu_check_from(Err(crate::ai::probe::ProbeError("os error 126".into())));
        assert_eq!(broken.status, CheckStatus::Fail);
        assert!(broken.message.contains("126"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml preflight::tests`
Expected: FAIL — `cannot find function check_msvc_runtime`, `cannot find function gpu_check_from`.

- [ ] **Step 3: Write minimal implementation**

Add above `run_preflight`:

```rust
/// Check 7: the engine binary can enumerate a GPU without loading a model.
fn gpu_check_from(
    result: Result<Vec<crate::ai::probe::DeviceInfo>, crate::ai::probe::ProbeError>,
) -> PreflightCheck {
    match result {
        Ok(devices) if !devices.is_empty() => {
            let d = &devices[0];
            PreflightCheck::pass(
                "gpu_offload",
                "GPU offload",
                format!(
                    "{} ({}, {} MiB free) - available for GPU offload",
                    d.name, d.id, d.free_mib
                ),
            )
        }
        Ok(_) => PreflightCheck::warn(
            "gpu_offload",
            "GPU offload",
            "No GPU device found - Arnold will run on CPU (slower, still fully offline)."
                .to_string(),
        )
        .with_action(true),
        Err(e) => PreflightCheck::fail(
            "gpu_offload",
            "GPU offload",
            format!("The engine could not be executed: {}", e),
        )
        .with_action(true),
    }
}

/// Check 8: the Microsoft Visual C++ runtime the bundled engine imports.
fn check_msvc_runtime() -> PreflightCheck {
    if crate::runtime_env::msvc_runtime_present() {
        PreflightCheck::pass(
            "msvc_runtime",
            "Visual C++ runtime",
            "Microsoft Visual C++ 2015-2022 x64 runtime detected",
        )
    } else {
        PreflightCheck::warn(
            "msvc_runtime",
            "Visual C++ runtime",
            "Missing Microsoft Visual C++ 2015-2022 x64 runtime - the engine cannot start. Install it from https://aka.ms/vs/17/release/vc_redist.x64.exe"
                .to_string(),
        )
        .with_action(true)
    }
}
```

Inside `run_preflight`, after check 3 (the `llama_connection` block) and before check 4:

```rust
    // 3b. GPU availability + the VC++ runtime the bundled engine imports.
    {
        let resource = crate::bundled_engine_resource(app_handle);
        // No `?` here: a locked vault must still produce a full report (check 2
        // already reports it), exactly like `engine_requirements` does.
        let saved = match state.db() {
            Ok(pool) => crate::db::pool::with_conn(pool, |conn| {
                crate::db::settings::get_setting(conn, "llama_exe_path")
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default(),
            Err(_) => String::new(),
        };
        let resolved = crate::resolve_engine_exe(&paths.data_dir, resource.as_deref(), &saved);
        let result = if resolved.is_empty() {
            Err(crate::ai::probe::ProbeError(
                "no llama-server.exe found".into(),
            ))
        } else {
            let probe = crate::ai::probe::default_probe();
            let exe = std::path::PathBuf::from(resolved);
            tokio::task::spawn_blocking(move || probe.list_devices(&exe))
                .await
                .unwrap_or_else(|e| Err(crate::ai::probe::ProbeError(e.to_string())))
        };
        checks.push(gpu_check_from(result));
        checks.push(check_msvc_runtime());
    }
```

`run_preflight` needs an `AppHandle` for `bundled_engine_resource`. Change its signature and the command:

```rust
pub async fn run_preflight(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<PreflightReport, AppError> {
```

```rust
#[tauri::command]
pub async fn preflight_run(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<PreflightReport, AppError> {
    run_preflight(&app, &state).await
}
```

Make `bundled_engine_resource` reachable from `preflight.rs`: it is currently
`fn bundled_engine_resource(...)` at `lib.rs:2136` — change it to
`pub(crate) fn bundled_engine_resource(...)`. `resolve_engine_exe` is already
`pub(crate)` (`lib.rs:2214`).

Update the single existing caller of `run_preflight` (the `preflight_run` command at
`preflight.rs:305` — that is the only call site in the crate) to pass `&app`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml preflight::tests`
Expected: `test result: ok. 2 passed`

Then: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: no errors. Fix any missed `run_preflight` call site.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/preflight.rs src-tauri/src/lib.rs
git commit -m "feat(preflight): check gpu offload and the vc++ runtime"
```

---

### Task 9: Shared requirements module + harness

**Files:**

- Create: `src/js/requirements.js`
- Create: `tools/js-checks/requirements.test.mjs`

- [ ] **Step 1: Write the failing test**

Create `tools/js-checks/requirements.test.mjs`:

```js
import assert from "node:assert/strict";
import {
  requirementRows,
  requirementsTableHtml,
} from "../../src/js/requirements.js";

const full = requirementRows({
  engineLabel: "llama.cpp",
  devices: [{ id: "CUDA0", name: "NVIDIA GeForce RTX 3060" }],
  msvcRuntime: true,
  chatModel: "Qwythos-9B.gguf",
  embedModel: "nomic-embed.gguf",
});

assert.equal(full.length, 6, "six requirement rows");
assert.deepEqual(
  full.map((r) => r.key),
  ["engine", "webview2", "driver", "msvc", "chat_model", "embed_model"],
  "stable row order",
);
assert.equal(full[0].label, "llama.cpp + accelerator libraries");
assert.equal(full[0].bundled, true);
assert.equal(full[0].state, "included");
assert.equal(full[2].state, "detected");
assert.ok(full[2].detail.includes("RTX 3060"), "device name shown");
assert.equal(full[3].state, "detected");

const bare = requirementRows({
  engineLabel: "llama.cpp",
  devices: [],
  msvcRuntime: false,
  chatModel: "",
  embedModel: "",
});
assert.equal(bare[2].state, "missing");
assert.ok(bare[2].detail.includes("CPU"), "no-GPU detail mentions CPU");
assert.equal(bare[3].state, "missing");
assert.ok(bare[3].detail.includes("cannot start"));
assert.equal(bare[4].detail, "Not imported");
assert.ok(bare[5].detail.includes("semantic search"));

const html = requirementsTableHtml([
  {
    key: "engine",
    label: 'evil <script>alert("x")</script>',
    bundled: true,
    state: "included",
    detail: "Included",
  },
  {
    key: "driver",
    label: "GPU driver",
    bundled: false,
    state: "missing",
    detail: "Not detected",
  },
]);
assert.ok(html.includes('class="requirements-table"'), "table class");
assert.ok(html.includes("req-missing"), "missing state class");
assert.ok(html.includes("req-included"), "included state class");
assert.ok(!html.includes("<script"), "no tag smuggling");
assert.ok(html.includes("&lt;script&gt;"), "label escaped");
assert.ok(html.includes(">Included<"), "bundled column rendered");
assert.ok(html.includes(">Not bundled<"), "external column rendered");
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node tools/js-checks/requirements.test.mjs`
Expected: FAIL — `Cannot find module '../../src/js/requirements.js'`.

- [ ] **Step 3: Write minimal implementation**

Create `src/js/requirements.js`:

```js
// Single source of truth for "what does this app need?".
// Rendered identically by the onboarding wizard (step 0) and System.
// Every value is plain data so this file stays node-testable (no DOM).
import { escapeHtml } from "./sanitize.js";

const INSTRUCTIONS =
  "Install from https://aka.ms/vs/17/release/vc_redist.x64.exe";

/**
 * @param {object} input
 * @param {string} input.engineLabel   from `engine_requirements.engine`
 * @param {Array}  input.devices       from `engine_requirements.devices`
 * @param {boolean} input.msvcRuntime  from `engine_requirements.msvc_runtime`
 * @param {string} input.chatModel     from `engine_requirements.chat_model`
 * @param {string} input.embedModel    from `engine_requirements.embed_model`
 */
export function requirementRows({
  engineLabel = "Engine",
  devices = [],
  msvcRuntime = false,
  chatModel = "",
  embedModel = "",
} = {}) {
  const device = Array.isArray(devices) && devices.length ? devices[0] : null;
  return [
    {
      key: "engine",
      label: `${engineLabel} + accelerator libraries`,
      bundled: true,
      state: "included",
      detail: "Included",
    },
    {
      key: "webview2",
      label: "WebView2 runtime",
      bundled: true,
      state: "included",
      detail: "Included",
    },
    {
      key: "driver",
      label: "GPU driver (NVIDIA)",
      bundled: false,
      state: device ? "detected" : "missing",
      detail: device
        ? `Detected - ${device.name}`
        : "Not detected - running on CPU (slower)",
    },
    {
      key: "msvc",
      label: "Microsoft Visual C++ 2015-2022 x64 runtime",
      bundled: false,
      state: msvcRuntime ? "detected" : "missing",
      detail: msvcRuntime
        ? "Detected"
        : `Missing - engine cannot start. ${INSTRUCTIONS}`,
    },
    {
      key: "chat_model",
      label: "Chat model (GGUF, instruct-tuned)",
      bundled: false,
      state: chatModel ? "detected" : "missing",
      detail: chatModel ? `Imported: ${chatModel}` : "Not imported",
    },
    {
      key: "embed_model",
      label: "Embedding model (GGUF)",
      bundled: false,
      state: embedModel ? "detected" : "missing",
      detail: embedModel
        ? `Imported: ${embedModel}`
        : "Not imported (semantic search disabled)",
    },
  ];
}

/** Rows -> escaped table markup. */
export function requirementsTableHtml(rows) {
  const body = (rows || [])
    .map(
      (row) => `
      <tr data-req="${escapeHtml(row.key)}">
        <th scope="row">${escapeHtml(row.label)}</th>
        <td>${row.bundled ? "Included" : "Not bundled"}</td>
        <td><span class="req-state req-${escapeHtml(row.state)}">${escapeHtml(row.detail)}</span></td>
      </tr>`,
    )
    .join("");
  return `<table class="data-table requirements-table"><tbody>${body}</tbody></table>`;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node tools/js-checks/requirements.test.mjs`
Expected: exits 0 with no output.

- [ ] **Step 5: Commit**

```bash
git add src/js/requirements.js tools/js-checks/requirements.test.mjs
git commit -m "feat(ui): add a single requirements source of truth"
```

---

### Task 10: Engine status helpers + harness

**Files:**

- Create: `src/js/engine-status.js`
- Create: `tools/js-checks/engine-status.test.mjs`

- [ ] **Step 1: Write the failing test**

Create `tools/js-checks/engine-status.test.mjs`:

```js
import assert from "node:assert/strict";
import {
  backendLabel,
  inlineStatus,
  gpuWarningOnce,
  resetGpuWarning,
} from "../../src/js/engine-status.js";

assert.equal(
  backendLabel({ backend: "gpu", layers_offloaded: 33, layers_total: 33 }),
  "GPU - 33/33 layers offloaded",
);
assert.equal(
  backendLabel({ backend: "partial", layers_offloaded: 12, layers_total: 33 }),
  "Partial - 12/33 layers on GPU",
);
assert.equal(backendLabel({ backend: "cpu" }), "CPU only");
assert.equal(
  backendLabel({ backend: "unknown" }),
  "Unknown - engine not started",
);
assert.equal(backendLabel(null), "Unknown - engine not started");

const gpuInfo = {
  engine: "llama.cpp",
  backend: "gpu",
  device_name: "NVIDIA GeForce RTX 3060",
  layers_offloaded: 33,
  layers_total: 33,
};
const cpuInfo = {
  engine: "llama.cpp",
  backend: "cpu",
  device_name: "NVIDIA GeForce RTX 3060",
  layers_offloaded: 0,
  layers_total: 33,
};
assert.ok(inlineStatus(gpuInfo).includes("RTX 3060"), "device named inline");
assert.ok(inlineStatus(gpuInfo).includes("33/33"), "layer count inline");

assert.equal(gpuWarningOnce(gpuInfo), null, "no warning on GPU");
resetGpuWarning();
const warn = gpuWarningOnce(cpuInfo);
assert.ok(warn.includes("CPU"), "cpu warning names CPU");
assert.ok(warn.includes("llama.cpp"), "cpu warning names the engine");
assert.ok(warn.includes("GPU acceleration is not active"));
assert.equal(
  gpuWarningOnce({ engine: "llama.cpp", backend: "cpu" }),
  null,
  "same warning is not repeated",
);
assert.equal(gpuWarningOnce(gpuInfo), null, "GPU transition is silent");
assert.ok(
  gpuWarningOnce({ engine: "llama.cpp", backend: "cpu" }),
  "a later transition back to CPU warns again",
);
assert.equal(
  gpuWarningOnce({ engine: "llama.cpp", backend: "unknown" }),
  null,
  "stopped/unknown state is silent",
);
assert.ok(
  gpuWarningOnce({ engine: "llama.cpp", backend: "cpu" }),
  "CPU after a stopped/unknown state warns again",
);
resetGpuWarning();
assert.equal(
  gpuWarningOnce({ engine: "llama.cpp", backend: "unknown" }),
  null,
  "unknown is silent",
);
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node tools/js-checks/engine-status.test.mjs`
Expected: FAIL — `Cannot find module '../../src/js/engine-status.js'`.

- [ ] **Step 3: Write minimal implementation**

Create `src/js/engine-status.js`:

```js
// Pure formatting for `engine_runtime_info` (EngineRuntimeInfo, snake_case).
// Node-testable: no DOM, no imports.

export function backendLabel(info) {
  const backend = info?.backend || "unknown";
  if (backend === "gpu") {
    return `GPU - ${info.layers_offloaded}/${info.layers_total} layers offloaded`;
  }
  if (backend === "partial") {
    return `Partial - ${info.layers_offloaded}/${info.layers_total} layers on GPU`;
  }
  if (backend === "cpu") return "CPU only";
  return "Unknown - engine not started";
}

/** One line for Settings: what the engine is doing right now. */
export function inlineStatus(info) {
  const label = backendLabel(info);
  const device = info?.device_name;
  if ((info?.backend === "gpu" || info?.backend === "partial") && device) {
    return `${label} (${device})`;
  }
  if (info?.backend === "cpu") {
    return `${label} - set GPU layers to auto and restart if this machine has a supported GPU`;
  }
  return label;
}

let lastWarned = null;

/** Warn once per backend transition; never block, never repeat. */
export function gpuWarningOnce(info) {
  const backend = info?.backend;
  if (backend === "gpu" || backend === "unknown") {
    // Recovery or a stopped engine re-arms the warning for a later fallback.
    lastWarned = null;
    return null;
  }
  if (!backend) {
    return null;
  }
  if (lastWarned === backend) return null;
  lastWarned = backend;
  const engine = info?.engine || "The AI engine";
  if (backend === "partial") {
    return `${engine}: ${info.layers_offloaded}/${info.layers_total} layers on the GPU, the rest on CPU.`;
  }
  return `${engine} is running on CPU - GPU acceleration is not active. Replies will be slower.`;
}

export function resetGpuWarning() {
  lastWarned = null;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `node tools/js-checks/engine-status.test.mjs`
Expected: exits 0 with no output.

- [ ] **Step 5: Commit**

```bash
git add src/js/engine-status.js tools/js-checks/engine-status.test.mjs
git commit -m "feat(ui): add pure engine status formatting helpers"
```

---

### Task 11: Settings — `auto` field and inline status

**Files:**

- Modify: `src/index.html:1078-1091`
- Modify: `src/js/tabs/settings.js:365,405-437,588`

- [ ] **Step 1: Update the markup**

Replace the `llama-ngl` field block in `src/index.html` (currently lines 1079-1091) with:

```html
<div class="field">
  <label class="field-label" for="llama-ngl">GPU layers</label>
  <input
    type="text"
    id="llama-ngl"
    class="text-input-control"
    pattern="(auto|all|[0-9]|[1-9][0-9]|[1-3][0-9]{2}|400)"
    title="Enter auto, all, or a whole number from 0 to 400."
    value="auto"
    placeholder="auto"
    autocomplete="off"
    spellcheck="false"
  />
  <p class="field-help" id="llama-ngl-help">
    <strong>auto</strong> = use as much GPU as fits. <strong>0</strong> = CPU
    only. A number pins that many layers; <strong>all</strong> pins every layer.
  </p>
  <p class="field-help" id="llama-ngl-status">-</p>
</div>
```

- [ ] **Step 2: Update `settings.js`**

At the top of `src/js/tabs/settings.js`, add the import (with the other imports):

```js
import { inlineStatus } from "../engine-status.js";
```

In `initLLMEngineSection`, add the status element next to `ngl`:

```js
const nglStatus = document.getElementById("llama-ngl-status");
```

Replace the load default (line ~431):

```js
if (ngl) ngl.value = savedNgl || "auto";
```

Replace the existing `ngl` change listener at `settings.js:588` so the browser
validates the same grammar as the backend before saving. Normalize case and
whitespace first; invalid input remains visible and is not persisted:

```js
ngl.addEventListener("change", () => {
  const value = ngl.value.trim().toLowerCase();
  ngl.value = value;
  if (!ngl.checkValidity()) {
    ngl.reportValidity();
    return;
  }
  persist("llama_ngl", value);
});
```

Add a refresh helper right after `refreshEnginePathHint`:

```js
// Tell the user what the engine is actually doing. Read-only: it never
// changes a setting, and a locked vault or stopped engine is not an error.
async function refreshGpuStatus() {
  if (!nglStatus) return;
  try {
    const info = await invoke("engine_runtime_info", {}, { silent: true });
    nglStatus.textContent = `Current: ${inlineStatus(info.chat)}`;
  } catch (e) {
    nglStatus.textContent = "Current: engine not running";
  }
}
```

Call it at the end of `loadSettings` (after `await refreshModelList();`):

```js
await refreshGpuStatus();
```

And refresh it after a restart. `restartService(service)` in the same file is the
restart path (`btn-restart-chat` / `btn-restart-embed` — there is no
`data-engine-action` handler here); add the refresh right after the existing
`await updateEngineStatus();` in **both** the success and failure branches:

```js
await updateEngineStatus();
await refreshGpuStatus();
```

Finally, extend the existing 30s poll at the bottom of `initLLMEngineSection` so the
status line cannot go stale:

```js
setInterval(() => {
  updateEngineStatus();
  refreshGpuStatus();
}, 30000);
```

- [ ] **Step 3: Manual verification**

Run: `node --check src/js/tabs/settings.js`
Expected: no output (syntax OK).

Run: `Select-String -Path src\index.html -Pattern 'id="llama-ngl"' -Context 0,2`
Expected: `type="text"` and `value="auto"` in the output.

- [ ] **Step 4: Commit**

```bash
git add src/index.html src/js/tabs/settings.js
git commit -m "feat(settings): default gpu layers to auto with inline engine status"
```

---

### Task 12: System tab — engine runtime + requirements card

**Files:**

- Modify: `src/views/system.html`
- Modify: `src/js/tabs/system.js`
- Modify: `src/styles/surfaces/system.css`

- [ ] **Step 1: Add the markup**

In `src/views/system.html`, replace the `<p>` under `<h2>AI Engines</h2>` (line 110) with:

```html
<p>
  The <span class="mono" id="rt-engine-name">-</span> runtime — where chat
  (:8080) and embed (:8081) are actually running.
</p>
<table class="data-table runtime-table">
  <thead>
    <tr>
      <th scope="col">Service</th>
      <th scope="col">Backend</th>
      <th scope="col">Device</th>
      <th scope="col">Layers offloaded</th>
      <th scope="col">VRAM</th>
      <th scope="col">Model / KV buffer</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <th scope="row">Chat</th>
      <td>
        <span class="status-cell">
          <span class="status-dot" id="rt-chat-backend-dot"></span>
          <span class="mono" id="rt-chat-backend">-</span>
        </span>
      </td>
      <td class="mono" id="rt-chat-device">-</td>
      <td class="mono" id="rt-chat-layers">-</td>
      <td class="mono" id="rt-chat-vram">-</td>
      <td class="mono" id="rt-chat-buffers">-</td>
    </tr>
    <tr>
      <th scope="row">Embed</th>
      <td>
        <span class="status-cell">
          <span class="status-dot" id="rt-embed-backend-dot"></span>
          <span class="mono" id="rt-embed-backend">-</span>
        </span>
      </td>
      <td class="mono" id="rt-embed-device">-</td>
      <td class="mono" id="rt-embed-layers">-</td>
      <td class="mono" id="rt-embed-vram">-</td>
      <td class="mono" id="rt-embed-buffers">-</td>
    </tr>
  </tbody>
</table>
```

Immediately after the closing `</section>` of the AI Engines section, insert:

```html
<section class="sys-section">
  <h2>Requirements</h2>
  <p>What Arnold ships with, what Windows must provide, and what you supply.</p>
  <div id="requirements-container">-</div>
</section>
```

- [ ] **Step 2: Add the JS**

At the top of `src/js/tabs/system.js`, add the imports:

```js
import { requirementRows, requirementsTableHtml } from "../requirements.js";
import { backendLabel } from "../engine-status.js";
```

Add these functions before `refreshRuntime`:

```js
// Engine-neutral GPU state. `engine_runtime_info` returns snake_case fields.
async function refreshEngineRuntime() {
  try {
    const info = await invoke("engine_runtime_info", {}, { silent: true });
    const chat = info?.chat || {};
    const embed = info?.embed || {};
    setCell("rt-engine-name", chat.engine || embed.engine || "-");
    for (const [service, runtime] of [
      ["chat", chat],
      ["embed", embed],
    ]) {
      setCell(`rt-${service}-backend`, backendLabel(runtime));
      setCell(
        `rt-${service}-device`,
        runtime.device_name
          ? `${runtime.device_name}${runtime.device ? ` (${runtime.device})` : ""}`
          : "-",
      );
      setCell(
        `rt-${service}-layers`,
        runtime.layers_total
          ? `${runtime.layers_offloaded}/${runtime.layers_total}`
          : "-",
      );
      setCell(
        `rt-${service}-vram`,
        runtime.vram_total_mib
          ? `${runtime.vram_free_mib ?? "-"} MiB free of ${runtime.vram_total_mib} MiB`
          : "-",
      );
      setCell(
        `rt-${service}-buffers`,
        runtime.model_mib
          ? `${runtime.model_mib} MiB model / ${runtime.kv_mib ?? "-"} MiB KV`
          : "-",
      );
      const dot = document.getElementById(`rt-${service}-backend-dot`);
      if (dot) {
        // Reuse the existing status-dot classes from layout.css.
        dot.className = "status-dot";
        if (runtime.backend === "gpu") dot.classList.add("ok");
        else if (runtime.backend === "cpu" || runtime.backend === "partial") {
          dot.classList.add("warn");
        }
      }
    }
  } catch (e) {
    // A locked vault or a stopped engine is not an error: blank the row rather
    // than leaving stale values from the last successful read.
    setCell("rt-engine-name", "-");
    for (const service of ["chat", "embed"]) {
      for (const field of ["backend", "device", "layers", "vram", "buffers"]) {
        setCell(`rt-${service}-${field}`, "-");
      }
      const dot = document.getElementById(`rt-${service}-backend-dot`);
      if (dot) dot.className = "status-dot";
    }
    setCell("rt-chat-backend", "engine not running");
    setCell("rt-embed-backend", "engine not running");
  }
}

async function refreshRequirements() {
  const container = document.getElementById("requirements-container");
  if (!container) return;
  try {
    const report = await invoke("engine_requirements", {}, { silent: true });
    const rows = requirementRows({
      engineLabel: report.engine,
      devices: report.devices,
      msvcRuntime: report.msvc_runtime,
      chatModel: report.chat_model,
      embedModel: report.embed_model,
    });
    container.innerHTML = requirementsTableHtml(rows);
  } catch (e) {
    container.textContent = "Could not read requirements.";
    console.error("[SYSTEM] engine_requirements failed", e);
  }
}
```

Call both from the tab-load path in `initSystem()` (after the existing
`refreshEngines();` at `system.js:536`):

```js
refreshEngines();
refreshEngineRuntime();
refreshRequirements();
```

Add `refreshEngineRuntime();` to the existing engine poll so the row tracks a
restart — inside the `if (systemTabActive())` guard in the `engineTimer` interval
at `system.js:546-551`, next to the existing `refreshEngines();` — and to the
engine-action handler after each `await refreshEngines();` — three sites: the Start
button at `system.js:292`, the `clear-port` branch at `system.js:327`, and the shared
restart/stop path at `system.js:333`:

```js
await refreshEngines();
await refreshEngineRuntime();
```

Also add `refreshEngineRuntime();` to `window.refreshSystemTab` so a manual refresh
picks up the new state.

Do **not** poll `refreshRequirements()` — it runs a child process
(`--list-devices`). Once per tab load is enough.

- [ ] **Step 3: Add the styles**

Append to `src/styles/surfaces/system.css` (tokens only — the design guard
rejects raw colours and any `box-shadow` outside the `--elev-*` scale):

```css
.requirements-table .req-state {
  color: var(--color-text-muted);
}
.requirements-table .req-detected {
  color: var(--color-text);
}
.requirements-table .req-missing {
  color: var(--color-warn);
}
.requirements-table .req-included {
  color: var(--color-text-muted);
}
```

No new dot styles: `#rt-backend-dot` uses the existing `.status-dot.ok` /
`.status-dot.warn` classes already defined in `src/styles/layout.css:222-224`.

- [ ] **Step 4: Verify**

Run: `node --check src/js/tabs/system.js`
Expected: no output.

Run: `node tools/js-checks/design.test.mjs`
Expected: exits 0 (the design guard rejects unknown shadow/token usage; fix any finding).

- [ ] **Step 5: Commit**

```bash
git add src/views/system.html src/js/tabs/system.js src/styles/surfaces/system.css
git commit -m "feat(system): show engine backend and the requirements list"
```

---

### Task 13: Onboarding step 0 + non-GPU toast

**Files:**

- Modify: `src/index.html:1430` (pane 0 opening, before welcome copy)
- Modify: `src/js/onboarding.js`
- Modify: `src/js/app.js:24-37`

- [ ] **Step 1: Add the requirements container to pane 0**

In `src/index.html`, inside `<section class="onboarding-pane active" data-pane="0">`, insert the container immediately after the opening `<section>` and before the first welcome paragraph. This keeps the external requirements disclosure above the existing welcome copy, as specified:

```html
<div id="onboarding-requirements" class="onboarding-requirements">
  <p class="field-help">Loading requirements…</p>
</div>
```

- [ ] **Step 2: Render it from `onboarding.js`**

Add the imports at the top of `src/js/onboarding.js`:

```js
import { requirementRows, requirementsTableHtml } from "./requirements.js";
```

Add the function before `goTo`:

```js
// Same table System shows (single source of truth in requirements.js).
async function refreshRequirements() {
  const container = document.getElementById("onboarding-requirements");
  if (!container) return;
  try {
    const report = await invoke("engine_requirements", {}, { silent: true });
    container.innerHTML = requirementsTableHtml(
      requirementRows({
        engineLabel: report.engine,
        devices: report.devices,
        msvcRuntime: report.msvc_runtime,
        chatModel: report.chat_model,
        embedModel: report.embed_model,
      }),
    );
  } catch (e) {
    container.textContent = "Requirements unavailable.";
  }
}
```

Call it in `goTo` when the step is 0 — `showOnboarding()` already calls `goTo(0)`,
so this one hook covers both first show and re-entry. Extend the existing
conditional chain (after `if (currentStep === 1) refreshEnginePath();` at
`onboarding.js:75`):

```js
if (currentStep === 0) refreshRequirements();
```

- [ ] **Step 3: Toast the backend truth**

`src/js/app.js` does not import from `notifications.js` yet. Add both imports next to
the others:

```js
import { showToast } from "./notifications.js";
import { gpuWarningOnce } from "./engine-status.js";
```

Add this helper after the imports and before the single event listener. The
`llama_engine_started` event can arrive before the log reader has parsed the
offload lines, so poll briefly for a known state rather than treating `unknown`
as CPU or silently missing the warning:

```js
async function warnWhenEngineStatusKnown() {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const snapshot = await invoke(
        "engine_runtime_info",
        {},
        { silent: true },
      );
      const info = snapshot?.chat;
      if (info?.backend && info.backend !== "unknown") {
        const warning = gpuWarningOnce(info);
        if (warning) showToast(warning, { variant: "warn", timeoutMs: 8000 });
        return;
      }
    } catch {
      // Engine state can be unavailable during unlock/reconfiguration; retry
      // within this bounded window, then let the normal UI poll take over.
    }
    await new Promise((resolve) => window.setTimeout(resolve, 500));
  }
}
```

Inside the `app_event` listener, after the onboarding branch and before
`handleAppEvent(payload)` (`app.js:30-35`), replace the existing comment and
dispatch lines with:

```js
if (payload?.type === "engine_runtime_status") {
  // Refresh System immediately if it has already been loaded.
  window.refreshSystemTab?.();
  const warning = gpuWarningOnce(payload.chat);
  if (warning) showToast(warning, { variant: "warn", timeoutMs: 8000 });
} else if (payload?.type === "llama_engine_started") {
  // Provision and direct restarts both emit this before log parsing necessarily
  // finishes; the bounded poll warns once status becomes authoritative.
  if (payload.service === "chat") void warnWhenEngineStatusKnown();
}
// Tell the user the truth about where inference runs (Q1: warn, never block).
```

- [ ] **Step 4: Verify**

Run: `node --check src/js/onboarding.js; node --check src/js/app.js`
Expected: no output from either.

Run: `Get-ChildItem tools\js-checks -Filter *.test.mjs | ForEach-Object { node $_.FullName }`
Expected: every harness exits 0.

- [ ] **Step 5: Commit**

```bash
git add src/index.html src/js/onboarding.js src/js/app.js
git commit -m "feat(onboarding): disclose requirements and warn on cpu fallback"
```

---

### Task 14: Docs, full verification, and the live check

**Files:**

- Modify: `AGENTS.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Record the decisions**

In `AGENTS.md`, add to the Known Decisions table:

```markdown
| D-161 | `EngineProbe` trait (`ai/probe/mod.rs`) is the only place that knows an engine's CLI flags or log format; `LlamaCppProbe` is the sole implementation and owns `-ngl`/`-lv 4`, `--list-devices`, and the device/offload/buffer regexes. `EngineRuntimeInfo` (snake_case, carries `engine` as a label) is the wire contract | The engine will be replaced eventually; without the boundary a swap rewrites the UI, preflight, and wire format. Six methods, one impl - deliberately small, and it must not grow past what the UI reads |
| D-162 | GPU layers default to `auto` (llama.cpp's fit-aware default); `normalize_gpu_layers` accepts `auto`/`all`/`0..=400` and falls back to `auto` with a warning. Migration 018 rewrites only the seeded `'99'`. The VC++ runtime is detected (`runtime_env::msvc_runtime_present`) and disclosed, never bundled | A hard pin (`99`) overrides both `-ngl`'s default and `--fit`, so an oversized model fails to load instead of partially offloading. The bundled binaries import `vcruntime140*.dll`/`msvcp140.dll`, which the installer does not ship - a clean machine fails with `0xc0000135` and no explanation |
```

Then amend the existing **D-125** row in place (it is not a new decision - it is the
same lifecycle decision with a different process type):

```markdown
| D-125 | `tokio::process::Command` for lifecycle (amended 2026-10-30; was `std::process::Command`) | The engine log drain must read the child's pipes through `tokio::io::AsyncRead`, and `std::process::ChildStdout` does not implement it - with `std::process` the drain cannot compile without a blocking thread per stream. Still no external process crate; the reap guarantee (`kill()` then `wait()`, no orphaned `llama-server.exe`) is unchanged |
```

Also update the "Latest code pass" block at the top of `AGENTS.md` with what shipped, the test counts, and the open live checks.

Update the `File Structure` section as well: show `src-tauri/src/ai/engine_log.rs`,
the new `src-tauri/src/ai/probe/{mod.rs,llama_cpp.rs}` subtree, and
`src-tauri/src/runtime_env.rs`. The file-tree entry for `ai/` should make clear
that only `probe/llama_cpp.rs` owns llama.cpp flags/log parsing; this keeps the
portable-engine boundary discoverable to the next session.

- [ ] **Step 2: Update `CHANGELOG.md`**

Add an entry at the top listing every file created/modified by this plan, one line each, plus the two decisions.

- [ ] **Step 3: Run the full verification suite**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
Get-ChildItem src\js -Recurse -Filter *.js | ForEach-Object { node --check $_.FullName }
Get-ChildItem tools\js-checks -Filter *.test.mjs | ForEach-Object { node $_.FullName }
```

Expected: fmt clean, all Rust tests pass (previous 95 + 27 new = 122), `cargo check` 0 errors (6 baseline warnings), every `node --check` silent, every harness exits 0 (9 existing + `requirements` + `engine-status` = 11).

Per-task expected Rust counts, so a regression is visible immediately:
`probe::tests` 1 · `llama_cpp` 3 → 12 · `engine_log` 4 · `ai::engine` 3 → 7 ·
`wire_tests` 4 → 6 · `runtime_env::tests` 1 · `gpu_layers_migration` 1 ·
`preflight::tests` 2.

- [ ] **Step 4: Live verification on the real runtime**

```bash
cargo tauri build --bundles msi nsis
.\tools\test-run.ps1 -Profile upgrade
```

Then in the app: unlock the vault, open System, and confirm:

1. The Chat row reads `GPU - 33/33 layers offloaded` with the RTX 3060 named; the
   Embed row independently reports its own backend, device, and offload ratio.
2. `engines/` log file `<data>/logs/llama-chat.log` exists and contains the offload lines.
3. Requirements shows exactly two `Included` rows, `Detected - NVIDIA GeForce RTX 3060`, `Detected` for the VC++ runtime, and both models imported.
4. Settings shows `Current: GPU - 33/33 layers offloaded (NVIDIA GeForce RTX 3060)`.
5. Set GPU layers to `0`, Restart, and confirm the toast warns that GPU acceleration
   is inactive and both System rows show `CPU only`; set it back to `auto`, Restart,
   and confirm the rows return to their actual GPU offload states.

- [ ] **Step 5: Commit**

`AGENTS.md` and `CHANGELOG.md` are both gitignored (`.gitignore` has `*.md` plus
explicit entries for each), so there is nothing to commit for them — they are
updated on disk only, exactly like the migration-doc fix already made there. Verify
the tracked work is clean instead:

```bash
git status --short
git diff --check
```

If the repo policy changes to track them, that is a separate `git add -f` decision —
do not force-add as part of this plan.

---

## Self-Review Notes

- **Spec coverage:** §4.1 → Tasks 2, 7 (default, normalise, migration); §4.2 → Tasks 4, 5 (drain, ring, rotation, both streams); §4.3 → Tasks 1–3 (trait, llama.cpp impl, parsing); §4.4 → Tasks 1, 5, 6 (`EngineRuntimeInfo`, `chat_info`, command, event); §4.5 → Tasks 11, 12, 13 (Settings, System, toast); §4.6 → Tasks 9, 12, 13 (shared module, System card, onboarding pane 0); §4.7 → Task 8 (both preflight checks); §7 tests → inline in every task; §10 files → matches the File Structure table.
- **Deliberate deviations from the spec, both recorded here:** `normalize_gpu_layers` lives in `ai/probe/llama_cpp.rs` rather than `ai/engine.rs` (the value grammar is llama.cpp's, not the engine's); the trait has a sixth `reset()` method instead of rebuilding the probe box on restart (same effect, no factory plumbing). The spec was updated to match.
- **Type consistency:** `EngineRuntimeInfo` fields are identical in Tasks 1, 5, 6, 12; `requirementRows()` keys match `requirementRows` tests and both call sites; `engine_requirements` wire keys (`engine`, `devices`, `msvc_runtime`, `chat_model`, `embed_model`) match the JS readers in Tasks 12 and 13.
- **Known rough edge:** `check_msvc_runtime` introduces the codebase's first `unsafe` block (one documented `LoadLibraryW` call). The alternative was a new `windows-sys` dependency, which needs a network fetch to vendor.
