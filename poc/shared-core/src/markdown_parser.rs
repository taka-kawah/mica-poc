//! Line-oriented Markdown structure for the live-preview PoC.
//!
//! This is a deliberately small, documented subset — not a full
//! CommonMark parser. It covers the constructs the PoC needs to
//! *display* (headings, bold, italic, code spans, lists, checkboxes,
//! links, horizontal rules, fenced code blocks). A full incremental
//! parser (tree-sitter-markdown) is a later MVP step; see
//! `docs/notion-spec-summary.md` and the README of this crate.
//!
//! All spans use half-open byte ranges `[start, end)` inside one line,
//! plus a `marker_range` for the Markdown symbols that live preview
//! hides on non-cursor lines (for example the `## ` of a heading or
//! the `**` around bold text).

/// What kind of block a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// `#`, `##`, … up to `######`. `heading_level` is 1–6.
    Heading { level: u8 },
    /// A list item (`-`, `*`, `+` or `1.`). May carry a checkbox.
    ListItem { checked: Option<bool> },
    /// A fenced code block line (the fence itself or a line inside it).
    CodeBlock,
    /// `---`, `***` or `___` on its own line.
    HorizontalRule,
    /// A block quote line starting with `>`.
    Quote,
    /// Anything else.
    Paragraph,
}

/// One styled span inside a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSpan {
    /// Half-open byte range of the *visible* text (markers excluded).
    pub range: (usize, usize),
    /// Byte range of the opening marker, if any (for example `**`).
    pub opening_marker: Option<(usize, usize)>,
    /// Byte range of the closing marker, if any.
    pub closing_marker: Option<(usize, usize)>,
    /// How the span should be rendered.
    pub style: SpanStyle,
}

/// How one span is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStyle {
    /// Plain text.
    Normal,
    /// `**bold**`.
    Bold,
    /// `*italic*`.
    Italic,
    /// `` `code` `` (monospace).
    Code,
    /// `[text](url)` — the visible part is the link text.
    Link,
}

/// The parsed structure of a single line.
#[derive(Debug, Clone)]
pub struct ParsedLine {
    /// The kind of block this line is.
    pub kind: BlockKind,
    /// Byte range of the heading/list/quote marker, if any.
    /// For a heading this is the `#…# ` prefix; for a list item the
    /// `- ` / `1. ` prefix (and `[ ]` / `[x]` when present).
    pub block_marker: Option<(usize, usize)>,
    /// Byte range of the visible content (block marker excluded).
    pub content_range: (usize, usize),
    /// Inline spans inside the content range.
    pub spans: Vec<InlineSpan>,
}

/// Parse one line of Markdown into its structure.
///
/// `in_code_block` tells the parser the previous line opened a fenced
/// code block that has not closed yet; the returned `in_code_block`
/// says whether the *next* line is still inside a fence.
pub fn parse_line(line: &str, in_code_block: bool) -> (ParsedLine, bool) {
    if in_code_block {
        if line.trim_start().starts_with("```") {
            return (
                ParsedLine {
                    kind: BlockKind::CodeBlock,
                    block_marker: Some((0, line.len())),
                    content_range: (line.len(), line.len()),
                    spans: Vec::new(),
                },
                false,
            );
        }
        return (
            ParsedLine {
                kind: BlockKind::CodeBlock,
                block_marker: None,
                content_range: (0, line.len()),
                spans: Vec::new(),
            },
            true,
        );
    }
    if line.trim_start().starts_with("```") {
        return (
            ParsedLine {
                kind: BlockKind::CodeBlock,
                block_marker: Some((0, line.len())),
                content_range: (line.len(), line.len()),
                spans: Vec::new(),
            },
            true,
        );
    }

    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();

    // Horizontal rule: ---, ***, ___ (three or more, nothing else).
    let rule_chars: String = trimmed
        .chars()
        .take_while(|c| *c == '-' || *c == '*' || *c == '_')
        .collect();
    if rule_chars.len() >= 3
        && rule_chars
            .chars()
            .all(|c| c == rule_chars.chars().next().unwrap())
        && trimmed.len() == rule_chars.len()
    {
        return (
            ParsedLine {
                kind: BlockKind::HorizontalRule,
                block_marker: Some((0, line.len())),
                content_range: (line.len(), line.len()),
                spans: Vec::new(),
            },
            false,
        );
    }

    // Heading: 1–6 '#' followed by a space.
    let hashes: usize = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
        let marker_end = indent + hashes + 1;
        return (
            ParsedLine {
                kind: BlockKind::Heading {
                    level: hashes as u8,
                },
                block_marker: Some((0, marker_end)),
                content_range: (marker_end, line.len()),
                spans: parse_inline(line, marker_end, line.len()),
            },
            false,
        );
    }

    // Quote: '>' at the start.
    if trimmed.starts_with('>') {
        let marker_end = indent + 1 + usize::from(trimmed[1..].starts_with(' '));
        return (
            ParsedLine {
                kind: BlockKind::Quote,
                block_marker: Some((0, marker_end)),
                content_range: (marker_end, line.len()),
                spans: parse_inline(line, marker_end, line.len()),
            },
            false,
        );
    }

    // List item: '-', '*', '+' or '1.' followed by a space.
    let (list_marker_end, checked) = list_item_prefix(line, indent);
    if let Some(marker_end) = list_marker_end {
        return (
            ParsedLine {
                kind: BlockKind::ListItem { checked },
                block_marker: Some((0, marker_end)),
                content_range: (marker_end, line.len()),
                spans: parse_inline(line, marker_end, line.len()),
            },
            false,
        );
    }

    (
        ParsedLine {
            kind: BlockKind::Paragraph,
            block_marker: None,
            content_range: (0, line.len()),
            spans: parse_inline(line, 0, line.len()),
        },
        false,
    )
}

/// Find a list-item prefix; returns (marker end, checkbox state).
fn list_item_prefix(line: &str, indent: usize) -> (Option<usize>, Option<bool>) {
    let rest = &line[indent..];
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return (None, None);
    };
    let mut marker_len = match first {
        '-' | '*' | '+' => 1,
        '0'..='9' => {
            let digits: usize = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            if rest[digits..].starts_with(". ") {
                digits + 1
            } else {
                return (None, None);
            }
        }
        _ => return (None, None),
    };
    if !rest[marker_len..].starts_with(' ') {
        return (None, None);
    }
    marker_len += 1; // the space after the bullet
                     // Checkbox: '[ ] ' or '[x] '.
    let after = &rest[marker_len..];
    if after.len() >= 4 && after.starts_with('[') && after[2..].starts_with("] ") {
        let checked = match after.chars().nth(1) {
            Some('x') | Some('X') => Some(true),
            Some(' ') => Some(false),
            _ => None,
        };
        if checked.is_some() {
            return (Some(indent + marker_len + 4), checked);
        }
    }
    (Some(indent + marker_len), None)
}

/// Parse bold / italic / code / link spans inside one content range.
/// Markers are reported separately so live preview can hide them.
fn parse_inline(line: &str, from: usize, to: usize) -> Vec<InlineSpan> {
    let mut spans: Vec<InlineSpan> = Vec::new();
    let bytes = line.as_bytes();
    let mut cursor = from;
    let mut plain_start = from;
    let push_plain = |spans: &mut Vec<InlineSpan>, start: usize, end: usize| {
        if end > start {
            spans.push(InlineSpan {
                range: (start, end),
                opening_marker: None,
                closing_marker: None,
                style: SpanStyle::Normal,
            });
        }
    };
    while cursor < to {
        // Bold: **text** (needs a closing **).
        if line[cursor..to].starts_with("**") {
            if let Some(close) = find_closing(line, cursor + 2, to, "**") {
                push_plain(&mut spans, plain_start, cursor);
                spans.push(InlineSpan {
                    range: (cursor + 2, close),
                    opening_marker: Some((cursor, cursor + 2)),
                    closing_marker: Some((close, close + 2)),
                    style: SpanStyle::Bold,
                });
                cursor = close + 2;
                plain_start = cursor;
                continue;
            }
        }
        // Code span: `text`.
        if bytes[cursor] == b'`' {
            if let Some(close) = find_closing(line, cursor + 1, to, "`") {
                push_plain(&mut spans, plain_start, cursor);
                spans.push(InlineSpan {
                    range: (cursor + 1, close),
                    opening_marker: Some((cursor, cursor + 1)),
                    closing_marker: Some((close, close + 1)),
                    style: SpanStyle::Code,
                });
                cursor = close + 1;
                plain_start = cursor;
                continue;
            }
        }
        // Italic: *text* (single asterisk, not part of **).
        if bytes[cursor] == b'*' && !line[cursor..to].starts_with("**") {
            if let Some(close) = find_closing(line, cursor + 1, to, "*") {
                // Do not treat the second * of a ** pair as italic close.
                if close + 1 <= to && line[close..to].starts_with("**") {
                    cursor += 1;
                    continue;
                }
                push_plain(&mut spans, plain_start, cursor);
                spans.push(InlineSpan {
                    range: (cursor + 1, close),
                    opening_marker: Some((cursor, cursor + 1)),
                    closing_marker: Some((close, close + 1)),
                    style: SpanStyle::Italic,
                });
                cursor = close + 1;
                plain_start = cursor;
                continue;
            }
        }
        // Link: [text](url) — visible part is the text.
        if bytes[cursor] == b'[' {
            if let Some((text_end, url_end)) = find_link(line, cursor, to) {
                push_plain(&mut spans, plain_start, cursor);
                spans.push(InlineSpan {
                    range: (cursor + 1, text_end),
                    opening_marker: Some((cursor, cursor + 1)),
                    closing_marker: Some((text_end, url_end)),
                    style: SpanStyle::Link,
                });
                cursor = url_end;
                plain_start = cursor;
                continue;
            }
        }
        cursor += 1;
        // Advance by one UTF-8 character, not one byte.
        while cursor < to && !line.is_char_boundary(cursor) {
            cursor += 1;
        }
    }
    push_plain(&mut spans, plain_start, to);
    spans
}

/// Find `marker` at or after `from`, returning its start byte, or `None`.
fn find_closing(line: &str, from: usize, to: usize, marker: &str) -> Option<usize> {
    if from >= to {
        return None;
    }
    line[from..to].find(marker).map(|offset| from + offset)
}

/// Find `[text](url)` starting at `open` (`[`); returns (text end, url end).
fn find_link(line: &str, open: usize, to: usize) -> Option<(usize, usize)> {
    let text_end = line[open..to].find(']')? + open;
    if !line[text_end..to].starts_with("](") {
        return None;
    }
    let url_end = line[text_end + 2..to].find(')')? + text_end + 2 + 1;
    Some((text_end, url_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_and_bold_markers() {
        let (parsed, _) = parse_line("## Hello **world**", false);
        assert_eq!(parsed.kind, BlockKind::Heading { level: 2 });
        assert_eq!(parsed.block_marker, Some((0, 3)));
        assert_eq!(parsed.content_range, (3, 18));
        let bold = parsed
            .spans
            .iter()
            .find(|span| span.style == SpanStyle::Bold)
            .expect("bold span");
        assert_eq!(&"## Hello **world**"[bold.range.0..bold.range.1], "world");
        assert_eq!(bold.opening_marker, Some((9, 11)));
        assert_eq!(bold.closing_marker, Some((16, 18)));
    }

    #[test]
    fn checkbox_list_item() {
        let (parsed, _) = parse_line("- [x] Done task", false);
        assert_eq!(
            parsed.kind,
            BlockKind::ListItem {
                checked: Some(true)
            }
        );
        assert_eq!(parsed.content_range.0, 6);
    }

    #[test]
    fn fenced_code_block_state() {
        let (opened, still_open) = parse_line("```rust", false);
        assert_eq!(opened.kind, BlockKind::CodeBlock);
        assert!(still_open);
        let (inside, still_open) = parse_line("let x = 1;", true);
        assert_eq!(inside.kind, BlockKind::CodeBlock);
        assert!(still_open);
        let (_, still_open) = parse_line("```", true);
        assert!(!still_open);
    }

    #[test]
    fn japanese_text_parses_cleanly() {
        let (parsed, _) = parse_line("# こんにちは **世界**", false);
        assert_eq!(parsed.kind, BlockKind::Heading { level: 1 });
        let bold = parsed
            .spans
            .iter()
            .find(|span| span.style == SpanStyle::Bold)
            .expect("bold span");
        assert_eq!(&"# こんにちは **世界**"[bold.range.0..bold.range.1], "世界");
    }
}
