// Formatting helpers for `engine_runtime_info` (EngineRuntimeInfo, snake_case).
// No DOM, no imports. `gpuWarningOnce` is stateful (module-level latch) so
// callers must not assume it is side-effect-free.
const BACKEND = {
  GPU: "gpu",
  PARTIAL: "partial",
  CPU: "cpu",
  UNKNOWN: "unknown",
};

export function backendLabel(info) {
  const backend = info?.backend || BACKEND.UNKNOWN;
  const offloaded = info?.layers_offloaded ?? 0;
  const total = info?.layers_total ?? 0;
  if (backend === BACKEND.GPU) {
    return `GPU - ${offloaded}/${total} layers offloaded`;
  }
  if (backend === BACKEND.PARTIAL) {
    return `Partial - ${offloaded}/${total} layers on GPU`;
  }
  if (backend === BACKEND.CPU) return "CPU only";
  return "Unknown - engine not started";
}

/** One line for Settings: what the engine is doing right now. */
export function inlineStatus(info) {
  const label = backendLabel(info);
  const device = info?.device_name;
  const backend = info?.backend;
  if ((backend === BACKEND.GPU || backend === BACKEND.PARTIAL) && device) {
    return `${label} (${device})`;
  }
  if (backend === BACKEND.CPU) {
    return `${label} - set GPU layers to auto and restart if this machine has a supported GPU`;
  }
  return label;
}

let lastWarned = null;

/** Warn once per backend transition; never block, never repeat. */
export function gpuWarningOnce(info) {
  const backend = info?.backend;
  if (backend === BACKEND.GPU || backend === BACKEND.UNKNOWN) {
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
  const offloaded = info?.layers_offloaded ?? 0;
  const total = info?.layers_total ?? 0;
  if (backend === BACKEND.PARTIAL) {
    return `${engine}: ${offloaded}/${total} layers on the GPU, the rest on CPU.`;
  }
  return `${engine} is running on CPU - GPU acceleration is not active. Replies will be slower.`;
}

export function resetGpuWarning() {
  lastWarned = null;
}
