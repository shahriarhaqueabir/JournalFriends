import { invoke } from "./ipc.js";
import { recordDiagnostic } from "./diagnostics.js";
import { stringifyError } from "./notifications.js";

let vaultOverlay = null;
let passwordInput = null;
let confirmInput = null;
let warningEl = null;
let unlockBtn = null;
let errorEl = null;
// True once a master password already exists on disk (encrypted DB). When
// false the next unlock CREATES the password, so we require a confirm field.
// Defaults to true so a status-check failure never forces existing users
// through the create-password flow (T-04).
let passwordSet = true;

export function initVault() {
  vaultOverlay = document.getElementById("vault-overlay");
  passwordInput = document.getElementById("vault-password");
  confirmInput = document.getElementById("vault-confirm");
  warningEl = document.getElementById("vault-warning");
  unlockBtn = document.getElementById("btn-unlock-vault");
  errorEl = document.getElementById("vault-error");

  if (!unlockBtn || !passwordInput) return;

  unlockBtn.addEventListener("click", unlockVault);
  passwordInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      unlockVault();
    }
  });
  confirmInput?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      unlockVault();
    }
  });

  checkVaultStatus();
}

async function checkVaultStatus() {
  try {
    const status = await invoke("vault_status", {}, { silent: true });
    passwordSet = !!status.passwordSet;
    if (!status.unlocked) {
      showVault();
    } else {
      hideVault();
    }
  } catch (err) {
    console.error("Failed to check vault status", err);
    showVault();
  }
}

export function showVault() {
  vaultOverlay?.classList.remove("hidden");
  // First unlock creates the password — require confirmation and warn that
  // it is unrecoverable (T-04).
  if (!passwordSet) {
    confirmInput?.classList.remove("hidden");
    warningEl?.classList.remove("hidden");
  } else {
    confirmInput?.classList.add("hidden");
    warningEl?.classList.add("hidden");
  }
  window.renderIcons?.();
  passwordInput?.focus();
}

export function hideVault() {
  vaultOverlay?.classList.add("hidden");
}

window.showVault = showVault;
window.hideVault = hideVault;

async function unlockVault() {
  const password = passwordInput.value;
  if (!password) {
    showError("Please enter a password.");
    return;
  }
  // First unlock creates the password — the confirm field must match (T-04).
  if (!passwordSet && confirmInput && confirmInput.value !== password) {
    showError("Passwords do not match.");
    confirmInput.focus();
    return;
  }

  unlockBtn.disabled = true;
  unlockBtn.textContent = "Unlocking...";
  showError("");

  try {
    await invoke("unlock_vault", { password });
    recordDiagnostic("vault:unlocked", {});
    passwordInput.value = "";
    if (confirmInput) confirmInput.value = "";
    hideVault();

    // T-07: Trigger onboarding check immediately after unlock
    await invoke("check_onboarding", {}, { silent: true });

    // Notify tabs to refetch — their startup fetch ran while the vault was
    // locked (VaultLocked) and no other refresh path exists.
    document.dispatchEvent(new CustomEvent("vault:unlocked"));
  } catch (err) {
    console.error("Unlock failed", err);
    showError(stringifyError(err));
    passwordInput.value = "";
    if (confirmInput) confirmInput.value = "";
    passwordInput.focus();
  } finally {
    unlockBtn.disabled = false;
    unlockBtn.textContent = "Unlock Journal";
  }
}

function showError(msg) {
  if (errorEl) {
    errorEl.textContent = msg;
    if (msg) {
      errorEl.setAttribute("role", "alert");
      passwordInput?.setAttribute("aria-invalid", "true");
    } else {
      passwordInput?.removeAttribute("aria-invalid");
      confirmInput?.removeAttribute("aria-invalid");
    }
  }
}
