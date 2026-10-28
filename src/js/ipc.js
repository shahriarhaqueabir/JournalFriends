import { showError, stringifyError } from "./notifications.js";
import { recordDiagnostic } from "./diagnostics.js";

function formatIpcError(err, command) {
  return `${command}: ${stringifyError(err)}`;
}

// Settings observability: IPC args are NEVER logged as values — only
// { key, type, length } so the activity log shows shape without content.
function argSummary(args) {
  return Object.entries(args ?? {}).map(([key, v]) => ({
    key,
    type: Array.isArray(v)
      ? `array(${v.length})`
      : v === null
        ? "null"
        : typeof v,
    length: v == null ? 0 : String(v).length,
  }));
}

export function invoke(command, args = {}, { silent = false } = {}) {
  const tauriInvoke = window?.__TAURI__?.core?.invoke;
  const startedAt = performance.now();
  const summary = argSummary(args);

  if (typeof tauriInvoke !== "function") {
    const isBrowser = !window.hasOwnProperty("__TAURI_INTERNALS__") && !window.hasOwnProperty("__TAURI__");
    const msg = isBrowser 
      ? "Tauri invoke() is unavailable. Are you viewing this in a browser instead of the Tauri app window?"
      : "Tauri invoke() is unavailable. Check tauri.conf.json > withGlobalTauri: true.";
    
    const e = new Error(msg);
    if (!silent) showError(e, { title: "IPC Connectivity Error" });
    recordDiagnostic("ipc:error", { command, args: summary, duration_ms: 0, error: msg });
    return Promise.reject(e);
  }

  return tauriInvoke(command, args)
    .then((result) => {
      recordDiagnostic("ipc:ok", {
        command,
        args: summary,
        duration_ms: Math.round(performance.now() - startedAt),
      });
      return result;
    })
    .catch((err) => {
      recordDiagnostic("ipc:error", {
        command,
        args: summary,
        duration_ms: Math.round(performance.now() - startedAt),
        error: formatIpcError(err, command),
      });
      if (!silent) showError(formatIpcError(err, command), { title: "IPC error" });
      throw err;
    });
}