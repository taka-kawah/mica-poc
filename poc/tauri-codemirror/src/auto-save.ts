/**
 * Auto-save core for the Tauri + CodeMirror 6 PoC, kept free of DOM
 * and Tauri imports so `npm run test:dom` can exercise it in node.
 *
 * The race this guards against: while the backend write is in flight,
 * the user keeps typing. The edit updates `tab.content`, sets
 * `tab.dirty` and restarts the debounce timer. If the save then
 * cleared `dirty` unconditionally, the restarted timer would see a
 * clean tab and skip the save, leaving the newer text unwritten while
 * the status bar says Saved. So the content is snapshotted before the
 * write, and `dirty` is only cleared when the buffer still equals
 * that snapshot when the write returns.
 */

/** The parts of an editor tab the save path reads and writes. */
export interface SaveableTab {
  path: string;
  /** Current buffer content (raw Markdown). */
  content: string;
  dirty: boolean;
}

/**
 * What happened to one save:
 * - `saved`: the snapshot was written and the buffer still matches it;
 *   the tab is clean.
 * - `changed`: the snapshot was written, but the buffer changed while
 *   the write was in flight; the tab stays dirty so the next save
 *   writes the newer text.
 * - `failed`: the write threw; the tab stays dirty.
 */
export type SaveOutcome =
  | { kind: "saved" }
  | { kind: "changed" }
  | { kind: "failed"; error: unknown };

/** Write the tab's current content with `write` and update `dirty`. */
export async function saveTabContent(
  tab: SaveableTab,
  write: (path: string, content: string) => Promise<unknown>,
): Promise<SaveOutcome> {
  const snapshot = tab.content;
  try {
    await write(tab.path, snapshot);
  } catch (error) {
    return { kind: "failed", error };
  }
  if (tab.content !== snapshot) {
    return { kind: "changed" };
  }
  tab.dirty = false;
  return { kind: "saved" };
}
