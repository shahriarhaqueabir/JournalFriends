import { invoke } from "../ipc.js";
import { getDiagnosticsSnapshot, recordDiagnostic } from "../diagnostics.js";
import { escapeHtml } from "../sanitize.js";
import { showToast, showError, stringifyError } from "../notifications.js";
import { initSettingsDiagnostics } from "./settings-diagnostics.js";

let _settingsInitialized = false;

export async function initSettings() {
  if (_settingsInitialized) return;
  _settingsInitialized = true;
  window._settingsInitialized = true;

  const btnSubmitBug = document.getElementById("btn-submit-bug");
  const feedbackInput = document.getElementById("bug-report-feedback");
  const statusEl = document.getElementById("bug-report-status");
  const btnSystemCheck = document.getElementById("btn-run-system-check");
  const btnChangeVault = document.getElementById("btn-change-vault-password");
  const vaultStatusEl = document.getElementById("vault-settings-status");
  const vaultErrorEl = document.getElementById("vault-password-error");
  const vaultInputs = [
    "vault-current-password",
    "vault-new-password",
    "vault-confirm-new-password",
  ]
    .map((id) => document.getElementById(id))
    .filter(Boolean);

  function clearVaultError() {
    if (vaultErrorEl) {
      vaultErrorEl.textContent = "";
      vaultErrorEl.classList.add("hidden");
    }
    vaultInputs.forEach((el) => el.removeAttribute("aria-invalid"));
  }

  function showVaultError(message, inputIds = []) {
    if (vaultStatusEl) {
      vaultStatusEl.className = "settings-status";
      vaultStatusEl.textContent = "";
    }
    if (vaultErrorEl) {
      vaultErrorEl.textContent = message;
      vaultErrorEl.classList.remove("hidden");
    }
    const marked = inputIds
      .map((id) => document.getElementById(id))
      .filter(Boolean);
    marked.forEach((el) => el.setAttribute("aria-invalid", "true"));
    marked[0]?.focus();
  }

  // T-06: change vault master password — rekeys the DB and re-encrypts
  // journal content on the backend; the UI just passes the two passwords.
  if (btnChangeVault) {
    btnChangeVault.addEventListener("click", async () => {
      const currentPassword = document.getElementById(
        "vault-current-password",
      ).value;
      const newPassword = document.getElementById("vault-new-password").value;
      const confirmPassword = document.getElementById(
        "vault-confirm-new-password",
      ).value;

      clearVaultError();
      if (!currentPassword || !newPassword) {
        showVaultError("Enter both the current and a new master password.", [
          "vault-current-password",
          "vault-new-password",
        ]);
        return;
      }
      if (newPassword !== confirmPassword) {
        showVaultError("The new password and its confirmation do not match.", [
          "vault-confirm-new-password",
        ]);
        return;
      }
      if (newPassword.length < 4) {
        showVaultError("The new password must be at least 4 characters.", [
          "vault-new-password",
        ]);
        return;
      }

      try {
        btnChangeVault.disabled = true;
        btnChangeVault.textContent = "Changing…";
        vaultStatusEl.className = "settings-status";
        vaultStatusEl.textContent = "";
        await invoke("change_vault_password", {
          oldPassword: currentPassword,
          newPassword,
        });
        document.getElementById("vault-current-password").value = "";
        document.getElementById("vault-new-password").value = "";
        document.getElementById("vault-confirm-new-password").value = "";
        vaultStatusEl.className = "settings-status success";
        vaultStatusEl.textContent =
          "Master password changed — your journal was re-encrypted with the new password.";
        showToast("Vault password changed successfully.", {
          variant: "success",
        });
      } catch (e) {
        console.error("Failed to change vault password", e);
        showVaultError(stringifyError(e), ["vault-current-password"]);
      } finally {
        btnChangeVault.disabled = false;
        btnChangeVault.textContent = "Change Master Password";
      }
    });
  }

  // LLM Engine dropdowns + engine status — repointed to the engine section
  // (old AI Configuration model dropdown was removed in the llama.cpp migration).
  window.refreshAiSettings = () => {
    if (window.refreshLlamaEngine) window.refreshLlamaEngine();
  };

  // P-05: Run System Check — invokes preflight_run and renders the checklist.
  if (btnSystemCheck) {
    btnSystemCheck.addEventListener("click", runSystemCheck);
  }

  initLLMEngineSection();
  initAppearanceSection();

  window.refreshAiSettings();

  // Settings observability — activity log + runtime/logs cards.
  try {
    await initSettingsDiagnostics();
  } catch (e) {
    console.error("[SETTINGS] diagnostics init failed", e);
  }

  if (!btnSubmitBug) return;

  btnSubmitBug.addEventListener("click", async () => {
    const feedback = feedbackInput.value.trim();
    if (!feedback) {
      statusEl.className = "settings-status error";
      statusEl.textContent = "Please provide some details about the bug first.";
      return;
    }

    try {
      btnSubmitBug.disabled = true;
      btnSubmitBug.textContent = "Generating...";
      statusEl.className = "settings-status";
      statusEl.textContent = "";
      recordDiagnostic("bug_report:requested", {
        feedback_length: feedback.length,
        frontend_event_count: getDiagnosticsSnapshot().length,
      });

      const appDiagnostics = await invoke(
        "app_diagnostics",
        {},
        { silent: true },
      ).catch((err) => ({
        error: stringifyError(err),
      }));

      const reportPath = await invoke("report_bug", {
        userFeedback:
          `${feedback}\n\n## Frontend Diagnostics\n${JSON.stringify(getDiagnosticsSnapshot(), null, 2)}` +
          `\n\n## App Diagnostics\n${JSON.stringify(appDiagnostics, null, 2)}`,
      });

      feedbackInput.value = "";
      statusEl.className = "settings-status success";
      statusEl.textContent = `Bug report generated successfully at:\n${reportPath}`;
    } catch (err) {
      console.error("Failed to generate bug report:", err);
      statusEl.className = "settings-status error";
      statusEl.textContent = `Error: ${err}`;
    } finally {
      btnSubmitBug.disabled = false;
      btnSubmitBug.textContent = "Generate Bug Report";
    }
  });

  const btnRestartOnboarding = document.getElementById(
    "btn-restart-onboarding",
  );
  if (btnRestartOnboarding) {
    console.log("[SETTINGS] Wiring restart onboarding button");
    let confirmPending = false;
    let confirmTimer = null;

    btnRestartOnboarding.addEventListener("click", async () => {
      if (!confirmPending) {
        // First click: enter confirm state
        confirmPending = true;
        const origText = btnRestartOnboarding.textContent;
        btnRestartOnboarding.textContent = "Click again to confirm reset";
        btnRestartOnboarding.classList.add("btn-danger");
        confirmTimer = setTimeout(() => {
          confirmPending = false;
          btnRestartOnboarding.textContent = origText;
          btnRestartOnboarding.classList.remove("btn-danger");
        }, 5000);
        return;
      }

      // Second click within 5s: execute
      clearTimeout(confirmTimer);
      confirmPending = false;
      btnRestartOnboarding.classList.remove("btn-danger");

      try {
        btnRestartOnboarding.disabled = true;
        await invoke("restart_onboarding");
        // Backend emits OnboardingRequired → app.js shows the wizard immediately.
        showToast("Onboarding restarted. Setup wizard is opening…", {
          variant: "success",
        });
      } catch (e) {
        showError(e, { title: "Failed to restart onboarding" });
      } finally {
        btnRestartOnboarding.disabled = false;
        btnRestartOnboarding.textContent = "Restart Onboarding";
      }
    });
  }
}

/** P-05: run preflight_run and render per-check results with colors. */
async function runSystemCheck() {
  const btn = document.getElementById("btn-run-system-check");
  const listEl = document.getElementById("system-check-list");
  const summaryEl = document.getElementById("system-check-summary");
  if (!btn || !listEl || !summaryEl) return;

  btn.disabled = true;
  btn.textContent = "Running…";
  summaryEl.classList.add("hidden");
  listEl.classList.remove("hidden");
  listEl.innerHTML =
    '<div class="system-check-item is-running"><span class="onboarding-spinner"></span><div class="system-check-content">Running checks…</div></div>';
  recordDiagnostic("system_check:requested", {});

  try {
    const report = await invoke("preflight_run", {}, { silent: true });
    renderSystemCheck(summaryEl, listEl, report);
  } catch (e) {
    // Graceful error state — backend unreachable or command failed.
    console.error("System check failed:", e);
    listEl.innerHTML =
      `<div class="system-check-item system-check-fail">` +
      `<span class="system-check-icon"><hj-icon name="circle-x" size="16" aria-hidden="true"></hj-icon></span>` +
      `<div class="system-check-content">` +
      `<div class="system-check-label">System check failed</div>` +
      `<div class="system-check-message">${escapeHtml(stringifyError(e))}</div>` +
      `</div></div>`;
    summaryEl.className = "system-check-summary summary-fail";
    summaryEl.innerHTML =
      `<div class="summary-icon"><hj-icon name="circle-x" size="16" aria-hidden="true"></hj-icon></div>` +
      `<div class="summary-text">Could not complete the system check.</div>`;
    window.renderIcons?.();
  } finally {
    btn.disabled = false;
    btn.textContent = "Run Again";
  }
}

function renderSystemCheck(summaryEl, listEl, report) {
  const checks = report.checks || [];
  const total = checks.length;
  listEl.innerHTML = "";

  if (total === 0) {
    listEl.innerHTML =
      '<div class="system-check-item system-check-warn"><span class="system-check-icon"><hj-icon name="circle-alert" size="16" aria-hidden="true"></hj-icon></span><div class="system-check-content"><div class="system-check-label">No checks were returned by the system check.</div></div></div>';
    summaryEl.className = "system-check-summary summary-warn";
    summaryEl.innerHTML =
      '<div class="summary-icon"><hj-icon name="circle-alert" size="16" aria-hidden="true"></hj-icon></div><div class="summary-text">No results — the report was empty.</div>';
    recordDiagnostic("system_check:complete", { total: 0, fail: 0, warn: 0 });
    return;
  }

  const failCount = checks.filter((c) => c.status === "fail").length;
  const warnCount = checks.filter((c) => c.status === "warn").length;
  const passCount = total - failCount - warnCount;
  const icons = {
    pass: "circle-check",
    warn: "circle-alert",
    fail: "circle-x",
  };

  for (const check of checks) {
    const status =
      check.status === "pass"
        ? "pass"
        : check.status === "warn"
          ? "warn"
          : "fail";
    const item = document.createElement("div");
    item.className = `system-check-item system-check-${status}`;
    item.innerHTML =
      `<span class="system-check-icon"><hj-icon name="${icons[status]}" size="16" aria-hidden="true"></hj-icon></span>` +
      `<div class="system-check-content">` +
      `<div class="system-check-label">${escapeHtml(check.label)}</div>` +
      (check.message
        ? `<div class="system-check-message">${escapeHtml(check.message)}</div>`
        : "") +
      `</div>` +
      `<span class="badge badge-${status}">${status}</span>`;
    listEl.appendChild(item);
  }

  let summaryClass = "system-check-summary";
  let icon = "circle-check";
  let text;
  if (failCount === 0 && warnCount === 0) {
    summaryClass += " summary-ok";
    text = `All ${total} checks passed — your system is ready.`;
  } else if (failCount === 0) {
    summaryClass += " summary-warn";
    icon = "circle-alert";
    text = `${warnCount} warning${warnCount === 1 ? "" : "s"} — check the details below.`;
  } else {
    summaryClass += " summary-fail";
    icon = "circle-x";
    text = `${failCount} check${failCount === 1 ? "" : "s"} failed — fix the issues below.`;
  }

  summaryEl.className = summaryClass;
  summaryEl.innerHTML =
    `<div class="summary-icon"><hj-icon name="${icon}" size="16" aria-hidden="true"></hj-icon></div>` +
    `<div class="summary-text">${text}</div>` +
    `<div class="summary-counts">` +
    `<span class="count-chip count-pass">${passCount} passed</span>` +
    (warnCount
      ? `<span class="count-chip count-warn">${warnCount} warning${warnCount === 1 ? "" : "s"}</span>`
      : "") +
    (failCount
      ? `<span class="count-chip count-fail">${failCount} failed</span>`
      : "") +
    `</div>`;

  recordDiagnostic("system_check:complete", {
    total,
    fail: failCount,
    warn: warnCount,
  });
  window.renderIcons?.();
}

/**
 * LLM Engine card: service status, model pickers, GGUF import, config.
 * Uses the tauri-plugin-dialog `open()` API for file selection — Tauri v2
 * removed `File.path` (the old HTML file-input hack does not work).
 */
async function initLLMEngineSection() {
  const chatSelect = document.getElementById("chat-model-select");
  const embedSelect = document.getElementById("embed-model-select");
  const chatStatus = document.getElementById("chat-status");
  const embedStatus = document.getElementById("embed-status");
  const chatPortLabel = document.getElementById("chat-port-label");
  const embedPortLabel = document.getElementById("embed-port-label");
  const exePath = document.getElementById("llama-exe-path");
  const exeHint = document.getElementById("llama-exe-hint");
  const ngl = document.getElementById("llama-ngl");
  const ctxSize = document.getElementById("llama-ctx-size");
  if (!chatSelect || !embedSelect) return;

  // Show the executable the backend actually launches. Resolution happens in
  // Rust (app-data engines/ → packaged resource → saved override), so the UI
  // reports the real path instead of a made-up placeholder.
  async function refreshEnginePathHint() {
    try {
      const info = await invoke("engine_path_info", {}, { silent: true });
      if (exePath)
        exePath.placeholder = info.resolved || "No llama-server.exe found";
      if (!exeHint) return;
      const savedPath = (exePath?.value || "").trim();
      const label =
        info.source === "app_local"
          ? "Bundled runtime (app data)"
          : info.source === "bundled_resource"
            ? "Bundled runtime"
            : info.source === "saved"
              ? "Saved executable"
              : null;
      if (!label || !info.resolved) {
        exeHint.textContent =
          "No llama-server.exe found. Restore the bundled engines folder or select your own llama.cpp build.";
      } else {
        let text = `${label}: ${info.resolved}`;
        if (savedPath && info.source !== "saved") {
          text +=
            " — the saved path is ignored while the bundled runtime is present.";
        }
        exeHint.textContent = text;
      }
      exeHint.title = info.resolved || "";
    } catch (e) {
      if (exeHint) exeHint.textContent = "Could not read the engine path.";
      console.error("[LLM ENGINE] engine_path_info failed", e);
    }
  }

  async function loadSettings() {
    try {
      const [
        chatPort,
        embedPort,
        savedNgl,
        savedCtx,
        savedExe,
        savedChat,
        savedEmbed,
      ] = await Promise.all([
        invoke("setting_get", { key: "llama_chat_port" }),
        invoke("setting_get", { key: "llama_embed_port" }),
        invoke("setting_get", { key: "llama_ngl" }),
        invoke("setting_get", { key: "llama_ctx_size" }),
        invoke("setting_get", { key: "llama_exe_path" }),
        invoke("setting_get", { key: "llama_chat_model" }),
        invoke("setting_get", { key: "llama_embed_model" }),
      ]);
      if (chatPortLabel) chatPortLabel.textContent = `:${chatPort || "8080"}`;
      if (embedPortLabel)
        embedPortLabel.textContent = `:${embedPort || "8081"}`;
      if (exePath) {
        exePath.value = savedExe || "";
        exePath.placeholder = "";
      }
      if (ngl) ngl.value = savedNgl || "99";
      if (ctxSize) ctxSize.value = savedCtx || "16384";
      await refreshModelList();
    } catch (e) {
      console.error("[LLM ENGINE] loadSettings failed", e);
    }
  }

  async function refreshModelList() {
    try {
      const [chatModels, embedModels, savedChat, savedEmbed] =
        await Promise.all([
          invoke("list_chat_models"),
          invoke("list_embed_models"),
          invoke("setting_get", { key: "llama_chat_model" }),
          invoke("setting_get", { key: "llama_embed_model" }),
        ]);
      chatSelect.innerHTML =
        chatModels && chatModels.length
          ? chatModels.map((m) => optionHtml(m, savedChat))
          : '<option value="">No chat GGUF imported</option>';
      embedSelect.innerHTML =
        embedModels && embedModels.length
          ? embedModels.map((m) => optionHtml(m, savedEmbed))
          : '<option value="">No embed GGUF imported</option>';
    } catch (e) {
      console.error("[LLM ENGINE] refreshModelList failed", e);
    }
  }

  window.refreshLlamaEngine = refreshModelList;

  function optionHtml(m, saved) {
    const selected = m.filename === saved ? " selected" : "";
    const size = m.size_bytes ? ` (${(m.size_bytes / 1e9).toFixed(1)} GB)` : "";
    return `<option value="${escapeHtml(m.filename)}"${selected}>${escapeHtml(m.name)}${size}</option>`;
  }

  async function updateEngineStatus() {
    try {
      const health = await invoke("check_engine_health", {}, { silent: true });
      chatStatus.className = `status-dot ${health.chat ? "online" : "offline"}`;
      embedStatus.className = `status-dot ${health.embed ? "online" : "offline"}`;
    } catch (e) {
      chatStatus.className = "status-dot offline";
      embedStatus.className = "status-dot offline";
    }
  }

  async function pickFile(kind) {
    const filters =
      kind === "exe"
        ? [{ name: "Executable", extensions: ["exe"] }]
        : [{ name: "GGUF", extensions: ["gguf"] }];
    const dialog = window?.__TAURI__?.dialog;
    if (!dialog || typeof dialog.open !== "function") {
      showError("File dialog unavailable", { title: "Select file" });
      return null;
    }
    try {
      return await dialog.open({ multiple: false, filters });
    } catch (e) {
      showError(e, { title: "File dialog failed" });
      return null;
    }
  }

  document
    .getElementById("btn-browse-exe")
    ?.addEventListener("click", async () => {
      const path = await pickFile("exe");
      if (path) {
        exePath.value = path;
        await invoke("setting_set", { key: "llama_exe_path", value: path });
        await refreshEnginePathHint();
        showToast("llama-server path saved. Restart the engine to apply.", {
          variant: "success",
        });
      }
    });

  document
    .getElementById("btn-import-chat")
    ?.addEventListener("click", async () => {
      await importModel("chat");
    });
  document
    .getElementById("btn-import-embed")
    ?.addEventListener("click", async () => {
      await importModel("embed");
    });

  async function importModel(modelType) {
    const btn = document.getElementById(`btn-import-${modelType}`);
    const path = await pickFile("gguf");
    if (!path) return;
    try {
      if (btn) {
        btn.disabled = true;
        btn.textContent = "Importing…";
      }
      await invoke("import_model", { source: path, modelType });
      showToast(`Imported ${modelType} model. Select it below, then Restart.`, {
        variant: "success",
      });
      await refreshModelList();
    } catch (e) {
      showError(e, { title: "Import failed" });
    } finally {
      if (btn) {
        btn.disabled = false;
        btn.textContent = "Import GGUF";
      }
    }
  }

  document
    .getElementById("btn-restart-chat")
    ?.addEventListener("click", async () => {
      await restartService("chat");
    });
  document
    .getElementById("btn-restart-embed")
    ?.addEventListener("click", async () => {
      await restartService("embed");
    });

  async function restartService(service) {
    try {
      const outcome = await invoke("restart_engine", { service });
      showToast(
        outcome && outcome.note ? outcome.note : `${service} engine restarted.`,
        { variant: "success" },
      );
      await updateEngineStatus();
    } catch (e) {
      showError(e, { title: `Failed to restart ${service}` });
      await updateEngineStatus();
    }
  }

  const persist = (key, value) => {
    invoke("setting_set", { key, value }).catch((e) =>
      showError(e, { title: "Failed to save setting" }),
    );
  };

  chatSelect.addEventListener("change", () =>
    persist("llama_chat_model", chatSelect.value),
  );
  embedSelect.addEventListener("change", () =>
    persist("llama_embed_model", embedSelect.value),
  );
  exePath.addEventListener("change", async () => {
    persist("llama_exe_path", exePath.value);
    await refreshEnginePathHint();
  });
  ngl.addEventListener("change", () => persist("llama_ngl", ngl.value));
  ctxSize.addEventListener("change", () =>
    persist("llama_ctx_size", ctxSize.value),
  );

  await refreshEnginePathHint();
  await loadSettings();
  await refreshEnginePathHint();
  await updateEngineStatus();
  // Re-probe status periodically so the card tracks engine state.
  setInterval(updateEngineStatus, 30000);
}

// ── Appearance: custom background image ───────────────────────
async function initAppearanceSection() {
  const pathInput = document.getElementById("bg-image-path");
  const btnBrowse = document.getElementById("btn-browse-bg");
  const btnClear = document.getElementById("btn-clear-bg");
  const slider = document.getElementById("bg-opacity");
  const preview = document.getElementById("bg-preview");
  if (!btnBrowse) return;

  async function load() {
    try {
      const [p, o] = await Promise.all([
        invoke("setting_get", { key: "background_image" }),
        invoke("setting_get", { key: "background_opacity" }),
      ]);
      if (pathInput) pathInput.value = p || "";
      if (slider) slider.value = o || "40";
      if (preview) {
        if (p) {
          try {
            const dataUrl = await invoke("background_read_image", { path: p });
            preview.style.backgroundImage = `url("${dataUrl}")`;
            preview.classList.remove("hidden");
          } catch (e) {
            preview.classList.add("hidden");
          }
        } else {
          preview.classList.add("hidden");
          preview.style.backgroundImage = "";
        }
      }
    } catch (e) {
      console.error("[APPEARANCE] load failed", e);
    }
  }

  btnBrowse.addEventListener("click", async () => {
    const dialog = window?.__TAURI__?.dialog;
    if (!dialog || typeof dialog.open !== "function") {
      showError("File dialog unavailable", { title: "Select image" });
      return;
    }
    let path;
    try {
      path = await dialog.open({
        multiple: false,
        filters: [
          {
            name: "Image",
            extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"],
          },
        ],
      });
    } catch (e) {
      showError(e, { title: "File dialog failed" });
      return;
    }
    if (!path) return;
    try {
      await invoke("setting_set", { key: "background_image", value: path });
      await invoke("setting_set", {
        key: "background_opacity",
        value: String(slider?.value || 40),
      });
      if (pathInput) pathInput.value = path;
      await load();
      await applyBackground();
      showToast("Background image set.", { variant: "success" });
    } catch (e) {
      showError(e, { title: "Failed to set background" });
    }
  });

  btnClear?.addEventListener("click", async () => {
    try {
      await invoke("setting_set", { key: "background_image", value: "" });
      if (pathInput) pathInput.value = "";
      if (preview) {
        preview.classList.add("hidden");
        preview.style.backgroundImage = "";
      }
      await applyBackground();
      showToast("Background cleared.", { variant: "success" });
    } catch (e) {
      showError(e, { title: "Failed to clear background" });
    }
  });

  slider?.addEventListener("change", async () => {
    try {
      await invoke("setting_set", {
        key: "background_opacity",
        value: String(slider.value),
      });
      await applyBackground();
    } catch (e) {
      showError(e, { title: "Failed to save opacity" });
    }
  });

  await load();
}

export async function applyBackground() {
  const layer = document.getElementById("bg-layer");
  const scrim = document.getElementById("bg-scrim");
  if (!layer || !scrim) return;
  try {
    const [p, o] = await Promise.all([
      invoke("setting_get", { key: "background_image" }),
      invoke("setting_get", { key: "background_opacity" }),
    ]);
    if (!p) {
      layer.classList.remove("active");
      scrim.classList.remove("active");
      layer.style.backgroundImage = "";
      document.body.classList.remove("has-bg-image");
      document.documentElement.classList.remove("has-bg-image");
      return;
    }
    const dataUrl = await invoke("background_read_image", { path: p });
    layer.style.backgroundImage = `url("${dataUrl}")`;
    layer.classList.add("active");
    scrim.classList.add("active");
    scrim.style.opacity = String((Number(o) || 40) / 100);
    document.body.classList.add("has-bg-image");
    document.documentElement.classList.add("has-bg-image");
  } catch (e) {
    console.error("[APPEARANCE] applyBackground failed", e);
    layer.classList.remove("active");
    scrim.classList.remove("active");
    document.body.classList.remove("has-bg-image");
    document.documentElement.classList.remove("has-bg-image");
  }
}
