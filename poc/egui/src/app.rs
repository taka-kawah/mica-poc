//! The egui app shell: file tree, tab bar, per-line live-preview
//! editor, status bar, command palette / Quick Open, global keymap.
//!
//! All file/live-preview logic lives in `mica-shared-core`; this file
//! is only the egui wiring (panels, widgets, keyboard dispatch).

use crate::document::OpenDocument;
use crate::editor_view::render_document;
use egui::{Align, Color32, Layout, RichText, Ui, Vec2};
use mica_shared_core::auto_save::SaveState;
use mica_shared_core::command_palette::{filter_entries, PaletteEntry};
use mica_shared_core::file_tree::{scan_folder, FileTreeEntry};
use mica_shared_core::keymap::{Keymap, KNOWN_COMMANDS};
use mica_shared_core::tabs::TabManager;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Which part of the UI owns plain (unmodified) keys like `j`/`k`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusArea {
    FileTree,
    Editor,
    Palette,
}

/// Which palette is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaletteMode {
    /// Ctrl+P: fuzzy search file names.
    QuickOpen,
    /// Ctrl+Shift+P: fuzzy search command names.
    Commands,
}

/// State of the open command palette / Quick Open overlay.
struct PaletteState {
    mode: PaletteMode,
    query: String,
    selected: usize,
    focus_query: bool,
}

/// The Mica egui application.
pub struct MicaApp {
    folder: PathBuf,
    tree_entries: Vec<FileTreeEntry>,
    collapsed_folders: HashSet<PathBuf>,
    tree_selected: usize,
    show_tree: bool,
    tabs: TabManager,
    documents: HashMap<PathBuf, OpenDocument>,
    keymap: Keymap,
    keymap_source: String,
    palette: Option<PaletteState>,
    focus_area: FocusArea,
    status_message: String,
    saved_since: Option<Instant>,
    started: Instant,
    quit_after: Option<Duration>,
}

impl MicaApp {
    /// Create the app for `folder`, loading the global keymap.
    pub fn new(folder: PathBuf, quit_after: Option<Duration>) -> Self {
        let (keymap, keymap_source) = load_keymap();
        let mut app = Self {
            folder: folder.clone(),
            tree_entries: scan_folder(&folder),
            collapsed_folders: HashSet::new(),
            tree_selected: 0,
            show_tree: true,
            tabs: TabManager::new(),
            documents: HashMap::new(),
            keymap,
            keymap_source,
            palette: None,
            focus_area: FocusArea::FileTree,
            status_message: format!("opened folder {}", folder.display()),
            saved_since: None,
            started: Instant::now(),
            quit_after,
        };
        // Open the first Markdown file so the window is not empty.
        if let Some(first) = app
            .tree_entries
            .iter()
            .find(|entry| entry.is_markdown)
            .map(|entry| entry.path.clone())
        {
            app.open_file(&first);
        }
        app
    }

    /// Apply egui style: proportional 16pt body, monospace code, dark.
    pub fn apply_style(ctx: &egui::Context) {
        ctx.set_visuals(egui::Visuals::dark());
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(16.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(24.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(15.0, egui::FontFamily::Monospace),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(12.0, egui::FontFamily::Proportional),
        );
        ctx.set_style_of(egui::Theme::Dark, style);
    }

    /// Open a file in a tab (focusing the existing tab when open).
    fn open_file(&mut self, path: &Path) {
        self.tabs.open_file(path);
        if !self.documents.contains_key(path) {
            let (document, error) = OpenDocument::load(path);
            if let Some(error) = error {
                self.status_message = error;
            }
            self.documents.insert(path.to_path_buf(), document);
        }
        if let Some(document) = self.documents.get_mut(path) {
            document.needs_focus = true;
        }
        self.focus_area = FocusArea::Editor;
        self.status_message = format!("opened {}", path.display());
    }

    /// Close the tab at `index`, dropping its document. A document
    /// with unsaved edits is saved first; when that save fails the tab
    /// stays open (edits kept in memory) and the error is shown in the
    /// status bar. Returns `true` when the tab was closed.
    fn close_tab(&mut self, index: usize) -> bool {
        let Some(path) = self.tabs.tabs().get(index).map(|tab| tab.path.clone()) else {
            return false;
        };
        if let Some(document) = self.documents.get_mut(&path) {
            if document.has_unsaved_changes() {
                if let Err(error) = document.save_now() {
                    self.status_message = format!(
                        "not closing {}: save failed: {error} (edits kept)",
                        path.display()
                    );
                    return false;
                }
            }
        }
        if let Some(closed) = self.tabs.close(index) {
            self.documents.remove(&closed.path);
            self.status_message = format!("closed {}", closed.title);
        }
        if let Some(active) = self.tabs.active_tab() {
            let path = active.path.clone();
            if let Some(document) = self.documents.get_mut(&path) {
                document.needs_focus = true;
            }
        }
        true
    }

    /// Save every document with unsaved edits right now (used on
    /// quit). Returns the error messages of the saves that failed.
    fn flush_all(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        for (path, document) in &mut self.documents {
            if !document.has_unsaved_changes() {
                continue;
            }
            match document.save_now() {
                Ok(()) => {
                    self.tabs.set_dirty(path, false);
                }
                Err(error) => errors.push(format!("{}: {error}", path.display())),
            }
        }
        errors
    }

    /// The active document, if any.
    fn active_document_mut(&mut self) -> Option<&mut OpenDocument> {
        let path = self.tabs.active_tab()?.path.clone();
        self.documents.get_mut(&path)
    }

    /// Visible tree entries (folders collapsed by the user are hidden
    /// along with their children).
    fn visible_tree_entries(&self) -> Vec<FileTreeEntry> {
        let mut visible = Vec::new();
        let mut hidden_below: Option<(PathBuf, usize)> = None;
        for entry in &self.tree_entries {
            if let Some((collapsed, depth)) = &hidden_below {
                if entry.path.starts_with(collapsed) && entry.depth > *depth {
                    continue;
                }
                hidden_below = None;
            }
            if entry.is_folder && self.collapsed_folders.contains(&entry.path) {
                hidden_below = Some((entry.path.clone(), entry.depth));
            }
            visible.push(entry.clone());
        }
        visible
    }

    /// Toggle the collapsed state of a folder entry.
    fn toggle_folder(&mut self, path: &Path) {
        if self.collapsed_folders.contains(path) {
            self.collapsed_folders.remove(path);
        } else {
            self.collapsed_folders.insert(path.to_path_buf());
        }
    }

    /// Run one command by name. Returns `true` when the command did
    /// something (so the palette can close after running it).
    fn run_command(&mut self, command: &str) -> bool {
        match command {
            "quick_open" => {
                self.palette = Some(PaletteState {
                    mode: PaletteMode::QuickOpen,
                    query: String::new(),
                    selected: 0,
                    focus_query: true,
                });
                self.focus_area = FocusArea::Palette;
                true
            }
            "command_palette" => {
                self.palette = Some(PaletteState {
                    mode: PaletteMode::Commands,
                    query: String::new(),
                    selected: 0,
                    focus_query: true,
                });
                self.focus_area = FocusArea::Palette;
                true
            }
            "toggle_file_tree" => {
                self.show_tree = !self.show_tree;
                self.status_message = if self.show_tree {
                    "file tree shown".to_string()
                } else {
                    "file tree hidden".to_string()
                };
                true
            }
            "tab_close" => {
                if let Some(active) = self.tabs.active_tab() {
                    let index = self
                        .tabs
                        .tabs()
                        .iter()
                        .position(|tab| tab.path == active.path)
                        .unwrap_or(0);
                    self.close_tab(index);
                }
                true
            }
            "tab_recent_next" => {
                self.tabs.focus_next();
                if let Some(active) = self.tabs.active_tab() {
                    let path = active.path.clone();
                    if let Some(document) = self.documents.get_mut(&path) {
                        document.needs_focus = true;
                    }
                }
                true
            }
            "save" => {
                if let Some(path) = self.tabs.active_tab().map(|tab| tab.path.clone()) {
                    if let Some(document) = self.documents.get_mut(&path) {
                        match document.save_now() {
                            Ok(()) => {
                                self.status_message = "saved".to_string();
                                self.tabs.set_dirty(&path, false);
                            }
                            Err(error) => {
                                self.status_message = format!("save failed: {error}");
                            }
                        }
                    }
                }
                true
            }
            "open_folder" => {
                self.status_message =
                    "open_folder is CLI-only in this PoC: start mica-egui with a folder argument"
                        .to_string();
                true
            }
            "search_in_folder" => {
                self.status_message = "search_in_folder is not implemented in this PoC".to_string();
                true
            }
            "cursor_down" | "cursor_up" | "expand_or_open" | "collapse" | "new_file" | "rename" => {
                // Tree commands are handled by the tree key handler,
                // not here.
                false
            }
            _ => {
                self.status_message = format!("unknown command: {command}");
                false
            }
        }
    }

    /// Handle raw key events through the global keymap. Returns the
    /// command that was run, if any.
    fn handle_keymap_keys(&mut self, ui: &Ui) -> Option<String> {
        if self.palette.is_some() {
            return None;
        }
        let events: Vec<egui::Event> = ui.input(|input| input.events.clone());
        for event in &events {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            {
                let Some(key_name) = describe_key(*key) else {
                    continue;
                };
                let mut description = String::new();
                if modifiers.ctrl || modifiers.command {
                    description.push_str("ctrl+");
                }
                if modifiers.shift {
                    description.push_str("shift+");
                }
                if modifiers.alt {
                    description.push_str("alt+");
                }
                description.push_str(&key_name);
                let area = match self.focus_area {
                    FocusArea::FileTree => "file_tree",
                    FocusArea::Editor => "editor",
                    FocusArea::Palette => "palette",
                };
                if let Some(command) = self.keymap.command_for_key(area, &description) {
                    let command = command.to_string();
                    // Tree-only commands are handled by the tree
                    // handler below; everything else runs here.
                    match command.as_str() {
                        "cursor_down" | "cursor_up" | "expand_or_open" | "collapse"
                        | "new_file" | "rename" => {
                            self.handle_tree_command(&command);
                        }
                        _ => {
                            self.run_command(&command);
                        }
                    }
                    return Some(command);
                }
            }
        }
        None
    }

    /// Tree-only commands from the keymap (`j`/`k`/`l`/`h` …).
    fn handle_tree_command(&mut self, command: &str) {
        let visible = self.visible_tree_entries();
        if visible.is_empty() {
            return;
        }
        self.tree_selected = self.tree_selected.min(visible.len() - 1);
        match command {
            "cursor_down" => {
                self.tree_selected = (self.tree_selected + 1).min(visible.len() - 1);
            }
            "cursor_up" => {
                self.tree_selected = self.tree_selected.saturating_sub(1);
            }
            "expand_or_open" => {
                let entry = visible[self.tree_selected].clone();
                if entry.is_folder {
                    self.toggle_folder(&entry.path);
                } else if entry.is_markdown {
                    self.open_file(&entry.path);
                }
            }
            "collapse" => {
                let entry = visible[self.tree_selected].clone();
                if entry.is_folder {
                    self.collapsed_folders.insert(entry.path.clone());
                }
            }
            "new_file" | "rename" => {
                self.status_message =
                    format!("{command} is not implemented in this PoC (tree is read-only)");
            }
            _ => {}
        }
    }

    /// Editor keys that `TextEdit` does not own: Enter splits the
    /// line, Backspace at column 0 merges upwards, Up/Down move the
    /// editing line. Runs after the editor was rendered this frame,
    /// so `editing_column` is current.
    fn handle_editor_keys(&mut self, ui: &Ui) {
        if self.palette.is_some() || self.focus_area != FocusArea::Editor {
            return;
        }
        let pressed = |key: egui::Key| ui.input(|input| input.key_pressed(key));
        if pressed(egui::Key::Enter) {
            if let Some(document) = self.active_document_mut() {
                document.split_editing_line_at_cursor();
            }
            return;
        }
        if pressed(egui::Key::Backspace) {
            // Merge only when the TextEdit did not already use this
            // Backspace to delete a character (see
            // `OpenDocument::backspace_should_merge`).
            let merge = self
                .active_document_mut()
                .map(|document| document.backspace_should_merge())
                .unwrap_or(false);
            if merge {
                if let Some(document) = self.active_document_mut() {
                    document.merge_editing_line_upwards();
                }
            }
            return;
        }
        if pressed(egui::Key::ArrowUp) {
            if let Some(document) = self.active_document_mut() {
                let line = document.editing_line.saturating_sub(1);
                document.move_editing_line(line);
            }
            return;
        }
        if pressed(egui::Key::ArrowDown) {
            if let Some(document) = self.active_document_mut() {
                let line = document.editing_line + 1;
                document.move_editing_line(line);
            }
        }
    }

    /// Auto-save tick: save every open document (active or in a
    /// background tab) whose debounce has elapsed, and keep repainting
    /// while anything is dirty.
    fn auto_save_tick(&mut self, ctx: &egui::Context) {
        if self.save_due_documents() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    /// The display-free part of [`Self::auto_save_tick`]. Returns
    /// `true` while any document still has unsaved edits.
    fn save_due_documents(&mut self) -> bool {
        let mut any_dirty = false;
        for (path, document) in &mut self.documents {
            if document.auto_saver.save_is_due() {
                match document.save_now() {
                    Ok(()) => {
                        self.saved_since = Some(Instant::now());
                        self.status_message = format!("saved {}", path.display());
                        self.tabs.set_dirty(path, false);
                    }
                    Err(error) => {
                        self.status_message =
                            format!("auto-save failed for {}: {error}", path.display());
                    }
                }
            }
            any_dirty |= matches!(document.save_state(), SaveState::Dirty | SaveState::Saving);
        }
        if let Some(saved_since) = self.saved_since {
            if saved_since.elapsed() > Duration::from_millis(1500) {
                for document in self.documents.values_mut() {
                    document.auto_saver.acknowledge_saved();
                }
                self.saved_since = None;
            }
        }
        any_dirty
    }

    /// Mark the active tab dirty when its document became dirty.
    fn sync_tab_dirty(&mut self) {
        if let Some(active) = self.tabs.active_tab() {
            let path = active.path.clone();
            let dirty = self
                .documents
                .get(&path)
                .map(|document| {
                    matches!(document.save_state(), SaveState::Dirty | SaveState::Saving)
                })
                .unwrap_or(false);
            self.tabs.set_dirty(&path, dirty);
        }
    }
}

impl eframe::App for MicaApp {
    /// Quitting (closing the window) flushes every document with
    /// unsaved edits, so the debounce window never loses work. eframe
    /// cannot cancel the exit from here, so a failed save can only be
    /// reported on stderr.
    fn on_exit(&mut self) {
        for error in self.flush_all() {
            eprintln!("mica-egui: save on quit failed: {error}");
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // Auto-quit for headless xvfb verification (`--quit-after`).
        // eframe only calls `ui()` when a repaint is requested, so
        // keep repainting while the quit timer is armed; otherwise
        // this check would never run on an idle window. When the
        // timer fires, exit the process directly: this is a test-only
        // hook, and `ViewportCommand::Close` alone proved unreliable
        // for quitting from inside `ui()` (see README).
        if let Some(quit_after) = self.quit_after {
            ui.ctx().request_repaint();
            if self.started.elapsed() > quit_after {
                // `process::exit` skips `on_exit`, so flush here too.
                for error in self.flush_all() {
                    eprintln!("mica-egui: save on quit failed: {error}");
                }
                std::process::exit(0);
            }
        }

        self.handle_keymap_keys(ui);

        // Escape closes the palette, otherwise moves focus to the tree.
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            if self.palette.is_some() {
                self.palette = None;
                self.focus_area = FocusArea::Editor;
            } else {
                self.focus_area = FocusArea::FileTree;
            }
        }

        // ---- File tree (left panel) ----
        if self.show_tree {
            let mut open_request: Option<PathBuf> = None;
            let mut toggle_request: Option<PathBuf> = None;
            let visible = self.visible_tree_entries();
            let selected = self.tree_selected;
            egui::Panel::left("file_tree_panel")
                .resizable(true)
                .default_size(240.0)
                .show(ui, |ui| {
                    ui.heading("Files");
                    ui.label(
                        RichText::new(self.folder.display().to_string())
                            .small()
                            .weak(),
                    );
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (index, entry) in visible.iter().enumerate() {
                            let indent = "    ".repeat(entry.depth);
                            let label = if entry.is_folder {
                                let marker = if self.collapsed_folders.contains(&entry.path) {
                                    "▸"
                                } else {
                                    "▾"
                                };
                                format!("{indent}{marker} {}", entry.name)
                            } else {
                                format!("{indent}  {}", entry.name)
                            };
                            let is_selected = index == selected;
                            let response = ui.selectable_label(is_selected, label);
                            if response.clicked() {
                                self.tree_selected = index;
                                self.focus_area = FocusArea::FileTree;
                                if entry.is_folder {
                                    toggle_request = Some(entry.path.clone());
                                } else if entry.is_markdown {
                                    open_request = Some(entry.path.clone());
                                }
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("↻ Rescan folder").clicked() {
                        self.tree_entries = scan_folder(&self.folder);
                        self.status_message = "folder rescanned".to_string();
                    }
                    ui.label(
                        RichText::new(format!("keymap: {}", self.keymap_source))
                            .small()
                            .weak(),
                    );
                });
            if let Some(path) = toggle_request {
                self.toggle_folder(&path);
            }
            if let Some(path) = open_request {
                self.open_file(&path);
            }
        }

        // ---- Tab bar (top panel) ----
        let mut tab_to_focus: Option<usize> = None;
        let mut tab_to_close: Option<usize> = None;
        egui::Panel::top("tab_bar_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                for (index, tab) in self.tabs.tabs().iter().enumerate() {
                    let is_active = self
                        .tabs
                        .active_tab()
                        .map(|active| active.path == tab.path)
                        .unwrap_or(false);
                    let title = if tab.dirty {
                        format!("● {}", tab.title)
                    } else {
                        tab.title.clone()
                    };
                    if ui.selectable_label(is_active, title).clicked() {
                        tab_to_focus = Some(index);
                    }
                    if ui.small_button("×").clicked() {
                        tab_to_close = Some(index);
                    }
                }
                if self.tabs.tabs().is_empty() {
                    ui.label(RichText::new("No file open — pick one from the tree.").weak());
                }
            });
        });
        if let Some(index) = tab_to_focus {
            self.tabs.focus(index);
            if let Some(active) = self.tabs.active_tab() {
                let path = active.path.clone();
                if let Some(document) = self.documents.get_mut(&path) {
                    document.needs_focus = true;
                }
                self.focus_area = FocusArea::Editor;
            }
        }
        if let Some(index) = tab_to_close {
            self.close_tab(index);
        }

        // ---- Status bar (bottom panel) ----
        egui::Panel::bottom("status_bar_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                let (save_text, save_color) = match self
                    .active_document_mut()
                    .map(|document| document.save_state())
                {
                    Some(SaveState::Clean) => ("Clean", Color32::GRAY),
                    Some(SaveState::Dirty) => ("● Unsaved", Color32::YELLOW),
                    Some(SaveState::Saving) => ("Saving…", Color32::YELLOW),
                    Some(SaveState::Saved) => ("✓ Saved", Color32::GREEN),
                    None => ("—", Color32::GRAY),
                };
                ui.label(RichText::new(save_text).color(save_color).small());
                ui.separator();
                if let Some(active) = self.tabs.active_tab() {
                    ui.label(RichText::new(active.path.display().to_string()).small());
                    if let Some(document) = self.documents.get(&active.path) {
                        ui.label(
                            RichText::new(format!(
                                "line {}/{} · col {}",
                                document.editing_line + 1,
                                document.buffer.line_count(),
                                document.editing_column + 1
                            ))
                            .small()
                            .weak(),
                        );
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(self.status_message.clone()).small().weak());
                });
            });
        });

        // ---- Editor (central panel) ----
        let mut clicked_line: Option<usize> = None;
        egui::CentralPanel::default().show(ui, |ui| {
            let active_path = self.tabs.active_tab().map(|tab| tab.path.clone());
            match active_path {
                Some(path) => {
                    if let Some(document) = self.documents.get_mut(&path) {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                // Centred body column, max 720 px wide,
                                // with left-aligned text inside it.
                                let body_width = ui.available_width().min(720.0);
                                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(body_width, ui.available_height()),
                                        Layout::top_down(Align::LEFT),
                                        |ui| {
                                            clicked_line = render_document(ui, document);
                                        },
                                    );
                                });
                            });
                    }
                }
                None => {
                    ui.centered_and_justified(|ui| {
                        ui.label("Open a Markdown file from the file tree (`l` or click).");
                    });
                }
            }
        });
        if let Some(line) = clicked_line {
            if let Some(document) = self.active_document_mut() {
                document.move_editing_line(line);
                self.focus_area = FocusArea::Editor;
            }
        }

        self.handle_editor_keys(ui);
        self.sync_tab_dirty();
        self.render_palette(ui);
        self.auto_save_tick(ui.ctx());
    }
}

impl MicaApp {
    /// Render the command palette / Quick Open overlay and handle its keys.
    fn render_palette(&mut self, ui: &mut Ui) {
        let Some(mut state) = self.palette.take() else {
            return;
        };

        // Build the entry list for this mode.
        let entries: Vec<PaletteEntry> = match state.mode {
            PaletteMode::QuickOpen => self
                .tree_entries
                .iter()
                .filter(|entry| entry.is_markdown)
                .map(|entry| PaletteEntry {
                    title: entry.name.clone(),
                    detail: entry.path.display().to_string(),
                    action: entry.path.display().to_string(),
                })
                .collect(),
            PaletteMode::Commands => KNOWN_COMMANDS
                .iter()
                .map(|command| PaletteEntry {
                    title: command.to_string(),
                    detail: "command".to_string(),
                    action: command.to_string(),
                })
                .collect(),
        };
        let filtered = filter_entries(&state.query, &entries);
        state.selected = state.selected.min(filtered.len().saturating_sub(1));

        let mut activate: Option<String> = None;
        let mut close = false;
        egui::Window::new("palette_window")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
            .show(ui.ctx(), |ui| {
                ui.set_min_width(480.0);
                let prompt = match state.mode {
                    PaletteMode::QuickOpen => "Quick Open — type a file name",
                    PaletteMode::Commands => "Command Palette — type a command",
                };
                ui.label(RichText::new(prompt).strong());
                let response = egui::TextEdit::singleline(&mut state.query)
                    .hint_text("type to filter…")
                    .desired_width(f32::INFINITY)
                    .show(ui);
                if state.focus_query {
                    response.response.request_focus();
                    state.focus_query = false;
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for (index, entry) in filtered.iter().enumerate() {
                            let is_selected = index == state.selected;
                            let label = format!("{}  —  {}", entry.title, entry.detail);
                            if ui.selectable_label(is_selected, label).clicked() {
                                activate = Some(entry.action.clone());
                            }
                        }
                        if filtered.is_empty() {
                            ui.label(RichText::new("No matches.").weak());
                        }
                    });
            });

        // Palette keys (arrows / Enter). Escape is handled globally.
        if ui.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
            state.selected = (state.selected + 1).min(filtered.len().saturating_sub(1));
        }
        if ui.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
            state.selected = state.selected.saturating_sub(1);
        }
        if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            if let Some(entry) = filtered.get(state.selected) {
                activate = Some(entry.action.clone());
            }
        }

        if let Some(action) = activate {
            close = true;
            match state.mode {
                PaletteMode::QuickOpen => {
                    let path = PathBuf::from(&action);
                    self.open_file(&path);
                }
                PaletteMode::Commands => {
                    self.run_command(&action);
                }
            }
        }
        if !close {
            self.palette = Some(state);
        } else {
            self.focus_area = FocusArea::Editor;
        }
    }
}

/// Load the global keymap (`~/.config/mica/keymap.toml`), falling back
/// to the built-in defaults. Returns the keymap and a description of
/// where it came from, for the status area.
fn load_keymap() -> (Keymap, String) {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = PathBuf::from(&home)
        .join(".config")
        .join("mica")
        .join("keymap.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => match Keymap::from_toml(&text) {
            Ok(keymap) => (keymap, format!("{} (custom)", path.display())),
            Err(error) => (
                Keymap::default_keymap(),
                format!("{} (invalid: {error}; using defaults)", path.display()),
            ),
        },
        Err(_) => (
            Keymap::default_keymap(),
            "defaults (no ~/.config/mica/keymap.toml found)".to_string(),
        ),
    }
}

/// Map an egui `Key` to the lower-case name used in keymap.toml
/// (`"p"`, `"enter"`, `"tab"`, `"up"`, …). Returns `None` for keys
/// the keymap cannot name (function keys, mouse buttons, …).
fn describe_key(key: egui::Key) -> Option<String> {
    let debug = format!("{key:?}");
    let name = match debug.as_str() {
        "ArrowUp" => "up",
        "ArrowDown" => "down",
        "ArrowLeft" => "left",
        "ArrowRight" => "right",
        "Enter" => "enter",
        "Tab" => "tab",
        "Escape" => "escape",
        "Backspace" => "backspace",
        "Delete" => "delete",
        "Space" => "space",
        "Home" => "home",
        "End" => "end",
        "PageUp" => "pageup",
        "PageDown" => "pagedown",
        other => other,
    };
    // Single letters/digits come through as "A", "Num0", … — only
    // single characters are usable keymap names.
    let lower = name.to_lowercase();
    if lower.chars().count() == 1 {
        Some(lower)
    } else if matches!(
        lower.as_str(),
        "up" | "down"
            | "left"
            | "right"
            | "enter"
            | "tab"
            | "escape"
            | "backspace"
            | "delete"
            | "space"
            | "home"
            | "end"
            | "pageup"
            | "pagedown"
    ) {
        Some(lower)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mica_shared_core::auto_save::AutoSaver;

    /// A fresh folder with `a.md` and `b.md`; the app opens `a.md`
    /// first (as `new` does), then `b.md`, so `a.md` is a background
    /// tab and `b.md` is active.
    fn app_with_two_tabs(name: &str) -> (MicaApp, PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("mica-egui-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.md");
        let b = dir.join("b.md");
        std::fs::write(&a, "alpha\n").unwrap();
        std::fs::write(&b, "beta\n").unwrap();
        let mut app = MicaApp::new(dir.clone(), None);
        app.open_file(&b);
        assert_eq!(app.tabs.active_tab().unwrap().path, b);
        (app, dir, a, b)
    }

    /// Edit the first line of a (background) document, the way the
    /// cursor-line `TextEdit` does, inside the debounce window.
    fn edit_first_line(app: &mut MicaApp, path: &Path, text: &str) {
        let document = app.documents.get_mut(path).unwrap();
        document.line_edit_text = text.to_string();
        document.apply_line_input(Some(text.chars().count()));
        assert_eq!(document.save_state(), SaveState::Dirty);
        assert!(!document.auto_saver.save_is_due(), "still inside debounce");
    }

    fn tab_index(app: &MicaApp, path: &Path) -> Option<usize> {
        app.tabs.tabs().iter().position(|tab| tab.path == path)
    }

    #[test]
    fn closing_a_background_tab_saves_its_unsaved_edit() {
        let (mut app, dir, a, _) = app_with_two_tabs("close-saves");
        edit_first_line(&mut app, &a, "alpha edited");
        let index = tab_index(&app, &a).unwrap();
        assert!(app.close_tab(index));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha edited\n");
        assert!(tab_index(&app, &a).is_none());
        assert!(!app.documents.contains_key(&a));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn closing_a_tab_whose_save_fails_keeps_it_open() {
        let (mut app, dir, a, _) = app_with_two_tabs("close-fails");
        edit_first_line(&mut app, &a, "alpha edited");
        // Make the save fail: the folder is gone, so the temp file
        // cannot be created.
        std::fs::remove_dir_all(&dir).unwrap();
        let index = tab_index(&app, &a).unwrap();
        assert!(!app.close_tab(index));
        assert!(tab_index(&app, &a).is_some(), "tab must stay open");
        assert_eq!(
            app.documents[&a].buffer.line(0),
            Some("alpha edited"),
            "edits stay in memory"
        );
        assert!(app.documents[&a].has_unsaved_changes());
        assert!(
            app.status_message.contains("save failed"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn auto_save_tick_saves_background_documents() {
        let (mut app, dir, a, b) = app_with_two_tabs("tick-background");
        app.documents.get_mut(&a).unwrap().auto_saver =
            AutoSaver::with_delay(Duration::from_millis(1));
        edit_first_line(&mut app, &a, "alpha edited");
        app.tabs.set_dirty(&a, true);
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(app.tabs.active_tab().unwrap().path, b, "a.md is background");
        assert!(!app.save_due_documents(), "nothing left dirty");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha edited\n");
        assert!(!app.tabs.tabs()[tab_index(&app, &a).unwrap()].dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flush_all_saves_every_dirty_document() {
        let (mut app, dir, a, b) = app_with_two_tabs("flush-all");
        edit_first_line(&mut app, &a, "alpha edited");
        edit_first_line(&mut app, &b, "beta edited");
        assert!(app.flush_all().is_empty());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha edited\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "beta edited\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keymap_names_cover_default_bindings() {
        assert_eq!(describe_key(egui::Key::P), Some("p".to_string()));
        assert_eq!(describe_key(egui::Key::Tab), Some("tab".to_string()));
        assert_eq!(describe_key(egui::Key::Enter), Some("enter".to_string()));
        assert_eq!(describe_key(egui::Key::ArrowUp), Some("up".to_string()));
        let keymap = Keymap::default_keymap();
        assert_eq!(
            keymap.command_for_key("global", "ctrl+p"),
            Some("quick_open")
        );
        assert_eq!(
            keymap.command_for_key("global", "ctrl+shift+p"),
            Some("command_palette")
        );
        assert_eq!(
            keymap.command_for_key("file_tree", "j"),
            Some("cursor_down")
        );
        // Area bindings do not leak into the editor: typing "j" while
        // editing must insert a "j", not move the tree cursor.
        assert_eq!(keymap.command_for_key("editor", "j"), None);
    }

    #[test]
    fn palette_filters_files_and_commands() {
        let entries = vec![
            PaletteEntry {
                title: "welcome.md".to_string(),
                detail: "/notes/welcome.md".to_string(),
                action: "/notes/welcome.md".to_string(),
            },
            PaletteEntry {
                title: "second-note.md".to_string(),
                detail: "/notes/second-note.md".to_string(),
                action: "/notes/second-note.md".to_string(),
            },
        ];
        let filtered = filter_entries("wel", &entries);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "welcome.md");
    }
}
