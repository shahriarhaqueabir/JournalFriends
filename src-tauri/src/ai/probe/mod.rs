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

pub mod llama_cpp;

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

pub use llama_cpp::{strip_ansi, LlamaCppProbe};

/// The engine the app ships with today.
pub fn default_probe() -> Box<dyn EngineProbe> {
    Box::new(LlamaCppProbe::default())
}

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
