//! Live-preview decorations: how a line is *shown* without changing
//! the buffer.
//!
//! Rule (row-level, from the Notion child page): on the line where the
//! cursor sits, every Markdown marker is shown, so the user can edit
//! it. On every other line, markers are hidden and the text is shown
//! styled — headings larger, bold heavier, code in monospace.
//!
//! A [`DisplayLine`] is what a GUI renders for one buffer line. It
//! carries display segments plus the mapping between buffer columns
//! and display columns, so cursor movement and clicks can skip over
//! hidden markers instead of stopping on invisible characters.

use crate::markdown_parser::{parse_line, BlockKind, ParsedLine, SpanStyle};

/// One piece of visible text on a display line.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplaySegment {
    /// The text to draw.
    pub text: String,
    /// How to draw it.
    pub style: DisplayStyle,
    /// Buffer byte range this segment came from (for hit-testing).
    pub buffer_range: (usize, usize),
}

/// Visual style of one display segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayStyle {
    /// Bold text (bold spans, headings).
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Monospace (code spans, code blocks).
    pub monospace: bool,
    /// Relative font size: 1.0 is body text; headings are larger.
    /// Heading level 1 is the largest.
    pub size_factor: f32,
    /// Dimmed text (for example a horizontal rule).
    pub dimmed: bool,
}

impl Default for DisplayStyle {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            monospace: false,
            size_factor: 1.0,
            dimmed: false,
        }
    }
}

/// One buffer line, prepared for display.
#[derive(Debug, Clone)]
pub struct DisplayLine {
    /// The segments to draw, in order.
    pub segments: Vec<DisplaySegment>,
    /// The text the user sees (segments joined).
    pub display_text: String,
    /// For each display column (in `char`s), the buffer column it maps
    /// to. Length is `display_text.chars().count() + 1`; the last entry
    /// is the buffer column just past the line.
    pub display_to_buffer_columns: Vec<usize>,
    /// True when this line is the cursor line (markers shown).
    pub is_cursor_line: bool,
    /// Relative line height: headings need more room.
    pub line_height_factor: f32,
}

impl DisplayLine {
    /// Map a display column (in `char`s) back to a buffer column.
    pub fn buffer_column_for_display(&self, display_column: usize) -> usize {
        let index = display_column.min(self.display_to_buffer_columns.len().saturating_sub(1));
        self.display_to_buffer_columns[index]
    }

    /// Map a buffer column (in `char`s) to the nearest display column.
    pub fn display_column_for_buffer(&self, buffer_column: usize) -> usize {
        // Find the first display column whose buffer column is >= target.
        self.display_to_buffer_columns
            .iter()
            .position(|column| *column >= buffer_column)
            .unwrap_or_else(|| self.display_to_buffer_columns.len().saturating_sub(1))
    }
}

/// Build the display form of one line.
///
/// `cursor_line` is the line the cursor is on, or `None` when no line
/// is being edited. `in_code_block` is the fence state entering this
/// line (see [`parse_line`]).
pub fn decorate_line(
    line: &str,
    line_index: usize,
    cursor_line: Option<usize>,
    in_code_block: bool,
) -> (DisplayLine, bool) {
    let (parsed, next_in_code_block) = parse_line(line, in_code_block);
    let is_cursor_line = cursor_line == Some(line_index);
    let hide_markers = !is_cursor_line;
    (
        build_display_line(line, &parsed, is_cursor_line, hide_markers),
        next_in_code_block,
    )
}

fn build_display_line(
    line: &str,
    parsed: &ParsedLine,
    is_cursor_line: bool,
    hide_markers: bool,
) -> DisplayLine {
    let mut segments: Vec<DisplaySegment> = Vec::new();
    // display column -> buffer column, built in character units.
    // Starts empty: the first visible segment seeds the map with its
    // own buffer start, so hidden leading markers do not shift it.
    let mut column_map: Vec<usize> = Vec::new();
    let mut display_chars: usize = 0;

    // Helper: append visible text and extend the column map.
    let push_text = |segments: &mut Vec<DisplaySegment>,
                     column_map: &mut Vec<usize>,
                     display_chars: &mut usize,
                     text: &str,
                     style: DisplayStyle,
                     buffer_start_char: usize| {
        if text.is_empty() {
            return;
        }
        let char_count = text.chars().count();
        segments.push(DisplaySegment {
            text: text.to_string(),
            style,
            buffer_range: (
                char_offset_to_byte(line, buffer_start_char),
                char_offset_to_byte(line, buffer_start_char + char_count),
            ),
        });
        // The map has one entry per display *position* (between
        // characters). The entry at the boundary between two segments
        // belongs to the segment that starts there: when markers were
        // hidden, the new segment starts later in the buffer than the
        // previous segment ended, so overwrite the boundary entry
        // instead of appending after it.
        if column_map.len() == *display_chars {
            column_map.push(buffer_start_char);
        } else {
            column_map[*display_chars] = buffer_start_char;
        }
        for step in 1..=char_count {
            column_map.push(buffer_start_char + step);
        }
        *display_chars += char_count;
    };

    let heading_size = match parsed.kind {
        BlockKind::Heading { level } => match level {
            1 => 1.6,
            2 => 1.4,
            3 => 1.25,
            4 => 1.15,
            _ => 1.1,
        },
        _ => 1.0,
    };
    let line_height_factor = heading_size;

    // Block marker: heading hashes, list bullet, quote '>'.
    // Shown on the cursor line (or when it is not a marker that
    // live preview hides); hidden elsewhere.
    if let Some((marker_start, marker_end)) = parsed.block_marker {
        let marker_text = &line[marker_start..marker_end];
        let is_code_fence = parsed.kind == BlockKind::CodeBlock;
        let is_rule = parsed.kind == BlockKind::HorizontalRule;
        if is_code_fence || is_rule {
            // Fences and rules are structural: show a dimmed marker.
            let style = DisplayStyle {
                dimmed: true,
                monospace: is_code_fence,
                ..DisplayStyle::default()
            };
            push_text(
                &mut segments,
                &mut column_map,
                &mut display_chars,
                if is_rule {
                    "────────────────────────"
                } else {
                    marker_text
                },
                style,
                byte_to_char_offset(line, marker_start),
            );
        } else if !hide_markers {
            push_text(
                &mut segments,
                &mut column_map,
                &mut display_chars,
                marker_text,
                DisplayStyle::default(),
                byte_to_char_offset(line, marker_start),
            );
        }
        // When hidden, the marker contributes no display columns and
        // no map entries, so the cursor skips over it.
    }

    // Content spans.
    let content_is_code = parsed.kind == BlockKind::CodeBlock;
    for span in &parsed.spans {
        let (span_start, span_end) = span.range;
        let show_opening = span.opening_marker.is_some() && !hide_markers;
        let show_closing = span.closing_marker.is_some() && !hide_markers;
        if show_opening {
            let (marker_start, marker_end) = span.opening_marker.unwrap();
            push_text(
                &mut segments,
                &mut column_map,
                &mut display_chars,
                &line[marker_start..marker_end],
                DisplayStyle::default(),
                byte_to_char_offset(line, marker_start),
            );
        }
        let mut style = DisplayStyle {
            bold: span.style == SpanStyle::Bold || matches!(parsed.kind, BlockKind::Heading { .. }),
            italic: span.style == SpanStyle::Italic,
            monospace: span.style == SpanStyle::Code || content_is_code,
            size_factor: heading_size,
            ..DisplayStyle::default()
        };
        if content_is_code {
            style = DisplayStyle {
                monospace: true,
                ..DisplayStyle::default()
            };
        }
        push_text(
            &mut segments,
            &mut column_map,
            &mut display_chars,
            &line[span_start..span_end],
            style,
            byte_to_char_offset(line, span_start),
        );
        if show_closing {
            let (marker_start, marker_end) = span.closing_marker.unwrap();
            push_text(
                &mut segments,
                &mut column_map,
                &mut display_chars,
                &line[marker_start..marker_end],
                DisplayStyle::default(),
                byte_to_char_offset(line, marker_start),
            );
        }
    }

    let display_text: String = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect();
    if column_map.is_empty() {
        // Empty line: one display column mapping to buffer column 0.
        column_map.push(0);
    }
    DisplayLine {
        segments,
        display_text,
        display_to_buffer_columns: column_map,
        is_cursor_line,
        line_height_factor,
    }
}

/// Byte offset -> character offset inside one line.
fn byte_to_char_offset(line: &str, byte_offset: usize) -> usize {
    line[..byte_offset.min(line.len())].chars().count()
}

/// Character offset -> byte offset inside one line.
fn char_offset_to_byte(line: &str, char_offset: usize) -> usize {
    line.char_indices()
        .nth(char_offset)
        .map(|(byte, _)| byte)
        .unwrap_or(line.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_markers_hidden_off_cursor_line() {
        let (display, _) = decorate_line("## Hello", 0, Some(5), false);
        assert_eq!(display.display_text, "Hello");
        assert!(!display.is_cursor_line);
        assert!(display.line_height_factor > 1.0);
        // Display column 0 maps to buffer column 3 (after "## ").
        assert_eq!(display.buffer_column_for_display(0), 3);
    }

    #[test]
    fn heading_markers_shown_on_cursor_line() {
        let (display, _) = decorate_line("## Hello", 0, Some(0), false);
        assert_eq!(display.display_text, "## Hello");
        assert_eq!(display.buffer_column_for_display(0), 0);
    }

    #[test]
    fn bold_markers_hidden_and_cursor_skips_them() {
        let (display, _) = decorate_line("a **bold** b", 1, Some(0), false);
        assert_eq!(display.display_text, "a bold b");
        // Display column 2 ('b' of bold) maps to buffer column 4.
        assert_eq!(display.buffer_column_for_display(2), 4);
        // Buffer column 2 (first '*') maps to display column 2.
        assert_eq!(display.display_column_for_buffer(2), 2);
    }

    #[test]
    fn code_block_interior_line_is_one_monospace_segment() {
        let line = "    println!(\"こんにちは\");";
        let (display, still_in_block) = decorate_line(line, 3, Some(0), true);
        assert!(still_in_block);
        assert_eq!(display.display_text, line);
        assert_eq!(display.segments.len(), 1);
        assert!(display.segments[0].style.monospace);
        assert!(!display.segments[0].style.dimmed);
        assert_eq!(display.segments[0].style.size_factor, 1.0);
        assert_eq!(display.segments[0].buffer_range, (0, line.len()));
        // Identity column mapping: nothing is hidden inside code.
        let char_count = line.chars().count();
        assert_eq!(
            display.display_to_buffer_columns,
            (0..=char_count).collect::<Vec<_>>()
        );
    }

    #[test]
    fn japanese_heading_round_trips() {
        let (display, _) = decorate_line("# こんにちは", 0, Some(9), false);
        assert_eq!(display.display_text, "こんにちは");
        assert_eq!(display.buffer_column_for_display(0), 2);
    }
}
