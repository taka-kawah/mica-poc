//! GPUI view for the Mica PoC.
//!
//! One window: file-tree pane, tab bar, a multi-line editor that
//! renders `mica-shared-core` live-preview decorations as styled text
//! runs, a status bar, and a command palette / Quick Open overlay.
//!
//! License boundary (see `docs/research-2026-11.md` §8, read twice
//! before writing this file): this code depends only on the
//! Apache-2.0 `gpui-pre` family. It does NOT copy, vendor, or depend
//! on any Zed editor crate (`editor`, `text`, `rope`, `language`,
//! `multi_buffer`, `workspace`, `ui`, … — all GPL-3.0). The text
//! input handler and the custom text element below are written from
//! the concepts in GPUI's own Apache-2.0 `examples/input.rs` sample
//! (a single-line input) and from the shared-core decoration model
//! (shaped-line cache per paragraph, display mapping) — design-only
//! reference, no Zed editor code.

use std::ops::Range;
use std::path::PathBuf;

use gpui::{
    actions, div, fill, font, point, prelude::*, px, relative, rgb, size, white, App, Bounds,
    Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Focusable, GlobalElementId, InspectorElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, PaintQuad, Pixels, Point, Render, ShapedLine, Style, TextAlign, TextRun,
    UTF16Selection, UnderlineStyle, Window,
};
use mica_shared_core::auto_save::SaveState;
use mica_shared_core::live_preview::{DisplayLine, DisplaySegment, DisplayStyle};
use mica_shared_core::text_buffer::CursorPosition;

use crate::model::{
    cursor_for_utf16_offset, document_text, utf16_offset_for_cursor, EditorModel, AUTO_SAVE_WAIT,
};

/// Base body font size (Notion §7: `font_size = 16`).
const BASE_FONT_SIZE: f32 = 16.0;
/// Body line height factor (Notion §7: `line_height = 1.8`).
const LINE_HEIGHT_FACTOR: f32 = 1.8;
/// Centred body max width (Notion §7: `max_width = 720`).
const MAX_BODY_WIDTH: f32 = 720.0;

actions!(
    mica,
    [
        QuickOpen,
        CommandPalette,
        TabClose,
        TabNext,
        TabPrevious,
        ToggleFileTree,
        FocusCycle,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        MoveHome,
        MoveEnd,
        Backspace,
        Delete,
        Confirm,
        Escape,
        SaveNow,
        Quit,
    ]
);

/// Which pane the keyboard is driving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusArea {
    Editor,
    FileTree,
}

/// One shaped editor line, kept for click-to-cursor and for the IME
/// input handler (`bounds_for_range`, `character_index_for_point`).
#[derive(Clone)]
struct ShapedEditorLine {
    /// Top offset of the line inside the editor element, in pixels.
    top: Pixels,
    /// Height of the line, in pixels.
    height: Pixels,
    shaped: ShapedLine,
    /// The shared-core display line this was shaped from.
    display: DisplayLine,
}

/// The root view. It owns the headless [`EditorModel`] and implements
/// [`EntityInputHandler`] so GPUI routes keyboard / IME text into the
/// active buffer (or into the palette query while the palette is open).
pub struct MicaView {
    pub model: EditorModel,
    focus_handle: FocusHandle,
    focus_area: FocusArea,
    palette_open: bool,
    palette_commands: bool,
    palette_query: String,
    palette_selected: usize,
    /// Active IME composition range, as UTF-16 offsets over the
    /// active document text (or over the palette query).
    marked_range: Option<Range<usize>>,
    /// Last editor layout, for click handling and IME bounds.
    editor_bounds: Option<Bounds<Pixels>>,
    editor_lines: Vec<ShapedEditorLine>,
}

impl MicaView {
    pub fn new(model: EditorModel, cx: &mut Context<Self>) -> Self {
        Self {
            model,
            focus_handle: cx.focus_handle(),
            focus_area: FocusArea::Editor,
            palette_open: false,
            palette_commands: false,
            palette_query: String::new(),
            palette_selected: 0,
            marked_range: None,
            editor_bounds: None,
            editor_lines: Vec::new(),
        }
    }

    // -- model helpers -------------------------------------------------

    fn active_buffer_text(&self) -> String {
        self.model
            .active_buffer()
            .map(document_text)
            .unwrap_or_default()
    }

    /// The text the input handler currently edits: the palette query
    /// while the palette is open, else the active document.
    fn input_text(&self) -> String {
        if self.palette_open {
            self.palette_query.clone()
        } else {
            self.active_buffer_text()
        }
    }

    /// Cursor (UTF-16 offset) of the input handler's selection.
    fn input_cursor_utf16(&self) -> usize {
        if self.palette_open {
            self.palette_query.encode_utf16().count()
        } else {
            self.model
                .active_buffer()
                .map(|buffer| utf16_offset_for_cursor(buffer, self.model.cursor()))
                .unwrap_or(0)
        }
    }

    fn note_edit_and_schedule_auto_save(&mut self, cx: &mut Context<Self>) {
        self.model.note_edit();
        self.schedule_auto_save(cx);
        cx.notify();
    }

    /// Spawn a background timer; when it fires, save if the debounce
    /// delay has passed. A fresh edit restarts the shared-core timer,
    /// so a stale task simply finds nothing due and does nothing.
    fn schedule_auto_save(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |entity, cx| {
            cx.background_executor().timer(AUTO_SAVE_WAIT).await;
            let _ = entity.update(cx, |view, cx| {
                view.model.maybe_auto_save();
                cx.notify();
            });
        });
        task.detach();
    }

    /// Save right now (Ctrl+S / the `save` command).
    fn save_now(&mut self, _: &SaveNow, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.save_now();
        cx.notify();
    }

    // -- palette -------------------------------------------------------

    /// Open the palette (Quick Open or command palette). The editor
    /// keeps keyboard focus: the input handler routes typed text to
    /// the palette query while it is open (see `replace_text_in_range`),
    /// so the palette needs no second focus handle. (A separate
    /// palette focus handle was tried first: with no input handler
    /// registered for it, typed text was silently dropped.)
    fn open_palette(&mut self, commands: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = true;
        self.palette_commands = commands;
        self.palette_query.clear();
        self.palette_selected = 0;
        self.marked_range = None;
        cx.notify();
    }

    fn close_palette(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = false;
        self.palette_query.clear();
        self.marked_range = None;
        cx.notify();
    }

    fn palette_results(&self) -> Vec<mica_shared_core::command_palette::PaletteEntry> {
        self.model
            .filtered_palette_entries(self.palette_commands, &self.palette_query)
    }

    fn run_palette_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self
            .palette_results()
            .into_iter()
            .nth(self.palette_selected)
        else {
            return;
        };
        let action = entry.action.clone();
        if self.palette_commands {
            self.close_palette(window, cx);
            self.run_command(&action, window, cx);
        } else {
            let path = PathBuf::from(&action);
            self.close_palette(window, cx);
            if self.model.open_file(&path).is_ok() {
                self.focus_area = FocusArea::Editor;
                cx.notify();
            }
        }
    }

    /// Run a command by name (from the palette or the keymap).
    fn run_command(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            "quick_open" => self.open_palette(false, window, cx),
            "command_palette" => self.open_palette(true, window, cx),
            "toggle_file_tree" => {
                self.model.show_tree = !self.model.show_tree;
                cx.notify();
            }
            "tab_close" => {
                if let Some(active) = self.model.tabs.active_tab().map(|tab| tab.path.clone()) {
                    let index = self
                        .model
                        .tabs
                        .tabs()
                        .iter()
                        .position(|tab| tab.path == active);
                    if let Some(index) = index {
                        self.model.tabs.close(index);
                        cx.notify();
                    }
                }
            }
            "tab_recent_next" | "tab_next" => {
                self.model.tabs.focus_next();
                cx.notify();
            }
            "tab_previous" => {
                self.model.tabs.focus_previous();
                cx.notify();
            }
            "save" => {
                self.model.save_now();
                cx.notify();
            }
            "open_folder" => {
                // The PoC opens its folder from the command line; there
                // is no folder picker yet.
                cx.notify();
            }
            _ => {}
        }
    }

    // -- actions --------------------------------------------------------

    fn on_quick_open(&mut self, _: &QuickOpen, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette(false, window, cx);
    }

    fn on_command_palette(
        &mut self,
        _: &CommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_palette(true, window, cx);
    }

    fn on_toggle_tree(&mut self, _: &ToggleFileTree, _: &mut Window, cx: &mut Context<Self>) {
        self.model.show_tree = !self.model.show_tree;
        cx.notify();
    }

    fn on_focus_cycle(&mut self, _: &FocusCycle, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_area = match self.focus_area {
            FocusArea::Editor => FocusArea::FileTree,
            FocusArea::FileTree => FocusArea::Editor,
        };
        cx.notify();
        let _ = window;
    }

    fn on_tab_close(&mut self, _: &TabClose, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.model.tabs.active_tab().and_then(|active| {
            self.model
                .tabs
                .tabs()
                .iter()
                .position(|tab| tab.path == active.path)
        }) {
            self.model.tabs.close(index);
        }
        cx.notify();
    }

    fn on_tab_next(&mut self, _: &TabNext, _: &mut Window, cx: &mut Context<Self>) {
        self.model.tabs.focus_next();
        cx.notify();
    }

    fn on_tab_previous(&mut self, _: &TabPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.model.tabs.focus_previous();
        cx.notify();
    }

    fn on_move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            return;
        }
        match self.focus_area {
            FocusArea::Editor => self.model.move_cursor(0, -1),
            FocusArea::FileTree => {}
        }
        cx.notify();
    }

    fn on_move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            return;
        }
        if self.focus_area == FocusArea::Editor {
            self.model.move_cursor(0, 1);
        }
        cx.notify();
    }

    fn on_move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            if self.palette_selected > 0 {
                self.palette_selected -= 1;
            }
            cx.notify();
            return;
        }
        match self.focus_area {
            FocusArea::Editor => self.model.move_cursor(-1, 0),
            FocusArea::FileTree => {
                self.model.tree_selection = self.model.tree_selection.saturating_sub(1);
            }
        }
        cx.notify();
    }

    fn on_move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            let count = self.palette_results().len();
            if self.palette_selected + 1 < count {
                self.palette_selected += 1;
            }
            cx.notify();
            return;
        }
        match self.focus_area {
            FocusArea::Editor => self.model.move_cursor(1, 0),
            FocusArea::FileTree => {
                let max = self.model.tree.len().saturating_sub(1);
                if self.model.tree_selection < max {
                    self.model.tree_selection += 1;
                }
            }
        }
        cx.notify();
    }

    fn on_move_home(&mut self, _: &MoveHome, _: &mut Window, cx: &mut Context<Self>) {
        if self.focus_area == FocusArea::Editor {
            self.model.move_cursor_to_line_edge(false);
        }
        cx.notify();
    }

    fn on_move_end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        if self.focus_area == FocusArea::Editor {
            self.model.move_cursor_to_line_edge(true);
        }
        cx.notify();
    }

    fn on_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            self.palette_query.pop();
            self.palette_selected = 0;
            cx.notify();
            return;
        }
        self.model.delete_before_cursor();
        self.note_edit_and_schedule_auto_save(cx);
    }

    fn on_delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            return;
        }
        self.model.delete_after_cursor();
        self.note_edit_and_schedule_auto_save(cx);
    }

    fn on_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            self.run_palette_selection(window, cx);
            return;
        }
        if self.focus_area == FocusArea::FileTree {
            if let Some(entry) = self.model.tree.get(self.model.tree_selection).cloned() {
                if entry.is_markdown {
                    let _ = self.model.open_file(&entry.path);
                    self.focus_area = FocusArea::Editor;
                }
            }
            cx.notify();
            return;
        }
        // Enter in the editor inserts a newline at the cursor.
        self.model.insert_at_cursor("\n");
        self.note_edit_and_schedule_auto_save(cx);
    }

    fn on_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            self.close_palette(window, cx);
        }
    }

    fn on_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn on_tree_click(
        &mut self,
        index: usize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.model.tree.get(index).cloned() {
            self.model.tree_selection = index;
            if entry.is_markdown {
                let _ = self.model.open_file(&entry.path);
                self.focus_area = FocusArea::Editor;
            }
        }
        cx.notify();
    }

    fn on_tab_click(
        &mut self,
        index: usize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.tabs.focus(index);
        cx.notify();
    }

    fn on_editor_click(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.focus_area = FocusArea::Editor;
        let Some(bounds) = self.editor_bounds else {
            return;
        };
        let Some(local) = bounds.localize(&event.position) else {
            return;
        };
        // Find the line under the click.
        let mut line_index = self.editor_lines.len().saturating_sub(1);
        for (index, line) in self.editor_lines.iter().enumerate() {
            if local.y < line.top + line.height {
                line_index = index;
                break;
            }
        }
        let Some(line) = self.editor_lines.get(line_index) else {
            return;
        };
        let byte_index = line.shaped.closest_index_for_x(local.x);
        let display_column = line.shaped.text[..byte_index.min(line.shaped.text.len())]
            .chars()
            .count();
        let buffer_column = line.display.buffer_column_for_display(display_column);
        self.model.set_cursor(CursorPosition {
            line: line_index,
            column: buffer_column,
        });
        cx.notify();
    }
}

impl Focusable for MicaView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Text colour for a display segment: dimmed markers are grey, code
/// keeps the body colour, everything else is body text.
fn segment_color(segment: &DisplaySegment) -> gpui::Hsla {
    if segment.style.dimmed {
        rgb(0x999999).into()
    } else {
        rgb(0x222222).into()
    }
}

/// Build the [`TextRun`]s for one display line from its segments.
/// The marked (IME composition) range, when it overlaps this line, is
/// underlined — the same visual cue the GPUI input sample uses.
fn runs_for_line(
    display: &DisplayLine,
    line_text: &str,
    marked_char_range: Option<Range<usize>>,
) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut byte_offset = 0;
    for segment in &display.segments {
        // NOTE: ".SystemUIFont" resolves on Linux to the system sans,
        // but in this GPUI snapshot its bold/italic variants did not
        // render (see README). The explicit family below renders
        // bold/italic correctly with the installed Noto Sans faces.
        let mut run_font = font("Noto Sans");
        if segment.style.monospace {
            run_font = font("monospace");
        }
        if segment.style.bold {
            run_font = run_font.bold();
        }
        if segment.style.italic {
            run_font = run_font.italic();
        }
        let segment_char_start = line_text[..byte_offset.min(line_text.len())]
            .chars()
            .count();
        let segment_char_end = segment_char_start + segment.text.chars().count();
        let underline = match &marked_char_range {
            Some(marked) if segment_char_start < marked.end && segment_char_end > marked.start => {
                Some(UnderlineStyle {
                    color: Some(segment_color(segment)),
                    thickness: px(1.0),
                    wavy: false,
                })
            }
            _ => None,
        };
        runs.push(TextRun {
            len: segment.text.len(),
            font: run_font,
            color: segment_color(segment),
            background_color: None,
            underline,
            strikethrough: None,
        });
        byte_offset += segment.text.len();
    }
    runs
}

impl Render for MicaView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Auto-save check on every render, so a save also happens if
        // the timer task fired while the window was busy.
        self.model.maybe_auto_save();

        let save_label = match self.model.save_state() {
            SaveState::Clean => "Saved",
            SaveState::Dirty => "Unsaved changes…",
            SaveState::Saving => "Saving…",
            SaveState::Saved => "Saved",
        };
        let cursor = self.model.cursor();
        let active_title = self
            .model
            .tabs
            .active_tab()
            .map(|tab| tab.title.clone())
            .unwrap_or_else(|| "No file open".to_string());
        let status = format!(
            "{}  ·  {active_title}  ·  {save_label}  ·  line {}, column {}  ·  {}",
            self.model.folder.display(),
            cursor.line + 1,
            cursor.column + 1,
            match self.focus_area {
                FocusArea::Editor => "editor",
                FocusArea::FileTree => "file tree (F6 to switch)",
            }
        );

        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0xfafaf8))
            .text_color(rgb(0x222222))
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::on_quick_open))
            .on_action(cx.listener(Self::on_command_palette))
            .on_action(cx.listener(Self::on_toggle_tree))
            .on_action(cx.listener(Self::on_focus_cycle))
            .on_action(cx.listener(Self::on_tab_close))
            .on_action(cx.listener(Self::on_tab_next))
            .on_action(cx.listener(Self::on_tab_previous))
            .on_action(cx.listener(Self::on_move_left))
            .on_action(cx.listener(Self::on_move_right))
            .on_action(cx.listener(Self::on_move_up))
            .on_action(cx.listener(Self::on_move_down))
            .on_action(cx.listener(Self::on_move_home))
            .on_action(cx.listener(Self::on_move_end))
            .on_action(cx.listener(Self::on_backspace))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_confirm))
            .on_action(cx.listener(Self::on_escape))
            .on_action(cx.listener(Self::save_now))
            .on_action(cx.listener(Self::on_quit));

        // Command palette / Quick Open overlay (in flow, above the tab bar).
        if self.palette_open {
            let results = self.palette_results();
            let query_display = if self.palette_query.is_empty() {
                if self.palette_commands {
                    "Type a command…".to_string()
                } else {
                    "Type a file name…".to_string()
                }
            } else {
                format!("{}▏", self.palette_query)
            };
            let mut palette = div()
                .flex()
                .flex_col()
                .bg(white())
                .border_1()
                .border_color(rgb(0xcccccc))
                .m(px(8.))
                .p(px(8.))
                .gap_1()
                .child(div().text_size(px(16.)).child(query_display));
            for (index, entry) in results.iter().take(8).enumerate() {
                let selected = index == self.palette_selected;
                let row = div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .px_2()
                    .py(px(2.))
                    .when(selected, |row| row.bg(rgb(0xdde8ff)))
                    .child(entry.title.clone())
                    .child(
                        div()
                            .text_color(rgb(0x888888))
                            .text_size(px(12.))
                            .child(entry.detail.clone()),
                    );
                palette = palette.child(row);
            }
            root = root.child(palette);
        }

        // Tab bar.
        let mut tab_bar = div()
            .flex()
            .flex_row()
            .bg(rgb(0xeeeeec))
            .border_b_1()
            .border_color(rgb(0xdddddd));
        for (index, tab) in self.model.tabs.tabs().iter().enumerate() {
            let is_active = self
                .model
                .tabs
                .active_tab()
                .map(|active| active.path == tab.path)
                .unwrap_or(false);
            let label = if tab.dirty {
                format!("{} •", tab.title)
            } else {
                tab.title.clone()
            };
            tab_bar = tab_bar.child(
                div()
                    .px_3()
                    .py(px(6.))
                    .when(is_active, |tab| tab.bg(white()))
                    .child(label)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, event, window, cx| {
                            view.on_tab_click(index, event, window, cx);
                        }),
                    ),
            );
        }
        root = root.child(tab_bar);

        // Middle row: file tree + editor.
        let mut middle = div().flex().flex_row().flex_1();

        if self.model.show_tree {
            let mut tree = div()
                .id("file-tree")
                .flex()
                .flex_col()
                .w(px(240.))
                .bg(rgb(0xf0f0ee))
                .border_r_1()
                .border_color(rgb(0xdddddd))
                .overflow_y_scroll()
                .py(px(4.));
            for (index, entry) in self.model.tree.iter().enumerate() {
                let selected = index == self.model.tree_selection;
                let label = format!(
                    "{}{}",
                    "  ".repeat(entry.depth),
                    if entry.is_folder {
                        format!("{}/", entry.name)
                    } else {
                        entry.name.clone()
                    }
                );
                tree = tree.child(
                    div()
                        .px_2()
                        .py(px(2.))
                        .text_size(px(14.))
                        .when(selected, |row| row.bg(rgb(0xdde8ff)))
                        .child(label)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |view, event, window, cx| {
                                view.on_tree_click(index, event, window, cx);
                            }),
                        ),
                );
            }
            middle = middle.child(tree);
        }

        // Editor: centred body, max 720 px wide.
        let editor = div()
            .id("editor-scroll")
            .flex()
            .flex_1()
            .justify_center()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(MAX_BODY_WIDTH))
                    .px(px(24.))
                    .py(px(16.))
                    .child(EditorElement { view: cx.entity() })
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_editor_click)),
            );
        middle = middle.child(editor);
        root = root.child(middle);

        // Status bar.
        root = root.child(
            div()
                .flex()
                .flex_row()
                .bg(rgb(0xeeeeec))
                .border_t_1()
                .border_color(rgb(0xdddddd))
                .px_3()
                .py(px(4.))
                .text_size(px(13.))
                .text_color(rgb(0x666666))
                .child(status),
        );

        root
    }
}

// -- EntityInputHandler -------------------------------------------------
//
// The input handler edits the whole active document as one string
// (UTF-16 offsets, as the platform expects) and translates to/from
// the shared-core buffer positions in the model. While the palette
// is open, the same handler edits the palette query instead, so the
// palette is a real text input too.

impl EntityInputHandler for MicaView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.input_text();
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let start = range_utf16.start.min(utf16.len());
        let end = range_utf16.end.min(utf16.len());
        adjusted_range.replace(start..end);
        Some(String::from_utf16_lossy(&utf16[start..end]))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let cursor = self.input_cursor_utf16();
        Some(UTF16Selection {
            range: cursor..cursor,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette_open {
            let query = &mut self.palette_query;
            let utf16: Vec<u16> = query.encode_utf16().collect();
            let (start, end) = range_utf16
                .map(|range| (range.start.min(utf16.len()), range.end.min(utf16.len())))
                .unwrap_or((utf16.len(), utf16.len()));
            let mut chars: Vec<char> = query.chars().collect();
            let char_start = utf16[..start]
                .iter()
                .filter(|unit| (**unit & 0xFC00) != 0xDC00)
                .count();
            let char_end = utf16[..end]
                .iter()
                .filter(|unit| (**unit & 0xFC00) != 0xDC00)
                .count();
            chars.splice(char_start..char_end, text.chars());
            *query = chars.into_iter().collect();
            self.palette_selected = 0;
            self.marked_range = None;
            cx.notify();
            return;
        }
        self.model
            .replace_text_in_range(range_utf16.map(|range| (range.start, range.end)), text);
        self.marked_range = None;
        self.note_edit_and_schedule_auto_save(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette_open {
            self.replace_text_in_range(range_utf16, new_text, _window, cx);
            return;
        }
        let Some(buffer) = self.model.active_buffer() else {
            return;
        };
        let buffer = buffer.clone();
        let start_utf16 = range_utf16
            .as_ref()
            .map(|range| range.start)
            .unwrap_or_else(|| utf16_offset_for_cursor(&buffer, self.model.cursor()));
        self.model
            .replace_text_in_range(range_utf16.map(|range| (range.start, range.end)), new_text);
        if !new_text.is_empty() {
            let end_utf16 = start_utf16 + new_text.encode_utf16().count();
            self.marked_range = Some(start_utf16..end_utf16);
        } else {
            self.marked_range = None;
        }
        if let Some(selected) = new_selected_range {
            let end_utf16 = start_utf16 + selected.end;
            if let Some(buffer) = self.model.active_buffer() {
                let cursor = cursor_for_utf16_offset(buffer, end_utf16);
                self.model.set_cursor(cursor);
            }
        }
        self.note_edit_and_schedule_auto_save(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Candidate-window position for the IME: the bounds of the
        // range inside the shaped line that contains it. (On Windows
        // this is the known open GPUI issue #56149; untested here.)
        let Some(buffer) = self.model.active_buffer().cloned() else {
            return None;
        };
        let cursor = cursor_for_utf16_offset(&buffer, range_utf16.start);
        let line = self.editor_lines.get(cursor.line)?;
        let display_column = line.display.display_column_for_buffer(cursor.column);
        let byte_index = line
            .shaped
            .text
            .char_indices()
            .nth(display_column)
            .map(|(byte, _)| byte)
            .unwrap_or(line.shaped.text.len());
        let x = line.shaped.x_for_index(byte_index);
        let top = element_bounds.origin.y + line.top;
        Some(Bounds::from_corners(
            point(element_bounds.origin.x + x, top),
            point(element_bounds.origin.x + x + px(2.), top + line.height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.editor_bounds?;
        let local = bounds.localize(&point)?;
        let mut line_index = self.editor_lines.len().saturating_sub(1);
        for (index, line) in self.editor_lines.iter().enumerate() {
            if local.y < line.top + line.height {
                line_index = index;
                break;
            }
        }
        let line = self.editor_lines.get(line_index)?;
        let byte_index = line.shaped.closest_index_for_x(local.x);
        let display_column = line.shaped.text[..byte_index.min(line.shaped.text.len())]
            .chars()
            .count();
        let buffer_column = line.display.buffer_column_for_display(display_column);
        let buffer = self.model.active_buffer()?;
        Some(utf16_offset_for_cursor(
            buffer,
            CursorPosition {
                line: line_index,
                column: buffer_column,
            },
        ))
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.input_text().encode_utf16().count())
    }
}

// -- Editor element ------------------------------------------------------
//
// A custom element that shapes one line per buffer line (a shaped-line
// cache per paragraph, invalidated by re-shaping on every render —
// the PoC documents are small) and paints the live-preview runs, the
// cursor, and registers the input handler. This is the GPUI-side half
// of the Notion child page's stages 3–5; stages 1–2 and the coordinate
// mapping live in shared-core (`live_preview`).

struct EditorElement {
    view: Entity<MicaView>,
}

struct EditorPrepaint {
    lines: Vec<ShapedEditorLine>,
    cursor_quad: Option<PaintQuad>,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = EditorPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let height = self.view.read(cx).editor_total_height();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = height.into();
        let _ = window;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let view = self.view.read(cx);
        let display_lines = view.model.display_lines();
        let cursor = view.model.cursor();
        let marked_char_range = view.marked_char_range_on_line();

        let mut lines = Vec::with_capacity(display_lines.len());
        let mut cursor_quad = None;
        let mut top = px(0.);
        for (index, display) in display_lines.iter().enumerate() {
            // Fallback for lines the decoration layer drops: fenced
            // code-block content lines have no spans in shared-core
            // (`parse_line` returns `spans: []` for them), so their
            // display line is empty. Render the raw buffer line as one
            // monospace segment instead of a blank line. This is a
            // rendering fallback in the view, not a re-implementation
            // of the parser: it only triggers when the display line is
            // empty but the buffer line is not.
            let display = if display.display_text.is_empty() {
                match view
                    .model
                    .active_buffer()
                    .and_then(|buffer| buffer.line(index))
                {
                    Some(raw) if !raw.is_empty() => {
                        let char_count = raw.chars().count();
                        std::borrow::Cow::Owned(DisplayLine {
                            segments: vec![DisplaySegment {
                                text: raw.to_string(),
                                style: DisplayStyle {
                                    monospace: true,
                                    ..DisplayStyle::default()
                                },
                                buffer_range: (0, raw.len()),
                            }],
                            display_text: raw.to_string(),
                            display_to_buffer_columns: (0..=char_count).collect(),
                            is_cursor_line: display.is_cursor_line,
                            line_height_factor: 1.0,
                        })
                    }
                    _ => std::borrow::Cow::Borrowed(display),
                }
            } else {
                std::borrow::Cow::Borrowed(display)
            };
            let display = display.as_ref();
            let font_size = px(BASE_FONT_SIZE
                * display
                    .segments
                    .iter()
                    .fold(1.0f32, |max, segment| max.max(segment.style.size_factor)));
            let line_height = px(BASE_FONT_SIZE * LINE_HEIGHT_FACTOR * display.line_height_factor);
            let marked = marked_char_range.as_ref().and_then(|(line, range)| {
                if *line == index {
                    Some(range.clone())
                } else {
                    None
                }
            });
            let shaped = if display.display_text.is_empty() {
                // Shape a single space so the line still has a valid
                // layout for cursor placement and click handling.
                window.text_system().shape_line(
                    " ".into(),
                    font_size,
                    &[TextRun {
                        len: 1,
                        font: font(".SystemUIFont"),
                        color: rgb(0x222222).into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                )
            } else {
                let runs = runs_for_line(display, &display.display_text, marked);
                window.text_system().shape_line(
                    display.display_text.clone().into(),
                    font_size,
                    &runs,
                    None,
                )
            };
            if index == cursor.line {
                let display_column = display.display_column_for_buffer(cursor.column);
                let byte_index = display
                    .display_text
                    .char_indices()
                    .nth(display_column)
                    .map(|(byte, _)| byte)
                    .unwrap_or(display.display_text.len());
                let x = if display.display_text.is_empty() {
                    px(0.)
                } else {
                    shaped.x_for_index(byte_index)
                };
                cursor_quad = Some(fill(
                    Bounds::new(
                        point(bounds.origin.x + x, bounds.origin.y + top),
                        size(px(2.), line_height),
                    ),
                    rgb(0x222222),
                ));
            }
            lines.push(ShapedEditorLine {
                top,
                height: line_height,
                shaped,
                display: display.clone(),
            });
            top += line_height;
        }
        EditorPrepaint { lines, cursor_quad }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.view.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        for line in &prepaint.lines {
            if line.display.display_text.is_empty() {
                continue;
            }
            let origin = point(bounds.origin.x, bounds.origin.y + line.top);
            line.shaped
                .paint(origin, line.height, TextAlign::Left, None, window, cx)
                .unwrap_or(());
        }
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor_quad.take() {
                window.paint_quad(cursor);
            }
        }
        let lines = prepaint.lines.clone();
        self.view.update(cx, |view, _cx| {
            view.editor_bounds = Some(bounds);
            view.editor_lines = lines;
        });
    }
}

impl MicaView {
    /// Total editor height, for layout.
    fn editor_total_height(&self) -> Pixels {
        self.model
            .display_lines()
            .iter()
            .map(|line| px(BASE_FONT_SIZE * LINE_HEIGHT_FACTOR * line.line_height_factor))
            .fold(px(0.), |sum, height| sum + height)
    }

    /// The marked (IME composition) range as (line, char range on that
    /// line), if the composition sits on one line.
    fn marked_char_range_on_line(&self) -> Option<(usize, Range<usize>)> {
        let marked = self.marked_range.clone()?;
        let buffer = self.model.active_buffer()?;
        let start = cursor_for_utf16_offset(buffer, marked.start);
        let end = cursor_for_utf16_offset(buffer, marked.end);
        if start.line != end.line {
            return None;
        }
        Some((start.line, start.column..end.column))
    }
}

// -- keybindings ----------------------------------------------------------

/// Build GPUI key bindings from the loaded keymap's `[global]` table
/// (plus the editor keys this PoC always needs). GPUI keystrokes use
/// `-` separators (`ctrl-p`), the TOML file uses `+` (`ctrl+p`).
pub fn build_key_bindings() -> Vec<KeyBinding> {
    let bindings = vec![
        KeyBinding::new("backspace", Backspace, None),
        KeyBinding::new("delete", Delete, None),
        KeyBinding::new("left", MoveLeft, None),
        KeyBinding::new("right", MoveRight, None),
        KeyBinding::new("up", MoveUp, None),
        KeyBinding::new("down", MoveDown, None),
        KeyBinding::new("home", MoveHome, None),
        KeyBinding::new("end", MoveEnd, None),
        KeyBinding::new("enter", Confirm, None),
        KeyBinding::new("escape", Escape, None),
        KeyBinding::new("ctrl-s", SaveNow, None),
        KeyBinding::new("f6", FocusCycle, None),
        KeyBinding::new("ctrl-q", Quit, None),
    ];
    bindings
}

/// Map a keymap command name to the action it triggers, for the
/// `[global]` TOML table. Returns `None` for commands this PoC does
/// not wire to a key (they stay available from the palette).
fn action_for_command(command: &str) -> Option<Box<dyn gpui::Action>> {
    match command {
        "quick_open" => Some(Box::new(QuickOpen)),
        "command_palette" => Some(Box::new(CommandPalette)),
        "toggle_file_tree" => Some(Box::new(ToggleFileTree)),
        "tab_close" => Some(Box::new(TabClose)),
        "tab_recent_next" => Some(Box::new(TabNext)),
        "tab_previous" => Some(Box::new(TabPrevious)),
        "save" => Some(Box::new(SaveNow)),
        _ => None,
    }
}

/// Read the `[global]` table of the keymap TOML file and turn it into
/// GPUI key bindings, on top of the built-in editor bindings. Falls
/// back to the defaults in the shared core. `KeyBinding::load` is
/// used (rather than `KeyBinding::new`) because the keystrokes come
/// from a file at runtime, not from string literals.
pub fn key_bindings_from_keymap_file(cx: &App) -> Vec<KeyBinding> {
    let text = std::fs::read_to_string(crate::model::keymap_path())
        .unwrap_or_else(|_| mica_shared_core::keymap::DEFAULT_KEYMAP_TOML.to_string());
    let mut bindings = build_key_bindings();
    if let Ok(value) = text.parse::<toml::Value>() {
        if let Some(global) = value.get("global").and_then(|table| table.as_table()) {
            for (key, command) in global {
                let Some(command) = command.as_str() else {
                    continue;
                };
                let Some(action) = action_for_command(command) else {
                    continue;
                };
                // GPUI keystrokes use `-` separators (`ctrl-p`),
                // the TOML file uses `+` (`ctrl+p`).
                let keystroke = key.replace('+', "-");
                match KeyBinding::load(&keystroke, action, None, false, None, cx) {
                    Ok(binding) => bindings.push(binding),
                    Err(error) => eprintln!(
                        "mica-gpui: ignoring keymap binding {key:?} = {command:?}: {error}"
                    ),
                }
            }
        }
    }
    bindings
}
