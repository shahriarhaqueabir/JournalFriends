// Node harness for the light-DOM custom elements (no browser).
// Provides a minimal DOM/customElements shim, imports the component module,
// and asserts the rendering contracts that the surfaces depend on.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// ── Minimal DOM shim ─────────────────────────────────────────
class El {
  constructor(tag = "div") {
    this.tagName = tag.toUpperCase();
    this._attrs = new Map();
    this._children = [];
    this._listeners = {};
    this.textContent = "";
    const clsSet = new Set();
    this.classList = {
      _s: clsSet,
      add: (...c) => c.forEach((x) => clsSet.add(x)),
      remove: (...c) => c.forEach((x) => clsSet.delete(x)),
      contains: (c) => clsSet.has(c),
      toggle: (c, on) => (on === undefined ? (clsSet.has(c) ? clsSet.delete(c) : clsSet.add(c)) : on ? clsSet.add(c) : clsSet.delete(c)),
    };
    Object.defineProperty(this, "className", {
      get: () => [...clsSet].join(" "),
      set: (v) => { clsSet.clear(); String(v).split(/\s+/).filter(Boolean).forEach((x) => clsSet.add(x)); },
    });
    this.style = { setProperty() {}, };
    this.dataset = {};
  }
  setAttribute(k, v) {
    const had = this._attrs.has(k);
    this._attrs.set(k, String(v));
    if (!had && typeof this.attributeChangedCallback === "function" && this.constructor.observedAttributes?.includes(k)) {
      this.attributeChangedCallback(k, null, String(v));
    }
  }
  getAttribute(k) { return this._attrs.has(k) ? this._attrs.get(k) : null; }
  hasAttribute(k) { return this._attrs.has(k); }
  removeAttribute(k) {
    const had = this._attrs.has(k);
    this._attrs.delete(k);
    if (had && typeof this.attributeChangedCallback === "function" && this.constructor.observedAttributes?.includes(k)) {
      this.attributeChangedCallback(k, null, null);
    }
  }
  appendChild(c) { this._children.push(c); c.parentNode = this; return c; }
  append(...c) { c.forEach((x) => this.appendChild(x)); }
  prepend(...c) { this._children.unshift(...c); c.forEach((x) => (x.parentNode = this)); }
  replaceChildren(...c) { this._children = []; this.append(...c); }
  remove() { if (this.parentNode) this.parentNode._children = this.parentNode._children.filter((x) => x !== this); }
  after(sib) { if (this.parentNode) { const i = this.parentNode._children.indexOf(this); this.parentNode._children.splice(i + 1, 0, sib); } }
  addEventListener(t, fn) { this._listeners[t] = fn; }
  get children() { return this._children; }
  get firstElementChild() { return this._children.find((c) => c instanceof El) || null; }
  // Extremely small selector support: by class, by tag, :scope > .
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
  querySelectorAll(sel) {
    const out = [];
    const walk = (node) => {
      for (const c of node._children) {
        if (!(c instanceof El)) continue;
        const bare = sel.replace(":scope > ", "").trim();
        let match = false;
        if (bare.startsWith(".")) match = c.classList.contains(bare.slice(1));
        else if (/^[a-z][a-z0-9-]*$/i.test(bare)) match = c.tagName === bare.toUpperCase();
        if (match) out.push(c);
        walk(c);
      }
    };
    walk(this);
    return out;
  }
  click() { this._listeners.click?.({ target: this }); }
  getBoundingClientRect() { return { width: 0 }; }
  get isConnected() { return true; }
}

const registry = new Map();
globalThis.HTMLElement = El;
globalThis.customElements = {
  get: (n) => registry.get(n),
  define: (n, c) => registry.set(n, class extends c {}),
};
globalThis.document = {
  createElement: (t) => new El(t),
  createTextNode: (t) => ({ nodeType: 3, textContent: t }),
};

// ── Import the module under test ─────────────────────────────
const src = readFileSync(join(process.cwd(), "src/js/components/elements.js"), "utf8");
const mod = await import("data:text/javascript;base64," + Buffer.from(src).toString("base64"));
const { renderErrorStatus } = await import("../../src/js/ai-sidebar.js");

const { HjButton, HjMetricCard, HjField } = mod;

// ── Assertions ───────────────────────────────────────────────
// hj-button: role, tabindex, aria-disabled, disabled property round-trip.
{
  const b = new HjButton();
  b.connectedCallback();
  assert.equal(b.getAttribute("role"), "button", "button role");
  assert.equal(b.tabIndex, 0, "button tabindex 0 when enabled");
  b.disabled = true;
  assert.equal(b.hasAttribute("disabled"), true, "disabled attr set via property");
  assert.equal(b.getAttribute("aria-disabled"), "true", "aria-disabled when disabled");
  assert.equal(b.disabled, true, "disabled getter");
  b.disabled = false;
  assert.equal(b.hasAttribute("disabled"), false, "disabled attr cleared");
  // icon attribute prepends an hj-icon
  const b2 = new HjButton();
  b2.setAttribute("icon", "check");
  b2.connectedCallback();
  assert.ok(b2.querySelector("hj-icon"), "icon prepended");
}

// hj-metric-card: builds value + label spans.
{
  const m = new HjMetricCard();
  m.setAttribute("label", "Total Words");
  m.setAttribute("value", "1234");
  m.setAttribute("icon", "file-text");
  m.connectedCallback();
  const val = m.querySelector(".metric-value");
  const lbl = m.querySelector(".metric-label");
  assert.equal(val?.textContent, "1234", "metric value rendered");
  assert.equal(lbl?.textContent, "Total Words", "metric label rendered");
  m.setValue("9999");
  assert.equal(m.querySelector(".metric-value").textContent, "9999", "setValue updates");
}

// hj-field: builds label + help + error.
{
  const f = new HjField();
  f.setAttribute("label", "Title");
  f.setAttribute("help", "Optional");
  f.setAttribute("error", "Required");
  f.connectedCallback();
  assert.equal(f.querySelector("label")?.textContent, "Title", "field label");
  assert.equal(f.querySelector(".hj-field-help")?.textContent, "Optional", "field help");
  assert.equal(f.querySelector(".hj-field-error")?.textContent, "Required", "field error");
  f.removeAttribute("error");
  f.attributeChangedCallback("error");
  assert.equal(f.querySelector(".hj-field-error"), null, "field error removed");
}

// AI errors are rendered as text nodes, never parsed as author/model HTML.
{
  const container = new El("div");
  const untrusted = '<img src=x onerror="alert(1)">';
  const retry = renderErrorStatus(container, untrusted, "Model Not Found");
  const [bubble] = container.children;
  const [heading, lineBreak, message, actions] = bubble.children;

  assert.equal(heading.textContent, "Model Not Found", "error heading");
  assert.equal(lineBreak.tagName, "BR", "line break remains structural markup");
  assert.equal(message.tagName, "SPAN", "untrusted string remains a text node container");
  assert.equal(message.textContent, untrusted, "untrusted message is preserved literally");
  assert.equal(actions.children[0], retry, "retry action remains available");
}

// All expected elements registered.
for (const name of ["hj-button", "hj-card", "hj-metric-card", "hj-chip", "hj-badge", "hj-field", "hj-empty-state", "hj-panel"]) {
  assert.ok(customElements.get(name), `${name} defined`);
}

console.log("components: all assertions passed (" + registry.size + " elements)");
