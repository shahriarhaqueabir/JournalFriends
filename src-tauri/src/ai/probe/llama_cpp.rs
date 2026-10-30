//! llama.cpp implementation of `EngineProbe`.
//!
//! This is the ONLY module allowed to know llama.cpp's CLI flags or its log
//! line formats. A build bump that renames a log line degrades the status row
//! to `unknown`; it never changes engine behaviour or fails a start.

use super::{DeviceInfo, EngineProbe, EngineRuntimeInfo, ProbeError};
use std::ffi::OsString;
use std::path::Path;

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
}
