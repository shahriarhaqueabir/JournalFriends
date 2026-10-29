// One-off generator: emits self-contained animated Lucide SVGs.
// Each SVG carries its own <style> with hover-triggered, two-tone, CSS-only
// animation that honours prefers-reduced-motion. No JS, no network (CSP-safe).
// Run: node tools/js-checks/gen-animated-icons.mjs
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const OUT = join(process.cwd(), "src/assets/libs/animated-icons/lucide");
mkdirSync(OUT, { recursive: true });

// name -> { paths: [d...], anim: category }
const ICONS = {
  "arrow-up": { paths: ["M12 19V5", "m5 12 7-7 7 7"], anim: "rise" },
  "bar-chart-3": { paths: ["M3 3v16a2 2 0 0 0 2 2h16", "M7 16v-4", "M12 16V8", "M17 16v-8"], anim: "grow" },
  "book-open": { paths: ["M12 7v14", "M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"], anim: "pulse" },
  "bot": { paths: ["M12 8V4H8", "M12 8h4V4h-4", "M8 8h8a4 4 0 0 1 4 4v6a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4v-6a4 4 0 0 1 4-4z", "M9 14h.01", "M15 14h.01"], anim: "bounce" },
  "calendar": { paths: ["M8 2v4", "M16 2v4", "M3 10h18", "M5 4h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z"], anim: "pulse" },
  "check": { paths: ["M20 6 9 17l-5-5"], anim: "draw" },
  "chevron-left": { paths: ["m15 18-6-6 6-6"], anim: "nudge-left" },
  "chevron-right": { paths: ["m9 18 6-6-6-6"], anim: "nudge-right" },
  "circle-alert": { paths: ["M12 8v4", "M12 16h.01", "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z"], anim: "shake" },
  "circle-check": { paths: ["M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z", "m9 12 2 2 4-4"], anim: "draw" },
  "circle-x": { paths: ["M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z", "m15 9-6 6", "m9 9 6 6"], anim: "shake" },
  "copy": { paths: ["M12 2H8a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V6z", "M16 8h2a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2v-2"], anim: "nudge-right" },
  "cpu": { paths: ["M6 6h12v12H6z", "M9 2v2", "M15 2v2", "M9 20v2", "M15 20v2", "M2 9h2", "M2 15h2", "M20 9h2", "M20 15h2"], anim: "spin" },
  "external-link": { paths: ["M15 3h6v6", "M10 14 21 3", "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"], anim: "nudge-right" },
  "eye": { paths: ["M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z", "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"], anim: "pulse" },
  "file-text": { paths: ["M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z", "M14 2v4a2 2 0 0 0 2 2h4", "M10 9H8", "M16 13H8", "M16 17H8"], anim: "pulse" },
  "flame": { paths: ["M12 2s4 4 4 8a4 4 0 0 1-8 0c0-1 .5-2 1-3-2 1.5-5 4-5 8a8 8 0 0 0 16 0c0-5-8-13-8-13z"], anim: "flame" },
  "git-branch": { paths: ["M6 3v12", "M18 6a3 3 0 1 0 0-6 3 3 0 0 0 0 6z", "M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6z", "M18 9a9 9 0 0 1-9 9"], anim: "pulse" },
  "heart-pulse": { paths: ["M19 14c1.5-1.5 3-3.3 3-5.5A4.5 4.5 0 0 0 12 5 4.5 4.5 0 0 0 2 8.5c0 2.2 1.5 4 3 5.5l7 7z", "M3.2 12H9l1.5-3 3 6 1.5-3h5.8"], anim: "beat" },
  "inbox": { paths: ["M22 12h-6l-2 3h-4l-2-3H2", "M5.5 5.5 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.5-6.5A2 2 0 0 0 16.8 4H7.2a2 2 0 0 0-1.7 1.5z"], anim: "bounce" },
  "list-checks": { paths: ["M3 6h11", "M3 12h11", "M3 18h11", "m18 6 2 2 3-3", "m18 12 2 2 3-3", "m18 18 2 2 3-3"], anim: "draw" },
  "lock": { paths: ["M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z", "M7 11V7a5 5 0 0 1 10 0v4"], anim: "bounce" },
  "message-circle": { paths: ["M7.9 20A9 9 0 1 0 4 16.1L2 22z"], anim: "bounce" },
  "more-horizontal": { paths: ["M12 12h.01", "M19 12h.01", "M5 12h.01"], anim: "bounce" },
  "panel-right-close": { paths: ["M3 3h18v18H3z", "M15 3v18", "m8 9 3 3-3 3"], anim: "nudge-left" },
  "search": { paths: ["M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z", "m21 21-4.3-4.3"], anim: "pulse" },
  "server": { paths: ["M4 3h16a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z", "M4 15h16a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1z", "M7 6h.01", "M7 18h.01"], anim: "pulse" },
  "settings": { paths: ["M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z", "M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-2.7 1.1V21a2 2 0 1 1-4 0v-.1A1.6 1.6 0 0 0 7 19.4l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0-1.1-2.7H3a2 2 0 1 1 0-4h.1A1.6 1.6 0 0 0 4.6 7l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 2.7 1.1l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z"], anim: "spin" },
  "shield-check": { paths: ["M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z", "m9 12 2 2 4-4"], anim: "draw" },
  "smile": { paths: ["M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z", "M8 14s1.5 2 4 2 4-2 4-2", "M9 9h.01", "M15 9h.01"], anim: "bounce" },
  "sparkles": { paths: ["m12 3 1.9 5.8a2 2 0 0 0 1.3 1.3L21 12l-5.8 1.9a2 2 0 0 0-1.3 1.3L12 21l-1.9-5.8a2 2 0 0 0-1.3-1.3L3 12l5.8-1.9a2 2 0 0 0 1.3-1.3z", "M5 3v4", "M3 5h4", "M19 17v4", "M17 19h4"], anim: "twinkle" },
  "sun": { paths: ["M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8z", "M12 2v2", "M12 20v2", "m4.9 4.9 1.4 1.4", "m17.7 17.7 1.4 1.4", "M2 12h2", "M20 12h2", "m4.9 19.1 1.4-1.4", "m17.7 6.3 1.4-1.4"], anim: "spin" },
  "trash-2": { paths: ["M3 6h18", "M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2", "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6", "M10 11v6", "M14 11v6"], anim: "shake" },
  "x": { paths: ["M18 6 6 18", "m6 6 12 12"], anim: "spin" },
};

// Animation library — transform on the whole <g>, keyed by category.
const ANIM = {
  pulse: ".hjai{transition:transform .18s cubic-bezier(.16,1,.3,1)}.hjai-host:hover .hjai{transform:scale(1.12)}",
  bounce: ".hjai{transition:transform .18s cubic-bezier(.34,1.56,.64,1)}.hjai-host:hover .hjai{transform:translateY(-2px)}",
  grow: ".hjai{transition:transform .18s cubic-bezier(.16,1,.3,1);transform-origin:bottom}.hjai-host:hover .hjai{transform:scaleY(1.08)}",
  rise: ".hjai{transition:transform .18s cubic-bezier(.16,1,.3,1)}.hjai-host:hover .hjai{transform:translateY(-3px)}",
  "nudge-left": ".hjai{transition:transform .18s cubic-bezier(.16,1,.3,1)}.hjai-host:hover .hjai{transform:translateX(-2px)}",
  "nudge-right": ".hjai{transition:transform .18s cubic-bezier(.16,1,.3,1)}.hjai-host:hover .hjai{transform:translateX(2px)}",
  shake: ".hjai{transition:transform .18s ease-out}.hjai-host:hover .hjai{animation:hjai-shake .4s ease-in-out}@keyframes hjai-shake{0%,100%{transform:translateX(0)}25%{transform:translateX(-2px)}75%{transform:translateX(2px)}}",
  spin: ".hjai{transition:transform .24s cubic-bezier(.16,1,.3,1)}.hjai-host:hover .hjai{transform:rotate(18deg)}",
  flame: ".hjai{transition:transform .18s cubic-bezier(.16,1,.3,1)}.hjai-host:hover .hjai{transform:scale(1.14) rotate(-4deg)}",
  beat: ".hjai{transition:transform .18s ease-out}.hjai-host:hover .hjai{animation:hjai-beat .5s ease-in-out}@keyframes hjai-beat{0%,100%{transform:scale(1)}30%{transform:scale(1.16)}50%{transform:scale(0.98)}}",
  twinkle: ".hjai{transition:transform .24s cubic-bezier(.34,1.56,.64,1)}.hjai-host:hover .hjai{transform:rotate(-8deg) scale(1.12)}",
  draw: ".hjai{transition:transform .18s cubic-bezier(.16,1,.3,1)}.hjai-host:hover .hjai{transform:scale(1.1)}",
};

const svg = (name, { paths, anim }) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" role="img" aria-hidden="true" focusable="false">
<title>${name}</title>
<style>
.hjai{color:var(--icon-primary,currentColor)}
.hjai-p{stroke:var(--icon-primary,currentColor)}
.hjai-a{stroke:var(--icon-secondary,var(--color-accent,#4FD1C5))}
${ANIM[anim]}
@media (prefers-reduced-motion: reduce){.hjai,.hjai-host:hover .hjai{transition:none;animation:none}}
</style>
<g class="hjai">
${paths.map((d, i) => `<path class="${i === 0 ? "hjai-p" : "hjai-a"}" d="${d}"/>`).join("\n")}
</g>
</svg>
`;

let n = 0;
for (const [name, def] of Object.entries(ICONS)) {
  writeFileSync(join(OUT, `${name}.svg`), svg(name, def));
  n++;
}
console.log(`wrote ${n} animated icons to ${OUT}`);
