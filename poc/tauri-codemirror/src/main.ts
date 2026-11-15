/**
 * Mica Tauri + CodeMirror 6 PoC — frontend application shell.
 *
 * Plain TypeScript, no framework. The frontend owns only presentation:
 * the file tree pane, the tab bar, the CodeMirror editor, the command
 * palette / Quick Open overlay, and the status bar.
 *
 * Everything about files lives in the Rust backend (`src-tauri/`), which
 * delegates to `mica-shared-core` for the file tree, atomic saves,
 * keymap, command-palette fuzzy matching and the WSL bridge stub. The
 * frontend never touches the filesystem directly; it only calls Tauri
 * commands (`invoke`).
 *
 * Tabs: the backend mirrors open/close into shared-core's `TabManager`
 * (`open_tab` / `close_tab`), but the tab list the UI renders — order,
 * active tab, buffer content, dirty flag — is the frontend's own
 * `tabs` array below. The backend's copy is not read back.
 *
 * Auto-save: edits mark the active tab dirty and restart a 1000 ms
 * debounce timer (Notion §7 `auto_save_delay_ms = 1000`). When the
 * timer fires, the tab is saved through the backend's atomic save and
 * the status bar shows Saving → Saved → Clean. An edit made while the
 * write is in flight keeps the tab dirty (src/auto-save.ts), and saves
 * are queued so they land on disk in order.
 *
 * Keymap: loaded once from the backend (`load_keymap`), which reads
 * `~/.config/mica/keymap.toml` and falls back to the shared-core
 * defaults. Area bindings win over global ones, mirroring
 * `mica-shared-core`'s `Keymap::command_for_key`.
 */

import { invoke } from "@tauri-apps/api/core";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { EditorState } from "@codemirror/state";
import {
  EditorView,
  keymap as editorKeymap,
  lineNumbers,
} from "@codemirror/view";
import {
  livePreview,
  toggleTaskCheckboxAt,
} from "./live-preview";
import { saveTabContent } from "./auto-save";
import "./style.css";

/* ------------------------------------------------------------------ */
/* Types matching the Rust backend's serialised commands.             */
/* ------------------------------------------------------------------ */

interface FileTreeEntry {
  path: string;
  name: string;
  is_folder: boolean;
  depth: number;
  is_markdown: boolean;
}

interface PaletteEntry {
  title: string;
  detail: string;
  action: string;
}

interface KeyBinding {
  key: string;
  command: string;
  area: string;
}

interface KeymapInfo {
  /** "file" when ~/.config/mica/keymap.toml was read, else "defaults". */
  source: string;
  /** Human-readable location shown in the status bar. */
  location: string;
  bindings: KeyBinding[];
}

interface OpenFolderResult {
  path: string;
  entries: FileTreeEntry[];
}

interface EditorTab {
  path: string;
  title: string;
  /** Current buffer content (raw Markdown). */
  content: string;
  dirty: boolean;
}

type SaveState = "clean" | "dirty" | "saving" | "saved";
type PaletteMode = "command" | "file" | "search";

/* ------------------------------------------------------------------ */
/* Small DOM helpers.                                                 */
/* ------------------------------------------------------------------ */

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (found === null) {
    throw new Error(`Missing element #${id}`);
  }
  return found as T;
}

function fileNameOf(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}

/* ------------------------------------------------------------------ */
/* Application state.                                                 */
/* ------------------------------------------------------------------ */

const AUTO_SAVE_DELAY_MS = 1000;

let currentFolder: string | null = null;
let treeEntries: FileTreeEntry[] = [];
let collapsedFolders = new Set<string>();
let treeKeyboardFocus = 0;
let tabs: EditorTab[] = [];
let activeTabIndex: number | null = null;
let saveState: SaveState = "clean";
let saveTimer: ReturnType<typeof setTimeout> | undefined;
let savedFlashTimer: ReturnType<typeof setTimeout> | undefined;
/** While a tab's content is being swapped in, ignore doc changes. */
let swappingTab = false;
/** Keymap lookup: "area: normalised key" -> command name. */
const keyBindings = new Map<string, string>();

let paletteMode: PaletteMode = "command";
let paletteEntries: PaletteEntry[] = [];
let paletteSelected = 0;
let paletteFetchTimer: ReturnType<typeof setTimeout> | undefined;

const folderInput = element<HTMLInputElement>("folder-input");
const openFolderButton = element<HTMLButtonElement>("open-folder-button");
const toggleTreeButton = element<HTMLButtonElement>("toggle-tree-button");
const saveStateLabel = element<HTMLSpanElement>("save-state");
const fileTreePane = element<HTMLElement>("file-tree-pane");
const fileTreeElement = element<HTMLDivElement>("file-tree");
const tabBarElement = element<HTMLDivElement>("tab-bar");
const editorHost = element<HTMLDivElement>("editor-host");
const emptyState = element<HTMLDivElement>("empty-state");
const statusFile = element<HTMLSpanElement>("status-file");
const statusCursor = element<HTMLSpanElement>("status-cursor");
const statusKeymap = element<HTMLSpanElement>("status-keymap");
const paletteOverlay = element<HTMLDivElement>("palette-overlay");
const paletteInput = element<HTMLInputElement>("palette-input");
const paletteList = element<HTMLUListElement>("palette-list");

/* ------------------------------------------------------------------ */
/* Save-state display.                                                */
/* ------------------------------------------------------------------ */

function renderSaveState(): void {
  const labels: Record<SaveState, string> = {
    clean: "Clean",
    dirty: "● Unsaved",
    saving: "Saving…",
    saved: "Saved",
  };
  saveStateLabel.textContent = labels[saveState];
  saveStateLabel.dataset.state = saveState;
}

function setSaveState(next: SaveState): void {
  saveState = next;
  if (savedFlashTimer !== undefined) {
    clearTimeout(savedFlashTimer);
    savedFlashTimer = undefined;
  }
  if (next === "saved") {
    savedFlashTimer = setTimeout(() => setSaveState("clean"), 1500);
  }
  renderSaveState();
  renderTabBar();
}

/* ------------------------------------------------------------------ */
/* CodeMirror editor.                                                 */
/* ------------------------------------------------------------------ */

const editorView = new EditorView({
  parent: editorHost,
  state: EditorState.create({
    doc: "",
    extensions: [
      lineNumbers(),
      history(),
      editorKeymap.of([...defaultKeymap, ...historyKeymap]),
      EditorView.lineWrapping,
      livePreview,
      EditorView.updateListener.of((update) => {
        if (update.docChanged && !swappingTab) {
          onEditorEdited();
        }
        if (update.selectionSet) {
          renderCursorStatus();
        }
      }),
    ],
  }),
});

// Clicking a rendered checkbox toggles the task in the raw Markdown.
editorView.dom.addEventListener("mousedown", (event) => {
  const target = event.target as HTMLElement | null;
  const checkbox = target?.closest(".cm-mica-checkbox") as HTMLElement | null;
  if (checkbox === null || checkbox === undefined) {
    return;
  }
  const markerFrom = Number(checkbox.dataset.markerFrom ?? "");
  if (Number.isFinite(markerFrom)) {
    event.preventDefault();
    toggleTaskCheckboxAt(editorView, markerFrom);
  }
});

function activeTab(): EditorTab | null {
  return activeTabIndex === null ? null : (tabs[activeTabIndex] ?? null);
}

function onEditorEdited(): void {
  const tab = activeTab();
  if (tab === null) {
    return;
  }
  tab.content = editorView.state.doc.toString();
  tab.dirty = true;
  setSaveState("dirty");
  if (saveTimer !== undefined) {
    clearTimeout(saveTimer);
  }
  saveTimer = setTimeout(() => void saveActiveTab(), AUTO_SAVE_DELAY_MS);
}

/**
 * Saves run one at a time, in order: a save started while another is
 * in flight waits for it, so an older snapshot can never land on disk
 * after a newer one.
 */
let saveQueue: Promise<void> = Promise.resolve();

function saveActiveTab(): Promise<void> {
  const run = saveQueue.then(saveActiveTabNow);
  saveQueue = run.catch(() => undefined);
  return run;
}

async function saveActiveTabNow(): Promise<void> {
  const tab = activeTab();
  if (tab === null || !tab.dirty) {
    return;
  }
  setSaveState("saving");
  // Only clears `dirty` when the buffer did not change during the
  // write (see src/auto-save.ts for the race this prevents).
  const outcome = await saveTabContent(tab, (path, content) =>
    invoke("save_file", { path, content }),
  );
  switch (outcome.kind) {
    case "saved":
      setSaveState("saved");
      renderStatusFile();
      break;
    case "changed":
      // Edited mid-save: the edit already restarted the auto-save
      // timer, which will write the newer text.
      setSaveState("dirty");
      renderStatusFile();
      break;
    case "failed":
      setSaveState("dirty");
      statusFile.textContent = `Save failed: ${String(outcome.error)}`;
      break;
  }
}

/** Flush a pending auto-save immediately (used before tab switches). */
async function flushPendingSave(): Promise<void> {
  if (saveTimer !== undefined) {
    clearTimeout(saveTimer);
    saveTimer = undefined;
  }
  await saveActiveTab();
}

/* ------------------------------------------------------------------ */
/* Tabs: one file = one tab; reopening focuses the existing tab.      */
/* ------------------------------------------------------------------ */

function renderTabBar(): void {
  tabBarElement.innerHTML = "";
  tabs.forEach((tab, index) => {
    const tabElement = document.createElement("div");
    tabElement.className = "tab" + (index === activeTabIndex ? " active" : "");
    tabElement.setAttribute("role", "tab");
    tabElement.title = tab.path;

    const title = document.createElement("span");
    title.textContent = tab.title;
    tabElement.appendChild(title);

    if (tab.dirty) {
      const dirty = document.createElement("span");
      dirty.className = "tab-dirty";
      dirty.textContent = "●";
      dirty.title = "Unsaved changes";
      tabElement.appendChild(dirty);
    }

    const close = document.createElement("button");
    close.className = "tab-close";
    close.type = "button";
    close.textContent = "✕";
    close.title = "Close tab (Ctrl+W)";
    close.addEventListener("click", (event) => {
      event.stopPropagation();
      void closeTab(index);
    });
    tabElement.appendChild(close);

    tabElement.addEventListener("click", () => void focusTab(index));
    tabBarElement.appendChild(tabElement);
  });
}

async function openFile(path: string): Promise<void> {
  const existing = tabs.findIndex((tab) => tab.path === path);
  if (existing >= 0) {
    await focusTab(existing);
    return;
  }
  const content = await invoke<string>("read_file", { path });
  await invoke("open_tab", { path });
  tabs.push({ path, title: fileNameOf(path), content, dirty: false });
  await focusTab(tabs.length - 1);
  setSaveState("clean");
}

async function focusTab(index: number): Promise<void> {
  if (index < 0 || index >= tabs.length) {
    return;
  }
  await flushPendingSave();
  activeTabIndex = index;
  const tab = tabs[index];
  swappingTab = true;
  editorView.dispatch({
    changes: {
      from: 0,
      to: editorView.state.doc.length,
      insert: tab.content,
    },
    selection: { anchor: 0 },
  });
  swappingTab = false;
  editorHost.classList.remove("hidden");
  emptyState.classList.add("hidden");
  setSaveState(tab.dirty ? "dirty" : "clean");
  renderTabBar();
  renderStatusFile();
  renderCursorStatus();
  renderTree();
  editorView.focus();
}

async function closeTab(index: number): Promise<void> {
  if (index < 0 || index >= tabs.length) {
    return;
  }
  if (index === activeTabIndex) {
    await flushPendingSave();
  }
  await invoke("close_tab", { index });
  tabs.splice(index, 1);
  if (tabs.length === 0) {
    activeTabIndex = null;
    editorHost.classList.add("hidden");
    emptyState.classList.remove("hidden");
    setSaveState("clean");
  } else if (activeTabIndex !== null) {
    const next =
      index === activeTabIndex
        ? Math.min(index, tabs.length - 1)
        : activeTabIndex > index
          ? activeTabIndex - 1
          : activeTabIndex;
    activeTabIndex = null;
    await focusTab(next);
  }
  renderTabBar();
  renderStatusFile();
}

/**
 * `tab_recent_next` (Ctrl+Tab). PoC simplification: this cycles by tab
 * *position*, not most-recently-used order as the spec asks. The
 * command name stays the same as in the other PoCs.
 */
function focusNextTab(): void {
  if (tabs.length === 0) {
    return;
  }
  const next =
    activeTabIndex === null ? 0 : (activeTabIndex + 1) % tabs.length;
  void focusTab(next);
}

/* ------------------------------------------------------------------ */
/* File tree.                                                         */
/* ------------------------------------------------------------------ */

function visibleTreeEntries(): FileTreeEntry[] {
  const visible: FileTreeEntry[] = [];
  const hiddenDepths: number[] = [];
  for (const entry of treeEntries) {
    while (
      hiddenDepths.length > 0 &&
      entry.depth <= hiddenDepths[hiddenDepths.length - 1]
    ) {
      hiddenDepths.pop();
    }
    if (hiddenDepths.length > 0) {
      continue;
    }
    visible.push(entry);
    if (entry.is_folder && collapsedFolders.has(entry.path)) {
      hiddenDepths.push(entry.depth);
    }
  }
  return visible;
}

function renderTree(): void {
  const visible = visibleTreeEntries();
  const activePath = activeTab()?.path ?? null;
  fileTreeElement.innerHTML = "";
  visible.forEach((entry, index) => {
    const row = document.createElement("div");
    row.className =
      "file-tree-entry" +
      (entry.is_folder ? " is-folder" : "") +
      (entry.is_markdown ? " is-markdown" : "") +
      (entry.path === activePath ? " selected" : "") +
      (index === treeKeyboardFocus ? " keyboard-focus" : "");
    row.setAttribute("role", "treeitem");
    row.style.paddingLeft = `${8 + entry.depth * 16}px`;
    row.textContent =
      (entry.is_folder
        ? collapsedFolders.has(entry.path)
          ? "▸ "
          : "▾ "
        : "  ") + entry.name;
    row.title = entry.path;
    row.addEventListener("click", () => {
      treeKeyboardFocus = index;
      if (entry.is_folder) {
        toggleFolder(entry.path);
      } else if (entry.is_markdown) {
        void openFile(entry.path);
      }
    });
    fileTreeElement.appendChild(row);
  });
}

function toggleFolder(path: string): void {
  if (collapsedFolders.has(path)) {
    collapsedFolders.delete(path);
  } else {
    collapsedFolders.add(path);
  }
  renderTree();
}

async function openFolder(path: string): Promise<void> {
  const result = await invoke<OpenFolderResult>("open_folder", { path });
  currentFolder = result.path;
  treeEntries = result.entries;
  collapsedFolders = new Set();
  treeKeyboardFocus = 0;
  folderInput.value = result.path;
  renderTree();
  renderStatusFile();
}

/* ------------------------------------------------------------------ */
/* Command palette and Quick Open.                                    */
/* ------------------------------------------------------------------ */

function openPalette(mode: PaletteMode): void {
  paletteMode = mode;
  paletteInput.value = "";
  paletteInput.placeholder =
    mode === "command"
      ? "Type a command name…"
      : mode === "file"
        ? "Type a file name…"
        : "Type text to search for in this folder…";
  paletteOverlay.hidden = false;
  paletteInput.focus();
  void refreshPalette();
}

function closePalette(): void {
  paletteOverlay.hidden = true;
  editorView.focus();
}

async function refreshPalette(): Promise<void> {
  const query = paletteInput.value;
  try {
    if (paletteMode === "search") {
      if (query.trim() === "") {
        paletteEntries = [];
      } else {
        const paths = await invoke<string[]>("search_files", { query });
        paletteEntries = paths.map((path) => ({
          title: fileNameOf(path),
          detail: path,
          action: path,
        }));
      }
    } else {
      paletteEntries = await invoke<PaletteEntry[]>("search_palette", {
        query,
        kind: paletteMode,
      });
    }
  } catch (error) {
    paletteEntries = [
      {
        title: `Palette failed: ${String(error)}`,
        detail: "",
        action: "",
      },
    ];
  }
  paletteSelected = 0;
  renderPalette();
}

function renderPalette(): void {
  paletteList.innerHTML = "";
  paletteEntries.forEach((entry, index) => {
    const item = document.createElement("li");
    item.className = index === paletteSelected ? "selected" : "";
    item.setAttribute("role", "option");
    item.textContent = entry.title;
    if (entry.detail !== "") {
      const detail = document.createElement("span");
      detail.className = "palette-detail";
      detail.textContent = entry.detail;
      item.appendChild(detail);
    }
    item.addEventListener("click", () => {
      paletteSelected = index;
      void runPaletteSelection();
    });
    paletteList.appendChild(item);
  });
}

async function runPaletteSelection(): Promise<void> {
  const entry = paletteEntries[paletteSelected];
  if (entry === undefined || entry.action === "") {
    return;
  }
  closePalette();
  if (paletteMode === "command") {
    await runCommand(entry.action);
  } else {
    await openFile(entry.action);
  }
}

paletteInput.addEventListener("input", () => {
  if (paletteFetchTimer !== undefined) {
    clearTimeout(paletteFetchTimer);
  }
  paletteFetchTimer = setTimeout(() => void refreshPalette(), 80);
});

paletteInput.addEventListener("keydown", (event) => {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    paletteSelected = Math.min(
      paletteSelected + 1,
      paletteEntries.length - 1,
    );
    renderPalette();
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    paletteSelected = Math.max(paletteSelected - 1, 0);
    renderPalette();
  } else if (event.key === "Enter") {
    event.preventDefault();
    void runPaletteSelection();
  } else if (event.key === "Escape") {
    event.preventDefault();
    closePalette();
  }
});

paletteOverlay.addEventListener("click", (event) => {
  if (event.target === paletteOverlay) {
    closePalette();
  }
});

/* ------------------------------------------------------------------ */
/* Commands (from the shared-core keymap's KNOWN_COMMANDS).           */
/* ------------------------------------------------------------------ */

async function runCommand(command: string): Promise<void> {
  switch (command) {
    case "quick_open":
      openPalette("file");
      break;
    case "command_palette":
      openPalette("command");
      break;
    case "toggle_file_tree":
      fileTreePane.classList.toggle("hidden");
      break;
    case "tab_recent_next":
      focusNextTab();
      break;
    case "tab_close":
      if (activeTabIndex !== null) {
        await closeTab(activeTabIndex);
      }
      break;
    case "search_in_folder":
      openPalette("search");
      break;
    case "open_folder":
      folderInput.focus();
      folderInput.select();
      break;
    case "save":
      await saveActiveTab();
      break;
    default:
      // Commands the PoC backend knows but the frontend does not
      // implement yet (file-tree-only or v1 commands such as
      // new_file / rename / cursor_down). Reported, not faked.
      statusFile.textContent = `Command "${command}" is not implemented in this PoC.`;
      break;
  }
}

/* ------------------------------------------------------------------ */
/* Keymap: load from the backend, apply on keydown.                   */
/* ------------------------------------------------------------------ */

function normaliseKey(key: string): string {
  return key.trim().toLowerCase().replace(/ /g, "");
}

function keyDescription(event: KeyboardEvent): string {
  const parts: string[] = [];
  if (event.ctrlKey || event.metaKey) {
    parts.push("ctrl");
  }
  if (event.shiftKey) {
    parts.push("shift");
  }
  if (event.altKey) {
    parts.push("alt");
  }
  parts.push(event.key.toLowerCase());
  return parts.join("+");
}

function commandForKey(area: string, key: string): string | undefined {
  // Area bindings win over global ones, like shared-core.
  return (
    keyBindings.get(`${area}:${normaliseKey(key)}`) ??
    keyBindings.get(`global:${normaliseKey(key)}`)
  );
}

async function loadKeymap(): Promise<void> {
  try {
    const info = await invoke<KeymapInfo>("load_keymap");
    keyBindings.clear();
    for (const binding of info.bindings) {
      keyBindings.set(
        `${binding.area}:${normaliseKey(binding.key)}`,
        binding.command,
      );
    }
    statusKeymap.textContent = `Keymap: ${info.location} (${info.source})`;
  } catch (error) {
    statusKeymap.textContent = `Keymap: failed to load (${String(error)})`;
  }
}

document.addEventListener("keydown", (event) => {
  // The palette handles its own keys (except global Escape, handled
  // by the palette input listener).
  if (!paletteOverlay.hidden) {
    return;
  }
  const target = event.target as HTMLElement | null;
  const inTextInput =
    target !== null &&
    (target.tagName === "INPUT" || target.tagName === "TEXTAREA");

  if (event.key === "Escape") {
    return;
  }

  // Ctrl+S is a PoC extra (not in the Notion keymap): save now.
  if ((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "s") {
    event.preventDefault();
    void saveActiveTab();
    return;
  }

  const inTree = target === fileTreeElement;
  const description = keyDescription(event);
  const hasModifier = event.ctrlKey || event.metaKey || event.shiftKey || event.altKey;

  if (inTree && !hasModifier) {
    // Single keys in the file tree use the [file_tree] area.
    const command = commandForKey("file_tree", description);
    if (command === undefined) {
      return;
    }
    event.preventDefault();
    void runTreeCommand(command);
    return;
  }

  if (!hasModifier || inTextInput) {
    return;
  }
  const command = commandForKey("editor", description);
  if (command === undefined) {
    return;
  }
  // Let the editor keep plain Ctrl+C / Ctrl+V / Ctrl+Z / Ctrl+A etc:
  // only intercept keys the keymap actually binds.
  event.preventDefault();
  void runCommand(command);
});

async function runTreeCommand(command: string): Promise<void> {
  const visible = visibleTreeEntries();
  switch (command) {
    case "cursor_down":
      treeKeyboardFocus = Math.min(treeKeyboardFocus + 1, visible.length - 1);
      renderTree();
      break;
    case "cursor_up":
      treeKeyboardFocus = Math.max(treeKeyboardFocus - 1, 0);
      renderTree();
      break;
    case "expand_or_open": {
      const entry = visible[treeKeyboardFocus];
      if (entry === undefined) {
        break;
      }
      if (entry.is_folder) {
        toggleFolder(entry.path);
      } else if (entry.is_markdown) {
        await openFile(entry.path);
      }
      break;
    }
    case "collapse": {
      const entry = visible[treeKeyboardFocus];
      if (entry !== undefined && entry.is_folder) {
        collapsedFolders.add(entry.path);
        renderTree();
      }
      break;
    }
    default:
      await runCommand(command);
      break;
  }
}

/* ------------------------------------------------------------------ */
/* Status bar.                                                        */
/* ------------------------------------------------------------------ */

function renderStatusFile(): void {
  const tab = activeTab();
  if (tab === null) {
    statusFile.textContent =
      currentFolder === null ? "No folder open" : currentFolder;
    return;
  }
  statusFile.textContent = tab.path + (tab.dirty ? " ●" : "");
}

function renderCursorStatus(): void {
  const head = editorView.state.selection.main.head;
  const line = editorView.state.doc.lineAt(head);
  statusCursor.textContent = `Ln ${line.number}, Col ${head - line.from + 1}`;
}

/* ------------------------------------------------------------------ */
/* Wiring and startup.                                                */
/* ------------------------------------------------------------------ */

openFolderButton.addEventListener("click", () => {
  const path = folderInput.value.trim();
  if (path !== "") {
    void openFolder(path).catch((error: unknown) => {
      statusFile.textContent = `Could not open folder: ${String(error)}`;
    });
  }
});

folderInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    openFolderButton.click();
  }
});

toggleTreeButton.addEventListener("click", () => {
  fileTreePane.classList.toggle("hidden");
});

async function start(): Promise<void> {
  renderSaveState();
  renderStatusFile();
  await loadKeymap();
  try {
    const defaultFolder = await invoke<string>("default_folder");
    folderInput.value = defaultFolder;
    await openFolder(defaultFolder);
  } catch (error) {
    statusFile.textContent = `Startup: ${String(error)}`;
  }
}

void start();
