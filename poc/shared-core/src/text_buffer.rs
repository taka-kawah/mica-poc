//! Plain-text buffer: the single source of truth for a document.
//!
//! The buffer always holds raw Markdown. Live preview never rewrites
//! the buffer — it only changes how lines are *displayed*
//! (see [`crate::live_preview`]). That is the design from the Notion
//! child page 「ライブプレビューの仕組み（事前調査）」: editing,
//! saving and undo all work on plain text.
//!
//! Positions are (line, column) with the column counted in Unicode
//! scalar values (`char`s), not bytes, so Japanese text and emoji
//! behave predictably.

/// A position in the buffer: zero-based line and column (in `char`s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CursorPosition {
    /// Zero-based line index.
    pub line: usize,
    /// Zero-based column index, counted in `char`s.
    pub column: usize,
}

/// A line-oriented text buffer holding raw Markdown.
#[derive(Debug, Clone, Default)]
pub struct TextBuffer {
    lines: Vec<String>,
}

impl TextBuffer {
    /// Create a buffer from whole-document text.
    /// A trailing newline does not create an extra empty line.
    pub fn from_text(text: &str) -> Self {
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self { lines }
    }

    /// Create an empty buffer (one empty line).
    pub fn new() -> Self {
        Self::from_text("")
    }

    /// Number of lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The text of one line, or `None` when out of range.
    pub fn line(&self, line: usize) -> Option<&str> {
        self.lines.get(line).map(String::as_str)
    }

    /// The whole document, lines joined with `\n` and a trailing newline.
    pub fn to_text(&self) -> String {
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }

    /// Insert text at a position. Newlines inside `text` split lines.
    /// Returns the position just after the inserted text.
    pub fn insert(&mut self, position: CursorPosition, text: &str) -> CursorPosition {
        let line = position.line.min(self.lines.len().saturating_sub(1));
        let column = position.column;
        let current = self.lines[line].clone();
        let chars: Vec<char> = current.chars().collect();
        let split_at = column.min(chars.len());
        let (before, after): (String, String) = (
            chars[..split_at].iter().collect(),
            chars[split_at..].iter().collect(),
        );
        let inserted_lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        if inserted_lines.len() == 1 {
            self.lines[line] = format!("{before}{}{after}", inserted_lines[0]);
            CursorPosition {
                line,
                column: split_at + inserted_lines[0].chars().count(),
            }
        } else {
            let mut new_lines: Vec<String> =
                Vec::with_capacity(self.lines.len() + inserted_lines.len() - 1);
            new_lines.extend_from_slice(&self.lines[..line]);
            new_lines.push(format!("{before}{}", inserted_lines[0]));
            for middle in &inserted_lines[1..inserted_lines.len() - 1] {
                new_lines.push(middle.clone());
            }
            let last_inserted = inserted_lines.last().unwrap().clone();
            new_lines.push(format!("{last_inserted}{after}"));
            new_lines.extend_from_slice(&self.lines[line + 1..]);
            let new_line = line + inserted_lines.len() - 1;
            let new_column = last_inserted.chars().count();
            self.lines = new_lines;
            CursorPosition {
                line: new_line,
                column: new_column,
            }
        }
    }

    /// Delete the characters in the half-open range
    /// `[start, end)` and return the deleted text.
    /// `end` must be at or after `start`; ranges spanning lines are allowed.
    pub fn delete_range(&mut self, start: CursorPosition, end: CursorPosition) -> String {
        if start.line >= self.lines.len() || end.line >= self.lines.len() {
            return String::new();
        }
        let (start, end) = if (end.line, end.column) < (start.line, start.column) {
            (end, start)
        } else {
            (start, end)
        };
        let deleted = self.text_in_range(start, end);
        if start.line == end.line {
            let chars: Vec<char> = self.lines[start.line].chars().collect();
            let from = start.column.min(chars.len());
            let to = end.column.min(chars.len());
            let remaining: String = chars[..from].iter().chain(&chars[to..]).collect();
            self.lines[start.line] = remaining;
        } else {
            let first_chars: Vec<char> = self.lines[start.line].chars().collect();
            let first_kept: String = first_chars[..start.column.min(first_chars.len())]
                .iter()
                .collect();
            let last_chars: Vec<char> = self.lines[end.line].chars().collect();
            let last_kept: String = last_chars[end.column.min(last_chars.len())..]
                .iter()
                .collect();
            let mut new_lines: Vec<String> = Vec::new();
            new_lines.extend_from_slice(&self.lines[..start.line]);
            new_lines.push(format!("{first_kept}{last_kept}"));
            new_lines.extend_from_slice(&self.lines[end.line + 1..]);
            self.lines = new_lines;
        }
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        deleted
    }

    /// The text inside the half-open range `[start, end)`.
    pub fn text_in_range(&self, start: CursorPosition, end: CursorPosition) -> String {
        if start.line >= self.lines.len() || end.line >= self.lines.len() {
            return String::new();
        }
        let (start, end) = if (end.line, end.column) < (start.line, start.column) {
            (end, start)
        } else {
            (start, end)
        };
        if start.line == end.line {
            let chars: Vec<char> = self.lines[start.line].chars().collect();
            let from = start.column.min(chars.len());
            let to = end.column.min(chars.len());
            return chars[from..to].iter().collect();
        }
        let mut parts: Vec<String> = Vec::new();
        let first_chars: Vec<char> = self.lines[start.line].chars().collect();
        parts.push(
            first_chars[start.column.min(first_chars.len())..]
                .iter()
                .collect(),
        );
        for line in &self.lines[start.line + 1..end.line] {
            parts.push(line.clone());
        }
        let last_chars: Vec<char> = self.lines[end.line].chars().collect();
        parts.push(
            last_chars[..end.column.min(last_chars.len())]
                .iter()
                .collect(),
        );
        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_ascii_and_japanese() {
        let mut buffer = TextBuffer::from_text("hello\nworld");
        let after = buffer.insert(CursorPosition { line: 0, column: 5 }, " こんにちは");
        assert_eq!(buffer.to_text(), "hello こんにちは\nworld\n");
        assert_eq!(after.column, 5 + " こんにちは".chars().count());
    }

    #[test]
    fn insert_newline_splits_line() {
        let mut buffer = TextBuffer::from_text("ab");
        buffer.insert(CursorPosition { line: 0, column: 1 }, "\n");
        assert_eq!(buffer.to_text(), "a\nb\n");
    }

    #[test]
    fn delete_within_and_across_lines() {
        let mut buffer = TextBuffer::from_text("hello\nworld\nagain");
        let deleted = buffer.delete_range(
            CursorPosition { line: 0, column: 2 },
            CursorPosition { line: 1, column: 3 },
        );
        assert_eq!(deleted, "llo\nwor");
        assert_eq!(buffer.to_text(), "held\nagain\n");
    }

    #[test]
    fn empty_document_has_one_empty_line() {
        let buffer = TextBuffer::new();
        assert_eq!(buffer.line_count(), 1);
        assert_eq!(buffer.to_text(), "\n");
    }
}
