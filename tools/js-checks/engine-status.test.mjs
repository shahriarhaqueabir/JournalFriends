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
