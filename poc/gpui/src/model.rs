//! Headless editor model for the GPUI PoC.
//!
//! This module has no GPUI dependency: it wires `mica-shared-core`
//! (text buffer, live preview, file tree, tabs, auto-save, keymap,
//! command palette) into one document model that the GPUI view renders
//! and that the `--smoke-test` mode and the unit tests drive without a
//! display. All text positions are (line, column) in `char`s, exactly
//! like the shared core, so Japanese text is safe by construction.

use std::collections::HashMap;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mica_shared_core::auto_save::{save_atomically, AutoSaver, SaveState};
use mica_shared_core::command_palette::{filter_entries, PaletteEntry};
use mica_shared_core::file_tree::{scan_folder, FileTreeEntry};
use mica_shared_core::keymap::{Keymap, KNOWN_COMMANDS};
use mica_shared_core::live_preview::{decorate_line, DisplayLine};
use mica_shared_core::tabs::TabManager;
use mica_shared_core::text_buffer::{CursorPosition, TextBuffer};

/// The whole document text the GPUI input handler works on: the active
/// buffer's lines joined with `\n`, with no trailing newline, so
/// UTF-16 offsets over it map cleanly onto buffer positions.
pub fn document_text(buffer: &TextBuffer) -> String {
    (0..buffer.line_count())
        .map(|line| buffer.line(line).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert a UTF-16 offset inside `document_text` to a (line, column)
/// buffer position. Offsets past the end clamp to the end.
pub fn cursor_for_utf16_offset(buffer: &TextBuffer, offset_utf16: usize) -> CursorPosition {
    let mut remaining_utf16 = offset_utf16;
    for line in 0..buffer.line_count() {
        let text = buffer.line(line).unwrap_or("");
        let line_utf16 = text.encode_utf16().count();
        if remaining_utf16 <= line_utf16 {
            // Walk the line's chars to find the column.
            let mut column = 0;
            let mut used_utf16 = 0;
            for ch in text.chars() {
                if used_utf16 >= remaining_utf16 {
                    break;
                }
                used_utf16 += ch.len_utf16();
                column += 1;
            }
            return CursorPosition { line, column };
        }
        remaining_utf16 -= line_utf16;
        // The '\n' between lines costs one UTF-16 unit.
        if line + 1 < buffer.line_count() {
            if remaining_utf16 == 0 {
                return CursorPosition {
                    line,
                    column: text.chars().count(),
                };
            }
            remaining_utf16 -= 1;
        }
    }
    let last = buffer.line_count().saturating_sub(1);
    CursorPosition {
        line: last,
        column: buffer
            .line(last)
            .map(|text| text.chars().count())
            .unwrap_or(0),
    }
}

/// Convert a buffer position to a UTF-16 offset inside `document_text`.
pub fn utf16_offset_for_cursor(buffer: &TextBuffer, cursor: CursorPosition) -> usize {
    let mut offset = 0;
    for line in 0..cursor.line.min(buffer.line_count()) {
        offset += buffer.line(line).unwrap_or("").encode_utf16().count();
        offset += 1; // '\n'
    }
    let line_text = buffer.line(cursor.line).unwrap_or("");
    offset += line_text
        .chars()
        .take(cursor.column)
        .map(|ch| ch.len_utf16())
        .sum::<usize>();
    offset
}

/// Which list the command palette overlay shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    /// Ctrl+P: Markdown files in the folder.
    QuickOpen,
    /// Ctrl+Shift+P: command names.
    Commands,
}

/// Replace the UTF-16 range `range_utf16` of `text` with `new_text`
/// (offsets clamp to the end; a reversed range is normalised). Returns
/// the UTF-16 offset where `new_text` starts. Used for the palette
/// query, which is a plain string rather than a buffer.
pub fn replace_utf16_range(text: &mut String, range_utf16: Range<usize>, new_text: &str) -> usize {
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let mut start = range_utf16.start.min(utf16.len());
    let mut end = range_utf16.end.min(utf16.len());
    if end < start {
        std::mem::swap(&mut start, &mut end);
    }
    // Count chars by skipping low surrogates (the second half of a
    // surrogate pair belongs to the char that started before it).
    let chars_before = |offset: usize| {
        utf16[..offset]
            .iter()
            .filter(|unit| (**unit & 0xFC00) != 0xDC00)
            .count()
    };
    let mut chars: Vec<char> = text.chars().collect();
    chars.splice(chars_before(start)..chars_before(end), new_text.chars());
    *text = chars.into_iter().collect();
    start
}

/// The editor model: folder, tree, tabs, one buffer + cursor +
/// auto-saver per open file, and the global keymap.
pub struct EditorModel {
    pub folder: PathBuf,
    pub tree: Vec<FileTreeEntry>,
    pub tabs: TabManager,
    pub buffers: HashMap<PathBuf, TextBuffer>,
    pub cursors: HashMap<PathBuf, CursorPosition>,
    pub savers: HashMap<PathBuf, AutoSaver>,
    /// The loaded global keymap (`~/.config/mica/keymap.toml`).
    /// The GPUI key bindings are built from the same file (see
    /// `view::key_bindings_from_keymap_file`); this copy is kept so
    /// the model can answer `command_for_key` lookups without
    /// re-reading the file.
    #[allow(dead_code)]
    pub keymap: Keymap,
    /// Index into `tree` of the keyboard-selected row.
    pub tree_selection: usize,
    pub show_tree: bool,
    /// Active IME composition: the document it belongs to and its
    /// UTF-16 range over that document's `document_text`. Read it
    /// through [`EditorModel::marked_range`], which only reports it
    /// for the active document.
    ime_marked: Option<(PathBuf, Range<usize>)>,
}

impl EditorModel {
    /// Open a folder: scan the tree and load the global keymap
    /// (`~/.config/mica/keymap.toml`, defaults when the file is absent
    /// or invalid).
    pub fn open_folder(folder: &Path) -> Self {
        let keymap = load_keymap();
        Self {
            folder: folder.to_path_buf(),
            tree: scan_folder(folder),
            tabs: TabManager::new(),
            buffers: HashMap::new(),
            cursors: HashMap::new(),
            savers: HashMap::new(),
            keymap,
            tree_selection: 0,
            show_tree: true,
            ime_marked: None,
        }
    }

    /// Re-scan the folder (external changes). Called when the file
    /// watcher (a later step) reports a change; kept as API for now.
    #[allow(dead_code)]
    pub fn rescan(&mut self) {
        self.tree = scan_folder(&self.folder);
    }

    /// The path of the active tab, if any.
    pub fn active_path(&self) -> Option<PathBuf> {
        self.tabs.active_tab().map(|tab| tab.path.clone())
    }

    /// Open a file in a tab (reopening focuses the existing tab) and
    /// load its content into a buffer.
    pub fn open_file(&mut self, path: &Path) -> io::Result<()> {
        let content = std::fs::read_to_string(path)?;
        self.tabs.open_file(path);
        self.end_background_composition();
        self.buffers
            .entry(path.to_path_buf())
            .or_insert_with(|| TextBuffer::from_text(&content));
        self.cursors
            .entry(path.to_path_buf())
            .or_insert_with(CursorPosition::default);
        self.savers
            .entry(path.to_path_buf())
            .or_insert_with(AutoSaver::new);
        Ok(())
    }

    /// The active buffer, if a file is open.
    pub fn active_buffer(&self) -> Option<&TextBuffer> {
        self.active_path().and_then(|path| self.buffers.get(&path))
    }

    /// The active buffer, mutably.
    pub fn active_buffer_mut(&mut self) -> Option<&mut TextBuffer> {
        let path = self.active_path()?;
        self.buffers.get_mut(&path)
    }

    /// The active cursor position.
    pub fn cursor(&self) -> CursorPosition {
        self.active_path()
            .and_then(|path| self.cursors.get(&path).copied())
            .unwrap_or_default()
    }

    /// Set the active cursor position (clamped to the buffer).
    pub fn set_cursor(&mut self, cursor: CursorPosition) {
        if let Some(path) = self.active_path() {
            let line_count = self
                .buffers
                .get(&path)
                .map(|buffer| buffer.line_count())
                .unwrap_or(1);
            let line = cursor.line.min(line_count.saturating_sub(1));
            let column = cursor.column.min(
                self.buffers
                    .get(&path)
                    .and_then(|buffer| buffer.line(line))
                    .map(|text| text.chars().count())
                    .unwrap_or(0),
            );
            self.cursors.insert(path, CursorPosition { line, column });
        }
    }

    /// Insert text at the cursor and move the cursor after it.
    /// Returns the new cursor position.
    pub fn insert_at_cursor(&mut self, text: &str) -> CursorPosition {
        let cursor = self.cursor();
        let new_cursor = self
            .active_buffer_mut()
            .map(|buffer| buffer.insert(cursor, text))
            .unwrap_or(cursor);
        self.set_cursor(new_cursor);
        self.note_edit();
        new_cursor
    }

    /// Delete the character before the cursor (Backspace).
    pub fn delete_before_cursor(&mut self) {
        let cursor = self.cursor();
        if cursor.column == 0 && cursor.line == 0 {
            return;
        }
        let start = if cursor.column > 0 {
            CursorPosition {
                line: cursor.line,
                column: cursor.column - 1,
            }
        } else {
            let previous_line = cursor.line - 1;
            let previous_length = self
                .active_buffer()
                .and_then(|buffer| buffer.line(previous_line))
                .map(|text| text.chars().count())
                .unwrap_or(0);
            CursorPosition {
                line: previous_line,
                column: previous_length,
            }
        };
        if let Some(buffer) = self.active_buffer_mut() {
            buffer.delete_range(start, cursor);
        }
        self.set_cursor(start);
        self.note_edit();
    }

    /// Delete the character after the cursor (Delete key).
    pub fn delete_after_cursor(&mut self) {
        let cursor = self.cursor();
        let (line_length, line_count) = self
            .active_buffer()
            .map(|buffer| {
                (
                    buffer
                        .line(cursor.line)
                        .map(|text| text.chars().count())
                        .unwrap_or(0),
                    buffer.line_count(),
                )
            })
            .unwrap_or((0, 1));
        let end = if cursor.column < line_length {
            CursorPosition {
                line: cursor.line,
                column: cursor.column + 1,
            }
        } else if cursor.line + 1 < line_count {
            CursorPosition {
                line: cursor.line + 1,
                column: 0,
            }
        } else {
            return;
        };
        if let Some(buffer) = self.active_buffer_mut() {
            buffer.delete_range(cursor, end);
        }
        self.note_edit();
    }

    /// Move the cursor by (line_delta, column_delta), clamping to the
    /// buffer. Column movement is by `char`s.
    pub fn move_cursor(&mut self, line_delta: isize, column_delta: isize) {
        let cursor = self.cursor();
        let line_count = self
            .active_buffer()
            .map(|buffer| buffer.line_count())
            .unwrap_or(1);
        let new_line = (cursor.line as isize + line_delta)
            .clamp(0, line_count.saturating_sub(1) as isize) as usize;
        let line_length = self
            .active_buffer()
            .and_then(|buffer| buffer.line(new_line))
            .map(|text| text.chars().count())
            .unwrap_or(0);
        let new_column = if line_delta != 0 {
            cursor.column.min(line_length)
        } else {
            (cursor.column as isize + column_delta).clamp(0, line_length as isize) as usize
        };
        self.set_cursor(CursorPosition {
            line: new_line,
            column: new_column,
        });
    }

    /// Move the cursor to the start / end of its line.
    pub fn move_cursor_to_line_edge(&mut self, to_end: bool) {
        let cursor = self.cursor();
        let column = if to_end {
            self.active_buffer()
                .and_then(|buffer| buffer.line(cursor.line))
                .map(|text| text.chars().count())
                .unwrap_or(0)
        } else {
            0
        };
        self.set_cursor(CursorPosition {
            line: cursor.line,
            column,
        });
    }

    /// The active IME composition range (UTF-16 offsets over the
    /// active document), if a composition is in progress there.
    pub fn marked_range(&self) -> Option<Range<usize>> {
        let (path, range) = self.ime_marked.as_ref()?;
        (self.active_path().as_ref() == Some(path)).then(|| range.clone())
    }

    /// The range an input-handler call with `range_utf16` acts on:
    /// the given range, else the current composition (the platform
    /// passes `None` for every preedit update and the commit on X11,
    /// Wayland and Windows), else the cursor. Same rule as GPUI's
    /// `examples/input.rs` (`.or(self.marked_range)`).
    fn input_range(&self, range_utf16: Option<(usize, usize)>) -> Option<(usize, usize)> {
        range_utf16.or_else(|| self.marked_range().map(|range| (range.start, range.end)))
    }

    /// Replace a UTF-16 range (or the cursor when `None`) of the
    /// active document with `text`, without recording an edit.
    /// Returns (start of the inserted text, cursor after it).
    fn replace_range_raw(
        &mut self,
        range_utf16: Option<(usize, usize)>,
        text: &str,
    ) -> (CursorPosition, CursorPosition) {
        let Some(buffer) = self.active_buffer() else {
            return (CursorPosition::default(), CursorPosition::default());
        };
        let (start, end) = match range_utf16 {
            Some((start, end)) => (
                cursor_for_utf16_offset(buffer, start),
                cursor_for_utf16_offset(buffer, end),
            ),
            None => {
                let cursor = self.cursor();
                (cursor, cursor)
            }
        };
        let (start, end) = if (end.line, end.column) < (start.line, start.column) {
            (end, start)
        } else {
            (start, end)
        };
        if let Some(buffer) = self.active_buffer_mut() {
            buffer.delete_range(start, end);
        }
        let new_cursor = self
            .active_buffer_mut()
            .map(|buffer| buffer.insert(start, text))
            .unwrap_or(start);
        self.set_cursor(new_cursor);
        (start, new_cursor)
    }

    /// Apply an input-handler edit (typed text or an IME commit):
    /// replace `range_utf16` — or, when `None`, the current IME
    /// composition, or the cursor when nothing is composing — with
    /// `text`. Ends any composition and records an edit (auto-save).
    /// Returns the new cursor position.
    pub fn replace_text_in_range(
        &mut self,
        range_utf16: Option<(usize, usize)>,
        text: &str,
    ) -> CursorPosition {
        let range = self.input_range(range_utf16);
        self.ime_marked = None;
        let (_, new_cursor) = self.replace_range_raw(range, text);
        self.note_edit();
        new_cursor
    }

    /// Apply an IME preedit update: replace `range_utf16` (or the
    /// current composition, or the cursor) with `text` and mark it as
    /// the composition. `new_selected_utf16` is the cursor/selection
    /// inside `text` (relative UTF-16 offsets); the cursor goes to its
    /// end, or to the end of `text` when `None`.
    ///
    /// Composing text is NOT recorded as an edit: it does not mark the
    /// document dirty or restart the auto-save timer, and auto-save
    /// skips a document while it is composing. Only when the
    /// composition ends (empty `text` = cancelled, or a commit through
    /// [`Self::replace_text_in_range`], or [`Self::unmark_text`]) is
    /// the edit recorded.
    pub fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<(usize, usize)>,
        text: &str,
        new_selected_utf16: Option<(usize, usize)>,
    ) {
        let Some(path) = self.active_path() else {
            return;
        };
        let range = self.input_range(range_utf16);
        let (start, _) = self.replace_range_raw(range, text);
        if text.is_empty() {
            // Composition cancelled: the marked range clears.
            self.ime_marked = None;
            self.note_edit();
            return;
        }
        let Some(buffer) = self.active_buffer() else {
            return;
        };
        let start_utf16 = utf16_offset_for_cursor(buffer, start);
        let text_utf16 = text.encode_utf16().count();
        let cursor_utf16 = start_utf16
            + new_selected_utf16
                .map(|(_, end)| end.min(text_utf16))
                .unwrap_or(text_utf16);
        let cursor = cursor_for_utf16_offset(buffer, cursor_utf16);
        self.ime_marked = Some((path, start_utf16..start_utf16 + text_utf16));
        self.set_cursor(cursor);
    }

    /// The platform accepted the composition as it is. Returns `true`
    /// when a composition was in progress (its text is now a normal
    /// edit and the auto-save timer starts).
    pub fn unmark_text(&mut self) -> bool {
        if self.ime_marked.take().is_some() {
            self.note_edit();
            true
        } else {
            false
        }
    }

    /// Focus the tab at `index` (tab click).
    pub fn focus_tab(&mut self, index: usize) {
        self.tabs.focus(index);
        self.end_background_composition();
    }

    /// Focus the next tab (wrapping around).
    pub fn focus_next_tab(&mut self) {
        self.tabs.focus_next();
        self.end_background_composition();
    }

    /// Focus the previous tab (wrapping around).
    pub fn focus_previous_tab(&mut self) {
        self.tabs.focus_previous();
        self.end_background_composition();
    }

    /// After a tab switch: a composition left in a document that is no
    /// longer active is accepted as it is (like `unmark_text`) and
    /// recorded as an edit of THAT document, so auto-save no longer
    /// skips it. The platform IME is not involved: `marked_range`
    /// already reports no composition for the new active tab.
    fn end_background_composition(&mut self) {
        let active = self.active_path();
        let Some((path, _)) = self
            .ime_marked
            .take_if(|(path, _)| Some(&*path) != active.as_ref())
        else {
            return;
        };
        self.savers
            .entry(path.clone())
            .or_insert_with(AutoSaver::new)
            .note_edit();
        self.tabs.set_dirty(&path, true);
    }

    /// `true` while an IME composition is in progress in `path`.
    fn is_composing(&self, path: &Path) -> bool {
        matches!(&self.ime_marked, Some((marked, _)) if marked == path)
    }

    /// Record an edit: mark the active tab dirty and restart the
    /// auto-save timer.
    pub fn note_edit(&mut self) {
        if let Some(path) = self.active_path() {
            self.savers
                .entry(path.clone())
                .or_insert_with(AutoSaver::new)
                .note_edit();
            self.tabs.set_dirty(&path, true);
        }
    }

    /// Save state of the active document.
    pub fn save_state(&self) -> SaveState {
        self.active_path()
            .and_then(|path| self.savers.get(&path).map(|saver| saver.state()))
            .unwrap_or(SaveState::Clean)
    }

    /// `true` when the document at `path` has edits not on disk yet
    /// (including a save that failed).
    pub fn has_unsaved_changes(&self, path: &Path) -> bool {
        self.savers
            .get(path)
            .map(|saver| matches!(saver.state(), SaveState::Dirty | SaveState::Saving))
            .unwrap_or(false)
    }

    /// Save one open document now. On failure it stays dirty (the
    /// debounce restarts, so auto-save retries) and keeps its edits.
    fn save_path(&mut self, path: &Path) -> io::Result<()> {
        let content = self
            .buffers
            .get(path)
            .map(|buffer| buffer.to_text())
            .unwrap_or_default();
        let saver = self
            .savers
            .entry(path.to_path_buf())
            .or_insert_with(AutoSaver::new);
        saver.begin_save();
        match save_atomically(path, &content) {
            Ok(()) => {
                saver.finish_save();
                saver.acknowledge_saved();
                self.tabs.set_dirty(path, false);
                Ok(())
            }
            Err(error) => {
                saver.note_edit();
                Err(error)
            }
        }
    }

    /// Auto-save tick: save EVERY open document (active or in a
    /// background tab) whose debounce delay has passed. A document
    /// with an IME composition in progress is skipped until the
    /// composition ends. Returns `true` when at least one save
    /// happened.
    pub fn maybe_auto_save(&mut self) -> bool {
        let due: Vec<PathBuf> = self
            .savers
            .iter()
            .filter(|(path, saver)| saver.save_is_due() && !self.is_composing(path))
            .map(|(path, _)| path.clone())
            .collect();
        let mut saved = false;
        for path in due {
            saved |= self.save_path(&path).is_ok();
        }
        saved
    }

    /// Save the active document immediately, ignoring the debounce.
    pub fn save_now(&mut self) -> bool {
        let Some(path) = self.active_path() else {
            return false;
        };
        self.save_path(&path).is_ok()
    }

    /// Save every document with unsaved edits now (quit). Returns one
    /// message per failed save; an empty list means everything is on
    /// disk.
    pub fn flush_all(&mut self) -> Vec<String> {
        let dirty: Vec<PathBuf> = self
            .savers
            .keys()
            .filter(|path| self.has_unsaved_changes(path))
            .cloned()
            .collect();
        dirty
            .into_iter()
            .filter_map(|path| {
                self.save_path(&path)
                    .err()
                    .map(|error| format!("{}: {error}", path.display()))
            })
            .collect()
    }

    /// Close the tab at `index`. A document with unsaved edits is
    /// saved first; when that save fails the tab stays open (edits
    /// kept) and the error is returned. `Ok(false)` means there was no
    /// tab at `index`.
    pub fn close_tab(&mut self, index: usize) -> io::Result<bool> {
        let Some(path) = self.tabs.tabs().get(index).map(|tab| tab.path.clone()) else {
            return Ok(false);
        };
        if self.has_unsaved_changes(&path) {
            self.save_path(&path)?;
        }
        self.tabs.close(index);
        self.buffers.remove(&path);
        self.cursors.remove(&path);
        self.savers.remove(&path);
        if self.is_composing(&path) {
            self.ime_marked = None;
        }
        Ok(true)
    }

    /// Live-preview display lines for the active buffer, with the
    /// cursor line's markers shown and all other lines' hidden.
    pub fn display_lines(&self) -> Vec<DisplayLine> {
        let Some(buffer) = self.active_buffer() else {
            return Vec::new();
        };
        let cursor_line = self.cursor().line;
        let mut lines = Vec::with_capacity(buffer.line_count());
        let mut in_code_block = false;
        for index in 0..buffer.line_count() {
            let text = buffer.line(index).unwrap_or("");
            let (display, next_in_code_block) =
                decorate_line(text, index, Some(cursor_line), in_code_block);
            in_code_block = next_in_code_block;
            lines.push(display);
        }
        lines
    }

    /// Palette entries: commands for the command palette, Markdown
    /// files for Quick Open.
    pub fn palette_entries(&self, kind: PaletteKind) -> Vec<PaletteEntry> {
        match kind {
            PaletteKind::Commands => KNOWN_COMMANDS
                .iter()
                .map(|command| PaletteEntry {
                    title: (*command).to_string(),
                    detail: "command".to_string(),
                    action: (*command).to_string(),
                })
                .collect(),
            PaletteKind::QuickOpen => self
                .tree
                .iter()
                .filter(|entry| entry.is_markdown)
                .map(|entry| PaletteEntry {
                    title: entry.name.clone(),
                    detail: entry.path.to_string_lossy().into_owned(),
                    action: entry.path.to_string_lossy().into_owned(),
                })
                .collect(),
        }
    }

    /// Filtered palette entries for a query, best match first.
    pub fn filtered_palette_entries(&self, kind: PaletteKind, query: &str) -> Vec<PaletteEntry> {
        let entries = self.palette_entries(kind);
        filter_entries(query, &entries)
            .into_iter()
            .cloned()
            .collect()
    }
}

/// Load the global keymap from `~/.config/mica/keymap.toml`,
/// falling back to the built-in defaults when the file is missing
/// or invalid.
pub fn load_keymap() -> Keymap {
    let path = keymap_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => Keymap::from_toml(&text).unwrap_or_else(|_| Keymap::default_keymap()),
        Err(_) => Keymap::default_keymap(),
    }
}

/// Path of the global keymap file (Notion §7).
pub fn keymap_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("mica")
                .join("keymap.toml")
        })
        .unwrap_or_else(|| PathBuf::from(".config/mica/keymap.toml"))
}

/// Auto-save debounce delay used by the GUI scheduling (matches the
/// shared core's 1000 ms, plus a small margin).
pub const AUTO_SAVE_WAIT: Duration = Duration::from_millis(1100);

#[cfg(test)]
mod tests {
    use super::*;

    fn model_with_text(text: &str) -> EditorModel {
        let mut model = EditorModel::open_folder(Path::new("/nonexistent-folder-for-tests"));
        let path = PathBuf::from("/tmp/mica-gpui-test-note.md");
        model.tabs.open_file(&path);
        model
            .buffers
            .insert(path.clone(), TextBuffer::from_text(text));
        model
            .cursors
            .insert(path.clone(), CursorPosition::default());
        model.savers.insert(path, AutoSaver::new());
        model
    }

    #[test]
    fn bold_and_italic_segments_carry_their_styles() {
        let mut model = model_with_text("Some **bold** and *italic* words here.\nsecond line\n");
        // Cursor on the other line so markers are hidden on this line.
        model.set_cursor(CursorPosition { line: 1, column: 0 });
        let lines = model.display_lines();
        assert_eq!(lines[0].display_text, "Some bold and italic words here.");
        let bold = lines[0]
            .segments
            .iter()
            .find(|segment| segment.text == "bold")
            .expect("bold segment");
        assert!(bold.style.bold, "bold segment must be bold");
        let italic = lines[0]
            .segments
            .iter()
            .find(|segment| segment.text == "italic")
            .expect("italic segment");
        assert!(italic.style.italic, "italic segment must be italic");
    }

    #[test]
    fn japanese_text_survives_buffer_and_decoration_path() {
        let mut model = model_with_text("# こんにちは\nこれは**太字**です\n");
        // Insert Japanese text through the buffer path, as an IME
        // commit would arrive through the input handler. (The heading
        // already contains こんにちは, so insert a different word and
        // check both survive exactly once each.)
        model.set_cursor(CursorPosition { line: 1, column: 2 });
        model.insert_at_cursor("こんばんは");
        let text = document_text(model.active_buffer().unwrap());
        assert!(text.contains("こんばんは"));
        assert_eq!(text.matches("こんばんは").count(), 1);
        assert_eq!(text.matches("こんにちは").count(), 1);

        // The decoration path must not corrupt it either: the heading
        // line hides its marker, the cursor line shows its markers.
        let lines = model.display_lines();
        assert_eq!(lines[0].display_text, "こんにちは");
        assert_eq!(lines[1].display_text, "これこんばんはは**太字**です");
        // Cursor mapping still lands on the right buffer column.
        assert_eq!(lines[0].buffer_column_for_display(0), 2);
    }

    #[test]
    fn utf16_offsets_round_trip_with_japanese_and_emoji() {
        let model = model_with_text("こんにちは🌸\nsecond line\n");
        let buffer = model.active_buffer().unwrap().clone();
        // Every (line, column) survives a round trip through UTF-16
        // offsets, including the emoji (a surrogate pair: 2 UTF-16
        // units, 1 char) and the line break.
        for line in 0..buffer.line_count() {
            let length = buffer.line(line).unwrap().chars().count();
            for column in 0..=length {
                let cursor = CursorPosition { line, column };
                let offset = utf16_offset_for_cursor(&buffer, cursor);
                assert_eq!(cursor_for_utf16_offset(&buffer, offset), cursor);
            }
        }
        // The emoji itself: offset just past it is 7 UTF-16 units
        // (5 kana + 2 for the surrogate pair).
        let after_emoji = cursor_for_utf16_offset(&buffer, 7);
        assert_eq!(
            after_emoji,
            CursorPosition { line: 0, column: 6 },
            "emoji must count as one char column"
        );
    }

    #[test]
    fn ime_preedit_updates_replace_the_composition_then_commit() {
        // X11 / Wayland / Windows pass `range_utf16 = None` for every
        // preedit update and for the commit. Each update must replace
        // the previous preedit, not insert next to it (which produced
        // かかん漢 before the fix).
        let mut model = model_with_text("前\n");
        model.set_cursor(CursorPosition { line: 0, column: 1 });
        model.replace_and_mark_text_in_range(None, "か", Some((1, 1)));
        assert_eq!(document_text(model.active_buffer().unwrap()), "前か");
        assert_eq!(model.marked_range(), Some(1..2));
        assert_eq!(model.cursor(), CursorPosition { line: 0, column: 2 });
        model.replace_and_mark_text_in_range(None, "かん", Some((2, 2)));
        assert_eq!(document_text(model.active_buffer().unwrap()), "前かん");
        assert_eq!(model.marked_range(), Some(1..3));
        // Composing text is not an edit: no dirty state, no auto-save.
        assert_eq!(model.save_state(), SaveState::Clean);
        assert!(!model.tabs.active_tab().unwrap().dirty);

        model.replace_text_in_range(None, "漢");
        let text = document_text(model.active_buffer().unwrap());
        assert_eq!(text, "前漢");
        assert_eq!(text.matches('漢').count(), 1);
        assert!(!text.contains('か'));
        assert_eq!(model.marked_range(), None);
        assert_eq!(model.cursor(), CursorPosition { line: 0, column: 2 });
        // The commit is the edit that starts auto-save.
        assert_eq!(model.save_state(), SaveState::Dirty);

        // A plain insert after the commit goes to the cursor again.
        model.replace_text_in_range(None, "字");
        assert_eq!(document_text(model.active_buffer().unwrap()), "前漢字");
    }

    #[test]
    fn ime_cancel_and_unmark_end_the_composition() {
        let mut model = model_with_text("ab\n");
        model.set_cursor(CursorPosition { line: 0, column: 1 });
        model.replace_and_mark_text_in_range(None, "に", None);
        assert_eq!(document_text(model.active_buffer().unwrap()), "aにb");
        // Cancel: empty preedit removes the composition.
        model.replace_and_mark_text_in_range(None, "", None);
        assert_eq!(document_text(model.active_buffer().unwrap()), "ab");
        assert_eq!(model.marked_range(), None);
        // Unmark keeps the composed text as a normal edit.
        model.replace_and_mark_text_in_range(None, "日", None);
        assert!(model.unmark_text());
        assert_eq!(document_text(model.active_buffer().unwrap()), "a日b");
        assert_eq!(model.marked_range(), None);
        assert_eq!(model.save_state(), SaveState::Dirty);
        assert!(!model.unmark_text(), "nothing left to unmark");
    }

    #[test]
    fn palette_query_replace_utf16_range() {
        let mut query = String::from("ab🌸");
        // Preedit → preedit → commit over the same range, as the
        // palette does with its own marked range.
        let start = replace_utf16_range(&mut query, 4..4, "か");
        assert_eq!((query.as_str(), start), ("ab🌸か", 4));
        replace_utf16_range(&mut query, 4..5, "かん");
        replace_utf16_range(&mut query, 4..6, "漢");
        assert_eq!(query, "ab🌸漢");
        // Replacing across the surrogate pair removes the whole emoji.
        replace_utf16_range(&mut query, 2..4, "");
        assert_eq!(query, "ab漢");
    }

    /// A model over a fresh folder with `a.md` and `b.md` open; `b.md`
    /// is active and `a.md` is a background tab with one unsaved edit
    /// (its saver uses `delay`).
    fn model_with_dirty_background_tab(
        name: &str,
        delay: Duration,
    ) -> (EditorModel, PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("mica-gpui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.md");
        let b = dir.join("b.md");
        std::fs::write(&a, "alpha\n").unwrap();
        std::fs::write(&b, "beta\n").unwrap();
        let mut model = EditorModel::open_folder(&dir);
        model.open_file(&a).unwrap();
        model.savers.insert(a.clone(), AutoSaver::with_delay(delay));
        model.insert_at_cursor("edited ");
        model.open_file(&b).unwrap();
        assert_eq!(model.active_path().as_deref(), Some(b.as_path()));
        assert!(model.has_unsaved_changes(&a));
        (model, dir, a, b)
    }

    #[test]
    fn auto_save_tick_saves_dirty_background_documents() {
        let (mut model, dir, a, _) =
            model_with_dirty_background_tab("tick-background", Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        assert!(model.maybe_auto_save(), "background document was due");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "edited alpha\n");
        assert!(!model.has_unsaved_changes(&a));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_save_skips_a_document_while_composing() {
        let (mut model, dir, _, b) =
            model_with_dirty_background_tab("tick-composing", Duration::from_millis(1));
        model
            .savers
            .insert(b.clone(), AutoSaver::with_delay(Duration::from_millis(1)));
        model.insert_at_cursor("typed ");
        model.replace_and_mark_text_in_range(None, "か", None);
        std::thread::sleep(Duration::from_millis(10));
        model.maybe_auto_save();
        assert_eq!(
            std::fs::read_to_string(&b).unwrap(),
            "beta\n",
            "composing document must not be saved mid-composition"
        );
        model.replace_text_in_range(None, "漢");
        std::thread::sleep(Duration::from_millis(10));
        assert!(model.maybe_auto_save());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "typed 漢beta\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn switching_tabs_mid_composition_ends_it_so_auto_save_runs() {
        let (mut model, dir, a, b) =
            model_with_dirty_background_tab("switch-composing", Duration::from_millis(1));
        model
            .savers
            .insert(b.clone(), AutoSaver::with_delay(Duration::from_millis(1)));
        model.replace_and_mark_text_in_range(None, "か", None);
        assert!(
            !model.has_unsaved_changes(&b),
            "composing is not an edit yet"
        );

        // Switch to a.md with no further input: the composition in b.md
        // is accepted as it is and recorded as b.md's edit.
        let index_a = model
            .tabs
            .tabs()
            .iter()
            .position(|tab| tab.path == a)
            .unwrap();
        model.focus_tab(index_a);
        assert!(model.has_unsaved_changes(&b));
        assert!(model
            .tabs
            .tabs()
            .iter()
            .any(|tab| tab.path == b && tab.dirty));
        model.focus_next_tab();
        assert_eq!(model.active_path().as_deref(), Some(b.as_path()));
        assert_eq!(model.marked_range(), None, "no stale composition on return");

        std::thread::sleep(Duration::from_millis(10));
        assert!(model.maybe_auto_save());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "かbeta\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quit_flush_reports_a_failed_save() {
        // The window-close and Ctrl+Q guards stay open when this list
        // is non-empty.
        let (mut model, dir, a, _) =
            model_with_dirty_background_tab("flush-fails", Duration::from_secs(60));
        std::fs::remove_dir_all(&dir).unwrap();
        let errors = model.flush_all();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("a.md"));
        assert!(model.has_unsaved_changes(&a), "edits kept, still dirty");
    }

    #[test]
    fn closing_a_dirty_background_tab_saves_it_first() {
        // Inside the debounce window: only the close itself can save.
        let (mut model, dir, a, _) =
            model_with_dirty_background_tab("close-saves", Duration::from_secs(60));
        let index = model
            .tabs
            .tabs()
            .iter()
            .position(|tab| tab.path == a)
            .unwrap();
        assert!(model.close_tab(index).unwrap());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "edited alpha\n");
        assert!(!model.buffers.contains_key(&a));
        assert!(model.tabs.tabs().iter().all(|tab| tab.path != a));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn closing_a_tab_whose_save_fails_keeps_it_open() {
        let (mut model, dir, a, _) =
            model_with_dirty_background_tab("close-fails", Duration::from_secs(60));
        std::fs::remove_dir_all(&dir).unwrap();
        let index = model
            .tabs
            .tabs()
            .iter()
            .position(|tab| tab.path == a)
            .unwrap();
        assert!(model.close_tab(index).is_err());
        assert!(model.tabs.tabs().iter().any(|tab| tab.path == a));
        assert!(model.has_unsaved_changes(&a), "edits kept, still dirty");
        assert_eq!(
            model.buffers[&a].line(0),
            Some("edited alpha"),
            "edits stay in memory"
        );
    }

    #[test]
    fn quit_flush_saves_every_dirty_document() {
        let (mut model, dir, a, b) =
            model_with_dirty_background_tab("flush-all", Duration::from_secs(60));
        model.insert_at_cursor("typed ");
        assert!(model.flush_all().is_empty());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "edited alpha\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "typed beta\n");
        assert!(!model.has_unsaved_changes(&a) && !model.has_unsaved_changes(&b));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn input_handler_replace_range_inserts_at_cursor() {
        let mut model = model_with_text("hello world\n");
        model.set_cursor(CursorPosition { line: 0, column: 6 });
        model.replace_text_in_range(None, "こんにちは ");
        assert_eq!(
            document_text(model.active_buffer().unwrap()),
            "hello こんにちは world"
        );
        assert_eq!(
            model.cursor(),
            CursorPosition {
                line: 0,
                column: 6 + "こんにちは ".chars().count()
            }
        );
    }

    #[test]
    fn auto_save_writes_file_after_delay() {
        let dir = std::env::temp_dir().join("mica-gpui-auto-save-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.md");
        std::fs::write(&path, "original\n").unwrap();
        let mut model = EditorModel::open_folder(&dir);
        model.open_file(&path).unwrap();
        model.insert_at_cursor("edited ");
        assert!(!model.maybe_auto_save(), "not due yet");
        std::thread::sleep(Duration::from_millis(1100));
        assert!(model.maybe_auto_save(), "due after the delay");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "edited original\n");
        assert_eq!(model.save_state(), SaveState::Clean);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keymap_file_parses_and_defaults_apply() {
        let keymap = Keymap::default_keymap();
        assert_eq!(
            keymap.command_for_key("editor", "ctrl+p"),
            Some("quick_open")
        );
        assert_eq!(
            keymap.command_for_key("editor", "ctrl+shift+p"),
            Some("command_palette")
        );
    }

    #[test]
    fn palette_filters_files_and_commands() {
        let model = model_with_text("# hi\n");
        let commands = model.filtered_palette_entries(PaletteKind::Commands, "tab");
        assert!(commands.iter().any(|entry| entry.action == "tab_close"));
        assert!(
            !commands.iter().any(|entry| entry.action == "quick_open"),
            "quick_open must not match 'tab'"
        );
    }
}
