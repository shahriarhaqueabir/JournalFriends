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
    RE.get_or_init(|| Regex::new(r"(\w+) model buffer size =\s+([\d.]+) MiB").expect("valid regex"))
}

fn kv_buffer_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\w+) KV buffer size =\s+([\d.]+) MiB").expect("valid regex"))
}

/// Pick the buffer belonging to the accelerator. Prefers the device's entry;
/// falls back to the first non-CPU entry, then to the first entry overall (which
/// may be `CPU_Mapped` for CPU-only runs, where no accelerator buffer exists).
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

    /// Enumerate accelerators by running `exe --list-devices`. Blocking: the
    /// caller must invoke this off the async runtime (e.g. via spawn_blocking).
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

    fn reset(&mut self) {
        self.parsed = Parsed::default();
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
}

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
}
