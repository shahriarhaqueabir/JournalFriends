let toastContainer = null;

function ensureToastContainer() {
  if (toastContainer) return toastContainer;
  toastContainer = document.getElementById("toast-container");
  if (!toastContainer) {
    toastContainer = document.createElement("div");
    toastContainer.id = "toast-container";
    document.body.appendChild(toastContainer);
  }
  toastContainer.setAttribute("role", "status");
  toastContainer.setAttribute("aria-live", "polite");
  return toastContainer;
}

export function stringifyError(err) {
  if (err == null) return "Unknown error";
  if (typeof err === "string") return err;
  if (typeof err === "object") {
    if (typeof err.message === "string" && err.message.trim()) return err.message;
    try {
      return JSON.stringify(err);
    } catch {
      return String(err);
    }
  }
  return String(err);
}

export function showToast(message, { variant = "info", timeoutMs = 5000 } = {}) {
  const container = ensureToastContainer();
  const toast = document.createElement("div");
  toast.className = `toast toast--${variant}`;

  const textSpan = document.createElement("span");
  textSpan.className = "toast-message";
  textSpan.textContent = message;
  toast.appendChild(textSpan);

  const closeBtn = document.createElement("button");
  closeBtn.type = "button";
  closeBtn.className = "icon-btn toast-dismiss";
  closeBtn.setAttribute("aria-label", "Dismiss notification");
  closeBtn.innerHTML = '<hj-icon name="x" size="16" aria-hidden="true"></hj-icon>';
  toast.appendChild(closeBtn);

  container.appendChild(toast);
  window.renderIcons?.();

  const dismiss = () => {
    toast.classList.add("toast--hide");
    window.setTimeout(() => toast.remove(), 180);
  };

  const timeout = window.setTimeout(dismiss, timeoutMs);

  closeBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    window.clearTimeout(timeout);
    dismiss();
  });

  toast.addEventListener("click", () => {
    window.clearTimeout(timeout);
    dismiss();
  });
}

export function showError(err, { title } = {}) {
  const msg = stringifyError(err);
  showToast(title ? `${title}: ${msg}` : msg, { variant: "error" });
}
