import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";
import { recordDiagnostic } from "../diagnostics.js";
import { VirtualList } from "../virtuallist.js";

export function initJournal() {
  const listEl = document.getElementById("journal-entry-list");
  const editor = document.getElementById("journal-editor");
  const titleInput = document.getElementById("journal-title");
  const idInput = document.getElementById("current-entry-id");
  const statusEl = document.getElementById("save-status");
  const wordCountEl = document.getElementById("word-count");
  const btnNew = document.getElementById("btn-new-entry");
  const btnDelete = document.getElementById("btn-delete-entry");

  if (!editor || !titleInput || !idInput || !statusEl || !listEl) {
    console.error("Journal UI elements missing.");
    return;
  }

  // State for Keyset Pagination (D-104)
  let entries = [];
  let lastCursor = null;
  let isLoading = false;
  let hasMore = true;
  const PAGE_SIZE = 30;

  // State for Autosave
  let saveTimeout = null;
  let isSaving = false;
  let pendingSave = false;
  let lastSaved = { id: "", title: "", content: "" };

  const virtualList = new VirtualList(listEl, {
    rowHeight: 64,
    renderItem: (container, entry, index) => {
      const isActive = idInput.value === entry.id;
      const title = entry.title || "Untitled Entry";
      const date = new Date(entry.created_at).toLocaleDateString(undefined, {
        month: "short",
        day: "numeric",
      });
      const titleClass = entry.title ? "entry-row-title" : "entry-row-title untitled";

      container.innerHTML = `
        <button type="button" class="entry-row ${isActive ? "active" : ""}" data-id="${escapeHtml(entry.id)}">
          <span class="${titleClass}">${escapeHtml(title)}</span>
          <span class="entry-row-meta">
            <span>${escapeHtml(date)}</span>
            <span>${escapeHtml(entry.word_count)} words</span>
          </span>
        </button>
      `;

      const item = container.querySelector('.entry-row');
      item.onclick = () => loadEntry(entry.id);
    },
    onLoadMore: () => loadMore()
  });

  // ── REFRESH / LOAD LIST ──────────────────────────
  async function refreshList() {
    entries = [];
    lastCursor = null;
    hasMore = true;
    // We don't clear listEl.innerHTML here because VirtualList manages it
    await loadMore();
  }

  async function loadMore() {
    if (isLoading || !hasMore) return;
    
    isLoading = true;
    try {
      const result = await invoke("journal_list", { 
        cursor: lastCursor, 
        limit: PAGE_SIZE 
      });
      recordDiagnostic("journal:list_loaded", {
        count: result.length,
        cursor: lastCursor,
        has_more: result.length >= PAGE_SIZE,
      });

      if (result.length < PAGE_SIZE) {
        hasMore = false;
      }

      if (result.length > 0) {
        const last = result[result.length - 1];
        lastCursor = JSON.stringify({ ts: last.created_at, id: last.id });
        entries = [...entries, ...result.filter(r => !entries.some(e => e.id === r.id))];
        virtualList.setData(entries);
      } else if (entries.length === 0) {
        virtualList.setData([]);
      }
    } catch (err) {
      console.error("Failed to load journal list:", err);
    } finally {
      isLoading = false;
    }
  }

  // Remove appendToList as it's now handled by VirtualList

  window.loadJournalEntry = loadEntry;

  // ── LOAD ENTRY ──────────────────────────────────
  async function loadEntry(id) {
    if (isSaving) {
      setTimeout(() => loadEntry(id), 200);
      return;
    }

    try {
      const entry = await invoke("journal_get", { id });
      if (entry) {
        recordDiagnostic("journal:entry_loaded", {
          id,
          word_count: entry.word_count,
        });
        idInput.value = entry.id;
        titleInput.value = entry.title || "";
        editor.value = entry.content || "";
        updateWordCount();
        lastSaved = { id: entry.id, title: entry.title || "", content: entry.content || "" };
        statusEl.textContent = "Loaded";
        
        // Highlight in list
        listEl.querySelectorAll(".entry-row").forEach(i => i.classList.remove("active"));
        listEl.querySelector(`[data-id="${id}"]`)?.classList.add("active");
      }
    } catch (err) {
      console.error("Failed to load entry:", err);
    }
  }

  // ── NEW ENTRY ───────────────────────────────────
  function createNewEntry() {
    if (isSaving) return;
    idInput.value = "";
    titleInput.value = "";
    editor.value = "";
    updateWordCount();
    lastSaved = { id: "", title: "", content: "" };
    statusEl.textContent = "Ready";
    listEl.querySelectorAll(".entry-row").forEach(i => i.classList.remove("active"));
    editor.focus();
  }

  // ── DELETE ENTRY ─────────────────────────────────
  async function deleteEntry() {
    const id = idInput.value;
    if (!id) return;

    if (!confirm("Are you sure you want to move this entry to trash?")) return;

    try {
      const success = await invoke("journal_delete", { id });
      if (success) {
        recordDiagnostic("journal:entry_deleted", { id });
        createNewEntry();
        refreshList();
      }
    } catch (err) {
      console.error("Delete failed:", err);
    }
  }

  // ── AUTOSAVE ────────────────────────────────────
  function getDraft() {
    return {
      id: idInput.value ?? "",
      title: titleInput.value ?? "",
      content: editor.value ?? "",
    };
  }

  async function save() {
    if (isSaving) {
      pendingSave = true;
      return;
    }

    const { id, title, content } = getDraft();
    const isBlank = !content.trim() && !title.trim();

    if (!id && isBlank) {
      statusEl.textContent = "Ready";
      return;
    }

    if (id === lastSaved.id && title === lastSaved.title && content === lastSaved.content) {
      statusEl.textContent = "Saved";
      return;
    }

    statusEl.textContent = "Saving...";
    isSaving = true;
    pendingSave = false;

    try {
      const isNew = !id;
      const newId = await invoke("save_journal_entry", { id, title, content });
      idInput.value = newId;
      lastSaved = { id: newId, title, content };
      recordDiagnostic("journal:saved", {
        id: newId,
        is_new: isNew,
        title_present: Boolean(title.trim()),
        word_count: content.trim() ? content.trim().split(/\s+/).length : 0,
      });
      statusEl.textContent = "Saved at " + new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      
      if (isNew) {
        // Re-init the list to show the newest at top
        await refreshList();
      } else {
        // Update the item in the entries and virtualList
        const index = entries.findIndex(e => e.id === newId);
        if (index !== -1) {
          entries[index].title = title;
          entries[index].word_count = content.trim() ? content.split(/\s+/).length : 0;
          virtualList.setData(entries);
        }
      }
    } catch (err) {
      console.error("Save Error:", err);
      statusEl.textContent = "Save error";
    } finally {
      isSaving = false;
      if (pendingSave) {
        pendingSave = false;
        await save();
      }
    }
  }

  function updateWordCount() {
    const text = editor.value.trim();
    const count = text ? text.split(/\s+/).length : 0;
    wordCountEl.textContent = `${count} words`;
  }

  let lastEmitTime = 0;

  const triggerAutoSave = () => {
    statusEl.textContent = "Typing...";
    updateWordCount();
    
    // Step 4: Frontend Throttle - 500ms
    const now = Date.now();
    if (now - lastEmitTime > 500) {
      const id = idInput.value;
      if (id) {
        // D-106: Fires immediately
        recordDiagnostic("journal:analysis_queue_emit", { id });
        globalThis.__TAURI__.event.emit('journal_analysis_queued', { id });
        lastEmitTime = now;
      }
    }

    clearTimeout(saveTimeout);
    saveTimeout = setTimeout(save, 1500);
  };

  // ── SCROLL TO LOAD (VirtualList manages scroll) ────────────────
  // listEl.addEventListener("scroll", () => {
  //   const { scrollTop, scrollHeight, clientHeight } = listEl;
  //   if (scrollTop + clientHeight >= scrollHeight - 20) {
  //     loadMore();
  //   }
  // });

  // ── INIT ────────────────────────────────────────
  const entryMenuBtn = document.getElementById("btn-entry-menu");
  const entryMenu = document.getElementById("entry-menu");

  function closeEntryMenu() {
    entryMenu?.classList.add("hidden");
    entryMenuBtn?.setAttribute("aria-expanded", "false");
  }

  entryMenuBtn?.addEventListener("click", (event) => {
    event.stopPropagation();
    const opening = entryMenu?.classList.contains("hidden");
    entryMenu?.classList.toggle("hidden", !opening);
    entryMenuBtn.setAttribute("aria-expanded", String(Boolean(opening)));
  });

  document.addEventListener("click", (event) => {
    if (!entryMenu || entryMenu.classList.contains("hidden")) return;
    if (entryMenu.contains(event.target) || entryMenuBtn?.contains(event.target)) return;
    closeEntryMenu();
  });

  editor.addEventListener("input", triggerAutoSave);
  titleInput.addEventListener("input", triggerAutoSave);
  btnNew?.addEventListener("click", createNewEntry);
  btnDelete?.addEventListener("click", () => {
    closeEntryMenu();
    deleteEntry();
  });

  const btnAnalyze = document.getElementById("btn-analyze");
  btnAnalyze?.addEventListener("click", async () => {
    clearTimeout(saveTimeout); // don't let a pending autosave race the emit
    await save();
    const id = idInput.value;
    if (!id) return; // blank/new entry with nothing to analyze
    recordDiagnostic("journal:analysis_manual", { id, manual: true });
    globalThis.__TAURI__.event.emit("journal_analysis_queued", { id });
  });

  // Startup fetch runs before unlock, when journal_list would fail with
  // VaultLocked — gate on vault status and refetch on vault:unlocked instead.
  invoke("vault_status", {}, { silent: true })
    .then((s) => {
      if (s.unlocked) refreshList();
    })
    .catch(() => {});
  document.addEventListener("vault:unlocked", () => refreshList());
}
