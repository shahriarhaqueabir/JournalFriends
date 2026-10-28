const MAX_EVENTS = 1000;

const state = {
  enabled: true,
  events: [],
};

function sanitize(value) {
  if (value == null) return value;
  if (typeof value === "string") {
    return value.length > 240 ? `${value.slice(0, 240)}...` : value;
  }
  if (Array.isArray(value)) return value.map(sanitize);
  if (typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => {
        if (
          /(content|message|feedback|text|prompt|title|summary|task|fact|insight)/i.test(
            key,
          ) &&
          typeof item === "string"
        ) {
          return [key, `[text:${item.length} chars]`];
        }
        return [key, sanitize(item)];
      }),
    );
  }
  return value;
}

export function recordDiagnostic(type, detail = {}) {
  if (!state.enabled) return;

  const event = {
    at: new Date().toISOString(),
    type,
    detail: sanitize(detail),
  };

  state.events.push(event);
  if (state.events.length > MAX_EVENTS) {
    state.events.splice(0, state.events.length - MAX_EVENTS);
  }

  console.debug("[ArnoldDiag]", event.type, event.detail);
}

export function getDiagnosticsSnapshot() {
  return [...state.events];
}

globalThis.__ARNOLD_DIAGNOSTICS__ = {
  record: recordDiagnostic,
  snapshot: getDiagnosticsSnapshot,
  clear() {
    state.events.length = 0;
  },
  enable() {
    state.enabled = true;
  },
  disable() {
    state.enabled = false;
  },
};
