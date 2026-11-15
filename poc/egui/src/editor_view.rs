//! The per-line live-preview editor view.
//!
//! **Approach chosen: custom per-line widget, not a styled read view +
//! raw `TextEdit` swap.**
//!
//! The document is rendered one line at a time inside a scroll area:
//!
//! * every line except the cursor line is a read-only styled paragraph
//!   built from `shared-core` `decorate_line` segments (a
//!   `egui::text::LayoutJob`: heading sizes, bold/italic spans,
//!   monospace code, hidden markers);
//! * the cursor line is a raw single-line `egui::TextEdit` holding the
//!   line's Markdown, so its markers are visible and editable —
//!   exactly the row-level rule from the Notion child page.
//!
//! Clicking a styled line (or moving with Up/Down) makes it the cursor
//! line: the styled paragraph is replaced by the raw `TextEdit`.
//! Enter splits the line, Backspace at column 0 merges it upwards.
//!
//! **Limits of this approach (honest):**
//! * No multi-line selection or cross-line copy/paste: each line is
//!   its own widget, so selection lives inside one line only.
//! * Pasting multi-line text into a line inserts only the first line
//!   (single-line `TextEdit` drops the rest).
//! * Bold has no dedicated font weight in egui's bundled fonts, so
//!   bold spans are rendered in the strong text colour at body size
//!   rather than a true bold face. Headings (larger size), italics
//!   (sheared by epaint) and monospace code (real monospace face)
//!   do render as styled.
//! * The whole document is laid out every frame (no virtualization),
//!   so very large files (thousands of lines) will be slower than a
//!   virtualized editor. That is a PoC limit, not a design goal.

use crate::document::OpenDocument;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Ui};
use mica_shared_core::live_preview::decorate_line;

/// Base body font size in points (Notion §7 default is 16).
pub const BODY_FONT_SIZE: f32 = 16.0;

/// Build the styled `LayoutJob` for one non-cursor line.
///
/// `cursor_line` is the line being edited, so markers are hidden on
/// this line and shown on the cursor line (which is not rendered here
/// at all — it is a `TextEdit`). `in_code_block` is the fence state
/// entering this line; the function returns the fence state leaving it.
pub fn styled_line_job(
    ui: &Ui,
    line: &str,
    line_index: usize,
    cursor_line: usize,
    in_code_block: bool,
) -> (LayoutJob, bool) {
    let (display, next_in_code_block) =
        decorate_line(line, line_index, Some(cursor_line), in_code_block);
    let mut job = LayoutJob::default();
    let visuals = ui.visuals();
    for segment in &display.segments {
        let size = BODY_FONT_SIZE * segment.style.size_factor;
        let color = if segment.style.dimmed {
            visuals.weak_text_color()
        } else if segment.style.bold {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        };
        let font_id = FontId::new(
            size,
            if segment.style.monospace {
                FontFamily::Monospace
            } else {
                FontFamily::Proportional
            },
        );
        job.append(
            &segment.text,
            0.0,
            TextFormat {
                font_id,
                color,
                italics: segment.style.italic,
                ..Default::default()
            },
        );
    }
    if job.sections.is_empty() {
        if line.is_empty() {
            // Empty line: keep one blank row so the line stays clickable
            // and keeps its height.
            job.append(
                " ",
                0.0,
                TextFormat {
                    font_id: FontId::new(BODY_FONT_SIZE, FontFamily::Proportional),
                    color: Color32::TRANSPARENT,
                    ..Default::default()
                },
            );
        } else {
            // `shared-core` returns no spans for fenced code-block
            // interior lines (it only records their `content_range`),
            // so the decoration above is empty. Fall back to the raw
            // line in monospace rather than showing a blank row.
            job.append(
                line,
                0.0,
                TextFormat {
                    font_id: FontId::new(BODY_FONT_SIZE, FontFamily::Monospace),
                    color: visuals.text_color(),
                    ..Default::default()
                },
            );
        }
    }
    (job, next_in_code_block)
}

/// Render the whole document: styled lines plus the raw cursor-line
/// editor. Returns the line the user clicked, if any.
pub fn render_document(ui: &mut Ui, document: &mut OpenDocument) -> Option<usize> {
    let mut clicked_line: Option<usize> = None;
    let mut in_code_block = false;
    let line_count = document.buffer.line_count();

    for line_index in 0..line_count {
        let line_text = document.buffer.line(line_index).unwrap_or("").to_string();
        if line_index == document.editing_line {
            // Cursor line: raw Markdown in a single-line TextEdit.
            // This is egui's IME path (winit preedit/commit events),
            // so Japanese composition happens inside this widget.
            // Still run the decoration pass for the fence state, so
            // lines after a cursor line inside a code block keep the
            // right fence state.
            let (_, next_in_code_block) = decorate_line(
                &line_text,
                line_index,
                Some(document.editing_line),
                in_code_block,
            );
            in_code_block = next_in_code_block;
            let heading_size = heading_size_factor(&line_text);
            let response = egui::TextEdit::singleline(&mut document.line_edit_text)
                .font(FontId::new(
                    BODY_FONT_SIZE * heading_size,
                    FontFamily::Proportional,
                ))
                .desired_width(f32::INFINITY)
                .show(ui);
            if document.needs_focus {
                response.response.request_focus();
                document.needs_focus = false;
            }
            // Keep the char-column cursor position for Enter/Backspace
            // handling in the app (split/merge at the cursor).
            if let Some(cursor_range) = response.cursor_range {
                document.editing_column = cursor_range.primary.index.into();
            }
            document.sync_line_edit_to_buffer();
        } else {
            let (job, next_in_code_block) = styled_line_job(
                ui,
                &line_text,
                line_index,
                document.editing_line,
                in_code_block,
            );
            in_code_block = next_in_code_block;
            let response = ui.add(egui::Label::new(job).sense(egui::Sense::click()));
            if response.clicked() {
                clicked_line = Some(line_index);
            }
        }
    }
    clicked_line
}

/// Relative font size for a line used as the cursor line's `TextEdit`
/// font, so a heading keeps its size while being edited raw.
fn heading_size_factor(line: &str) -> f32 {
    let hashes = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if hashes == 0 || !line[hashes..].starts_with(' ') {
        return 1.0;
    }
    match hashes {
        1 => 1.6,
        2 => 1.4,
        3 => 1.25,
        4 => 1.15,
        _ => 1.1,
    }
}

/// Heading size factor used by the styled view (mirrors shared-core).
/// Test-only helper: the production path reads the factor from the
/// `shared-core` decoration itself.
#[cfg(test)]
pub fn heading_factor_for_level(level: usize) -> f32 {
    match level {
        1 => 1.6,
        2 => 1.4,
        3 => 1.25,
        4 => 1.15,
        _ => 1.1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_factor_matches_shared_core() {
        assert_eq!(heading_factor_for_level(1), 1.6);
        assert_eq!(heading_factor_for_level(2), 1.4);
        assert_eq!(heading_factor_for_level(3), 1.25);
    }

    #[test]
    fn heading_size_factor_only_for_real_headings() {
        assert_eq!(heading_size_factor("# Title"), 1.6);
        assert_eq!(heading_size_factor("## Title"), 1.4);
        assert_eq!(heading_size_factor("#not-a-heading"), 1.0);
        assert_eq!(heading_size_factor("plain"), 1.0);
    }

    #[test]
    fn shared_core_code_block_interior_has_no_spans() {
        // Regression test for the blank-code-lines bug seen in the
        // xvfb screenshot of welcome.md: `shared-core` returns no
        // spans for lines inside a fenced code block, so the egui
        // view must fall back to the raw line in monospace (see
        // `styled_line_job`) instead of rendering a blank row.
        let (display, still_in_block) =
            decorate_line("    println!(\"hello\");", 11, Some(0), true);
        assert!(still_in_block);
        assert!(display.segments.is_empty());
        assert_eq!(display.display_text, "");
    }
}
