// Light-DOM custom elements for the premium UI system.
// No Shadow DOM by design (design guard + CSP stay simple): styles live in
// styles/components/elements.css and are driven entirely by design tokens.
//
//   <hj-button variant="primary" icon="check">Save</hj-button>
//   <hj-card elevation="2" interactive> ... </hj-card>
//   <hj-metric-card label="Total Words" value="1234" icon="file-text" tone="purple"></hj-metric-card>
//   <hj-chip tone="accent">accent</hj-chip>
//   <hj-field label="Title" help="Optional"> <input/> </hj-field>
//   <hj-empty-state icon="inbox" title="Nothing here">Sub-text</hj-empty-state>
//   <hj-panel> overlay chrome </hj-panel>

function define(name, ctor) {
  if (!customElements.get(name)) customElements.define(name, ctor);
}

function iconEl(name, size = 16, tone) {
  const i = document.createElement("hj-icon");
  i.setAttribute("name", name);
  i.setAttribute("size", String(size));
  i.setAttribute("aria-hidden", "true");
  if (tone) i.setAttribute("tone", tone);
  return i;
}

// ── <hj-button> ──────────────────────────────────────────────
export class HjButton extends HTMLElement {
  static get observedAttributes() { return ["variant", "size", "icon", "disabled"]; }
  connectedCallback() {
    this._sync();
    if (!this._keyBound) {
      this._keyBound = true;
      this.addEventListener("keydown", (e) => {
        if (this.hasAttribute("disabled")) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          this.click();
        }
      });
    }
  }
  attributeChangedCallback() { if (this.isConnected) this._sync(); }
  _sync() {
    // Present as an accessible button without stealing inner content.
    if (!this.hasAttribute("role")) this.setAttribute("role", "button");
    this.tabIndex = this.hasAttribute("disabled") ? -1 : 0;
    if (this.hasAttribute("disabled")) this.setAttribute("aria-disabled", "true");
    else this.removeAttribute("aria-disabled");
    this.classList.add("hj-icon-host");

    const name = this.getAttribute("icon");
    const existing = this.querySelector(":scope > hj-icon");
    if (name && !existing) this.prepend(iconEl(name, 16));
    else if (name && existing && existing.getAttribute("name") !== name) existing.setAttribute("name", name);
    else if (!name && existing) existing.remove();
  }
  // Reflect `disabled` as a live property (mirrors native button semantics).
  get disabled() { return this.hasAttribute("disabled"); }
  set disabled(v) { if (v) this.setAttribute("disabled", ""); else this.removeAttribute("disabled"); }
}

// ── <hj-card> ────────────────────────────────────────────────
export class HjCard extends HTMLElement {
  connectedCallback() {
    this.classList.add("hj-icon-host");
    if (this.hasAttribute("interactive") && !this.hasAttribute("tabindex")) this.tabIndex = 0;
  }
}

// ── <hj-metric-card> ─────────────────────────────────────────
export class HjMetricCard extends HTMLElement {
  static get observedAttributes() { return ["label", "value", "icon", "tone"]; }
  connectedCallback() { this._render(); }
  attributeChangedCallback() { if (this.isConnected) this._render(); }
  _render() {
    if (this._built && !this.hasAttribute("value")) return;
    const label = this.getAttribute("label") || "";
    const value = this.getAttribute("value") || "";
    const icon = this.getAttribute("icon");
    const tone = this.getAttribute("tone");

    let wrap = this.querySelector(".metric-icon-wrap");
    if (!wrap) {
      wrap = document.createElement("div");
      wrap.className = "metric-icon-wrap";
      this.prepend(wrap);
    }
    wrap.replaceChildren(icon ? iconEl(icon, 18, tone) : document.createTextNode(""));

    let info = this.querySelector(".metric-info");
    if (!info) {
      info = document.createElement("div");
      info.className = "metric-info";
      this.appendChild(info);
    }
    let val = info.querySelector(".metric-value");
    let lbl = info.querySelector(".metric-label");
    if (!val) { val = document.createElement("span"); val.className = "metric-value"; info.appendChild(val); }
    if (!lbl) { lbl = document.createElement("span"); lbl.className = "metric-label"; info.appendChild(lbl); }
    if (val.textContent !== value) { val.textContent = value; val.classList.add("hj-count-pop"); }
    lbl.textContent = label;
    this._built = true;
  }
  setValue(v) {
    this.setAttribute("value", String(v));
    const val = this.querySelector(".metric-value");
    if (val) { val.textContent = String(v); val.classList.remove("hj-count-pop"); void val.offsetWidth; val.classList.add("hj-count-pop"); }
  }
}

// ── <hj-chip> / <hj-badge> ───────────────────────────────────
export class HjChip extends HTMLElement {
  connectedCallback() { if (!this.hasAttribute("role")) this.setAttribute("role", "status"); }
}
export class HjBadge extends HTMLElement {}

// ── <hj-field> ───────────────────────────────────────────────
export class HjField extends HTMLElement {
  static get observedAttributes() { return ["label", "help", "error", "for"]; }
  connectedCallback() { this._render(); }
  attributeChangedCallback() { if (this.isConnected) this._render(); }
  _render() {
    let label = this.querySelector(":scope > label");
    const labelText = this.getAttribute("label");
    if (labelText) {
      if (!label) { label = document.createElement("label"); this.prepend(label); }
      label.textContent = labelText;
      const forId = this.getAttribute("for");
      if (forId) label.setAttribute("for", forId);
    }

    let help = this.querySelector(":scope > .hj-field-help");
    const helpText = this.getAttribute("help");
    if (helpText) {
      if (!help) { help = document.createElement("p"); help.className = "hj-field-help"; this.appendChild(help); }
      help.textContent = helpText;
    } else if (help) help.remove();

    let err = this.querySelector(":scope > .hj-field-error");
    const errText = this.getAttribute("error");
    if (errText) {
      if (!err) { err = document.createElement("p"); err.className = "hj-field-error"; err.setAttribute("role", "alert"); this.appendChild(err); }
      err.textContent = errText;
    } else if (err) err.remove();
  }
}

// ── <hj-empty-state> ─────────────────────────────────────────
export class HjEmptyState extends HTMLElement {
  static get observedAttributes() { return ["icon", "title"]; }
  connectedCallback() { this._render(); }
  attributeChangedCallback() { if (this.isConnected) this._render(); }
  _render() {
    const icon = this.getAttribute("icon");
    const title = this.getAttribute("title");
    let wrap = this.querySelector(":scope > .hj-empty-icon");
    if (icon && !wrap) { wrap = document.createElement("div"); wrap.className = "hj-empty-icon"; wrap.appendChild(iconEl(icon, 22)); this.prepend(wrap); }
    else if (icon && wrap) wrap.replaceChildren(iconEl(icon, 22));
    else if (!icon && wrap) wrap.remove();
    if (title && !this.querySelector(":scope > .hj-empty-title")) {
      const t = document.createElement("p");
      t.className = "hj-empty-title";
      t.textContent = title;
      const anchor = this.querySelector(".hj-empty-icon");
      anchor ? anchor.after(t) : this.prepend(t);
    } else if (title) {
      const t = this.querySelector(":scope > .hj-empty-title");
      if (t) t.textContent = title;
    }
  }
}

// ── <hj-panel> ───────────────────────────────────────────────
export class HjPanel extends HTMLElement {}

define("hj-button", HjButton);
define("hj-card", HjCard);
define("hj-metric-card", HjMetricCard);
define("hj-chip", HjChip);
define("hj-badge", HjBadge);
define("hj-field", HjField);
define("hj-empty-state", HjEmptyState);
define("hj-panel", HjPanel);
