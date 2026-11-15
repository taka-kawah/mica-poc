//! One open document: the shared-core buffer plus the egui-side
//! editing state for the per-line live-preview editor.
//!
//! The buffer always holds raw Markdown. The editor shows one line at
//! a time as a raw single-line `TextEdit` (the *cursor line*) and every
//! other line as a styled read-only paragraph built from
//! `shared-core` decorations. `line_edit_text` is the in-progress
//! content of the cursor line's `TextEdit`; it is synced back into
//! the buffer on every change.

use mica_shared_core::auto_save::{AutoSaver, SaveState};
use mica_shared_core::text_buffer::{CursorPosition, TextBuffer};
use std::path::{Path, PathBuf};

/// An open file with its buffer, save state and editing cursor.
pub struct OpenDocument {
    /// File on disk.
    pub path: PathBuf,
    /// Raw Markdown buffer (single source of truth).
    pub buffer: TextBuffer,
    /// Debounced auto-save tracker from shared-core.
    pub auto_saver: AutoSaver,
    /// The line currently being edited (shown as a raw `TextEdit`).
    pub editing_line: usize,
    /// Column (in `char`s) of the editing cursor inside the editing line.
    pub editing_column: usize,
    /// Live content of the cursor line's `TextEdit`. Kept in sync with
    /// `buffer` line `editing_line` whenever it changes.
    pub line_edit_text: String,
    /// `true` after the editing line changed and the `TextEdit` needs
    /// keyboard focus (requested once, then cleared).
    pub needs_focus: bool,
    /// `true` when this frame's `TextEdit` input changed the editing
    /// line (see [`OpenDocument::apply_line_input`]). A Backspace that
    /// already deleted a character inside the line must not also merge
    /// the line upwards.
    pub line_changed_this_frame: bool,
}

impl OpenDocument {
    /// Load a document from disk. Missing/unreadable files start empty
    /// (the error is surfaced by the caller through the status bar).
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => (Self::from_text(path, &text), None),
            Err(error) => (
                Self::from_text(path, ""),
                Some(format!("could not read {}: {error}", path.display())),
            ),
        }
    }

    /// Create a document from in-memory text (used by tests).
    pub fn from_text(path: &Path, text: &str) -> Self {
        let buffer = TextBuffer::from_text(text);
        let first_line = buffer.line(0).unwrap_or("").to_string();
        Self {
            path: path.to_path_buf(),
            buffer,
            auto_saver: AutoSaver::new(),
            editing_line: 0,
            editing_column: 0,
            line_edit_text: first_line,
            needs_focus: false,
            line_changed_this_frame: false,
        }
    }

    /// Current save state for the status bar.
    pub fn save_state(&self) -> SaveState {
        self.auto_saver.state()
    }

    /// `true` when the document has edits that are not on disk yet
    /// (including a save that started but failed).
    pub fn has_unsaved_changes(&self) -> bool {
        matches!(self.save_state(), SaveState::Dirty | SaveState::Saving)
            || self.buffer.line(self.editing_line).unwrap_or("") != self.line_edit_text
    }

    /// Apply one frame of cursor-line `TextEdit` input: record the
    /// cursor column the widget reports (in `char`s) and sync the
    /// edited text into the buffer. Remembers whether the text changed
    /// so the app can tell "Backspace deleted a character" apart from
    /// "Backspace at column 0 of an unchanged line" (only the latter
    /// merges the line upwards; see [`Self::backspace_should_merge`]).
    pub fn apply_line_input(&mut self, cursor_column: Option<usize>) {
        if let Some(column) = cursor_column {
            self.editing_column = column;
        }
        self.line_changed_this_frame = self.sync_line_edit_to_buffer();
    }

    /// `true` when a Backspace pressed this frame should merge the
    /// editing line into the line above: the cursor is at column 0
    /// and the `TextEdit` did not already consume the Backspace by
    /// deleting a character (which would leave the cursor at 0 too,
    /// for example when a one-character line was just emptied).
    pub fn backspace_should_merge(&self) -> bool {
        self.editing_column == 0 && !self.line_changed_this_frame
    }

    /// Move the editing cursor to another line, syncing the
    /// `TextEdit` content from the buffer. The new line's `TextEdit`
    /// requests keyboard focus on the next frame.
    pub fn move_editing_line(&mut self, line: usize) {
        self.sync_line_edit_to_buffer();
        let line = line.min(self.buffer.line_count().saturating_sub(1));
        self.editing_line = line;
        let raw = self.buffer.line(line).unwrap_or("");
        let column = self.editing_column.min(raw.chars().count());
        self.editing_column = column;
        self.line_edit_text = raw.to_string();
        self.needs_focus = true;
    }

    /// Push `line_edit_text` back into the buffer when it differs from
    /// the buffer's line. Returns `true` when the buffer changed.
    /// `editing_column` is left alone: it is the `TextEdit`'s cursor,
    /// not the end of the re-inserted line.
    pub fn sync_line_edit_to_buffer(&mut self) -> bool {
        let raw = self.buffer.line(self.editing_line).unwrap_or("");
        if raw == self.line_edit_text {
            return false;
        }
        let line_length = raw.chars().count();
        self.buffer.delete_range(
            CursorPosition {
                line: self.editing_line,
                column: 0,
            },
            CursorPosition {
                line: self.editing_line,
                column: line_length,
            },
        );
        self.buffer.insert(
            CursorPosition {
                line: self.editing_line,
                column: 0,
            },
            &self.line_edit_text.clone(),
        );
        let line_length = self.line_edit_text.chars().count();
        self.editing_column = self.editing_column.min(line_length);
        self.auto_saver.note_edit();
        true
    }

    /// Split the editing line at the cursor column: the text before
    /// the cursor stays on this line, the rest moves to a new line
    /// below, which becomes the editing line. Returns `true` on success.
    pub fn split_editing_line_at_cursor(&mut self) -> bool {
        self.sync_line_edit_to_buffer();
        let chars: Vec<char> = self.line_edit_text.chars().collect();
        let at = self.editing_column.min(chars.len());
        let before: String = chars[..at].iter().collect();
        let after: String = chars[at..].iter().collect();
        let line = self.editing_line;
        let line_length = self
            .buffer
            .line(line)
            .map(|text| text.chars().count())
            .unwrap_or(0);
        self.buffer.delete_range(
            CursorPosition { line, column: 0 },
            CursorPosition {
                line,
                column: line_length,
            },
        );
        self.buffer.insert(
            CursorPosition { line, column: 0 },
            &format!("{before}\n{after}"),
        );
        self.auto_saver.note_edit();
        self.editing_line = line + 1;
        self.editing_column = 0;
        self.line_edit_text = after;
        self.needs_focus = true;
        true
    }

    /// Merge the editing line into the line above (Backspace at column
    /// 0). Returns `true` when a merge happened.
    pub fn merge_editing_line_upwards(&mut self) -> bool {
        if self.editing_line == 0 {
            return false;
        }
        self.sync_line_edit_to_buffer();
        let line = self.editing_line;
        let above_length = self
            .buffer
            .line(line - 1)
            .map(|text| text.chars().count())
            .unwrap_or(0);
        // Deleting the newline joins the lines; the buffer now holds
        // `above + current` on line `line - 1`.
        self.buffer.delete_range(
            CursorPosition {
                line: line - 1,
                column: above_length,
            },
            CursorPosition { line, column: 0 },
        );
        self.auto_saver.note_edit();
        self.editing_line = line - 1;
        self.editing_column = above_length;
        self.line_edit_text = self.buffer.line(line - 1).unwrap_or("").to_string();
        self.needs_focus = true;
        true
    }

    /// Save the buffer to disk atomically right now (used by auto-save
    /// and by Ctrl+S). Returns `Ok(())` on success. On failure the
    /// document stays dirty (the debounce restarts, so auto-save
    /// retries later) and the edits stay in memory.
    pub fn save_now(&mut self) -> std::io::Result<()> {
        self.sync_line_edit_to_buffer();
        self.auto_saver.begin_save();
        let result =
            mica_shared_core::auto_save::save_atomically(&self.path, &self.buffer.to_text());
        match result {
            Ok(()) => {
                self.auto_saver.finish_save();
                Ok(())
            }
            Err(error) => {
                self.auto_saver.note_edit();
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_line_at_cursor_moves_rest_down() {
        let mut document = OpenDocument::from_text(Path::new("note.md"), "hello world\nsecond");
        document.editing_column = 5;
        assert!(document.split_editing_line_at_cursor());
        assert_eq!(document.buffer.to_text(), "hello\n world\nsecond\n");
        assert_eq!(document.editing_line, 1);
        assert_eq!(document.line_edit_text, " world");
    }

    #[test]
    fn merge_line_upwards_joins_lines() {
        let mut document = OpenDocument::from_text(Path::new("note.md"), "hello\nworld");
        document.move_editing_line(1);
        document.editing_column = 0;
        assert!(document.merge_editing_line_upwards());
        assert_eq!(document.buffer.to_text(), "helloworld\n");
        assert_eq!(document.editing_line, 0);
        assert_eq!(document.editing_column, 5);
    }

    #[test]
    fn sync_line_edit_marks_document_dirty() {
        let mut document = OpenDocument::from_text(Path::new("note.md"), "one");
        assert_eq!(document.save_state(), SaveState::Clean);
        document.line_edit_text = "one two".to_string();
        assert!(document.sync_line_edit_to_buffer());
        assert_eq!(document.save_state(), SaveState::Dirty);
        assert_eq!(document.buffer.to_text(), "one two\n");
    }

    #[test]
    fn backspace_emptying_a_line_keeps_the_empty_line() {
        // Regression: Backspace on a one-character line used to delete
        // the character AND merge the (now empty) line upwards, because
        // the column was forced to the line end (0) after every sync.
        let mut document = OpenDocument::from_text(Path::new("note.md"), "above\na\nbelow");
        document.move_editing_line(1);
        document.apply_line_input(Some(1));
        assert!(!document.line_changed_this_frame);
        // The TextEdit handles Backspace: "a" -> "", cursor at 0.
        document.line_edit_text.clear();
        document.apply_line_input(Some(0));
        assert!(
            !document.backspace_should_merge(),
            "the Backspace was consumed by deleting 'a'"
        );
        assert_eq!(document.buffer.to_text(), "above\n\nbelow\n");
        assert_eq!(document.editing_line, 1);
        // A second Backspace on the now-empty, unchanged line merges.
        document.apply_line_input(Some(0));
        assert!(document.backspace_should_merge());
        assert!(document.merge_editing_line_upwards());
        assert_eq!(document.buffer.to_text(), "above\nbelow\n");
    }

    #[test]
    fn typing_keeps_the_textedit_cursor_column() {
        // Regression: the status bar column jumped to the line end
        // after typing in the middle of a line.
        let mut document = OpenDocument::from_text(Path::new("note.md"), "hello world");
        document.line_edit_text = "hello, world".to_string();
        document.apply_line_input(Some(6));
        assert_eq!(document.editing_column, 6);
        assert_eq!(document.buffer.to_text(), "hello, world\n");
        // Enter after that splits at the real cursor, not the line end.
        assert!(document.split_editing_line_at_cursor());
        assert_eq!(document.buffer.to_text(), "hello,\n world\n");
    }

    #[test]
    fn japanese_line_edit_round_trips() {
        let mut document = OpenDocument::from_text(Path::new("note.md"), "# こんにちは");
        document.line_edit_text = "# こんにちは、世界".to_string();
        document.sync_line_edit_to_buffer();
        assert_eq!(document.buffer.to_text(), "# こんにちは、世界\n");
    }
}
