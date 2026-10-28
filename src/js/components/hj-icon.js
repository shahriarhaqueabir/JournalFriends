// <hj-icon> — light-DOM animated icon element.
// Loads a vendored animated Lucide SVG (subset under
// assets/libs/animated-icons/lucide/), renders it inline, and forwards the
// two-tone CSS custom properties. Falls back to a static Lucide `<i>` only if
// the vendored file is unavailable. No Shadow DOM (design guard + CSP friendly).
//
//   <hj-icon name="list-checks" size="20" label="Tasks"></hj-icon>
//
// Attributes: name (kebab-case file), size (px, default 20), label (a11y),
// tone (adds a color context class), animate ("always" to animate without a host).

import { ANIMATED_ICONS } from "./animated-icons.data.js";

const ICON_BASE = "/assets/libs/animated-icons/lucide/";
const cache = new Map();

async function loadIcon(name) {
  if (cache.has(name)) return cache.get(name);
  // Inlined markup is preferred (no fetch, CSP/origin-proof); fetch is a
  // fallback for names not present in the generated bundle.
  const p = ANIMATED_ICONS[name]
    ? Promise.resolve(ANIMATED_ICONS[name])
    : fetch(`${ICON_BASE}${name}.svg`)
        .then((r) => (r.ok ? r.text() : null))
        .catch(() => null);
  cache.set(name, p);
  return p;
}

const SIZES = { 16: 16, 18: 18, 20: 20, 24: 24 };

export class HjIcon extends HTMLElement {
  static get observedAttributes() {
    return ["name", "size", "label", "tone"];
  }

  constructor() {
    super();
    this._token = 0;
  }

  connectedCallback() {
    this._render();
  }

  attributeChangedCallback() {
    if (this.isConnected) this._render();
  }

  async _render() {
    const name = this.getAttribute("name");
    if (!name) return;

    const size = SIZES[Number(this.getAttribute("size"))] || 20;
    const label = this.getAttribute("label");
    const token = ++this._token;

    this.style.setProperty("--icon-size", `${size}px`);
    this.setAttribute("data-icon", name);
    if (this.getAttribute("tone")) this.setAttribute("data-tone", this.getAttribute("tone"));

    const svg = await loadIcon(name);
    if (token !== this._token) return; // a newer render superseded this one

    if (svg) {
      // Vendored SVG is same-origin static markup authored by us — safe.
      this.innerHTML = svg;
      const el = this.firstElementChild;
      if (el) {
        el.setAttribute("width", String(size));
        el.setAttribute("height", String(size));
        el.setAttribute("aria-hidden", label ? "false" : "true");
        if (label) {
          el.setAttribute("role", "img");
          el.setAttribute("aria-label", label);
        }
      }
      this.classList.add("hj-icon--svg");
    } else {
      // Graceful fallback to static Lucide (still registered globally).
      this.classList.remove("hj-icon--svg");
      this.innerHTML = `<i data-lucide="${name}"></i>`;
      if (window.lucide?.createIcons) {
        try {
          window.lucide.createIcons({ icons: window.lucide.icons });
        } catch {
          /* ignore */
        }
      }
    }
  }
}

if (!customElements.get("hj-icon")) {
  customElements.define("hj-icon", HjIcon);
}
