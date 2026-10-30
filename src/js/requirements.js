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
