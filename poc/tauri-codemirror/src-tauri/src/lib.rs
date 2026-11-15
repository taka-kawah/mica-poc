//! Mica Tauri PoC — Rust backend.
//!
//! The backend owns everything about files. The TypeScript frontend
//! never touches the filesystem directly; it only calls the Tauri
//! commands in this module (`open_folder`, `list_files`, `read_file`,
//! `save_file`, `search_files`, `search_palette`, `list_commands`,
//! `load_keymap`, `open_tab`, `close_tab`, `list_tabs`,
//! `default_folder`).
//!
//! Every command delegates to `mica-shared-core` instead of
//! reimplementing the logic:
//!
//! * `file_tree::scan_folder` — folder scan for the tree.
//! * `tabs::TabManager` — one file = one tab; reopening focuses.
//! * `auto_save::save_atomically` — temp file + rename for saves.
//! * `keymap` — the global `~/.config/mica/keymap.toml`, with the
//!   built-in defaults as fallback.
//! * `command_palette::{filter_entries, PaletteEntry}` — fuzzy match
//!   for Ctrl+P / Ctrl+Shift+P.
//! * `wsl_bridge::LocalBridgeBackend` — the stdio JSON-RPC protocol
//!   (Zed-style) with the in-process local-filesystem backend used on
//!   Linux. The Windows transport (`wsl.exe … --exec …`) is
//!   documented in shared-core and is NOT faked as working here.

use mica_shared_core::{
    auto_save::save_atomically,
    command_palette::{filter_entries, PaletteEntry},
    file_tree::{scan_folder, FileTreeEntry},
    keymap::{Keymap, DEFAULT_KEYMAP_TOML, KNOWN_COMMANDS},
    tabs::{Tab, TabManager},
    wsl_bridge::{BridgeRequest, LocalBridgeBackend, METHOD_SEARCH},
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::State;

// ---------------------------------------------------------------------
// Serialisable shapes returned to the frontend.
// ---------------------------------------------------------------------

/// One file-tree row, as the frontend renders it.
#[derive(Debug, Clone, Serialize)]
struct FileTreeEntryDto {
    path: String,
    name: String,
    is_folder: bool,
    depth: usize,
    is_markdown: bool,
}

impl From<&FileTreeEntry> for FileTreeEntryDto {
    fn from(entry: &FileTreeEntry) -> Self {
        Self {
            path: entry.path.to_string_lossy().to_string(),
            name: entry.name.clone(),
            is_folder: entry.is_folder,
            depth: entry.depth,
            is_markdown: entry.is_markdown,
        }
    }
}

/// One command-palette / Quick Open row.
#[derive(Debug, Clone, Serialize)]
struct PaletteEntryDto {
    title: String,
    detail: String,
    action: String,
}

impl From<&PaletteEntry> for PaletteEntryDto {
    fn from(entry: &PaletteEntry) -> Self {
        Self {
            title: entry.title.clone(),
            detail: entry.detail.clone(),
            action: entry.action.clone(),
        }
    }
}

/// One key binding from the keymap file (or the defaults).
#[derive(Debug, Clone, Serialize)]
struct KeyBindingDto {
    key: String,
    command: String,
    area: String,
}

/// What `load_keymap` returns: where the keymap came from, and all
/// of its bindings, so the frontend can apply them.
#[derive(Debug, Clone, Serialize)]
struct KeymapInfoDto {
    /// `"file"` when `~/.config/mica/keymap.toml` was read, else
    /// `"defaults"`.
    source: String,
    /// Human-readable location shown in the status bar.
    location: String,
    bindings: Vec<KeyBindingDto>,
}

/// Result of opening a folder: its canonical path and its tree.
#[derive(Debug, Clone, Serialize)]
struct OpenFolderResult {
    path: String,
    entries: Vec<FileTreeEntryDto>,
}

/// One open tab, as the frontend's tab bar shows it.
#[derive(Debug, Clone, Serialize)]
struct TabDto {
    path: String,
    title: String,
    dirty: bool,
}

impl From<&Tab> for TabDto {
    fn from(tab: &Tab) -> Self {
        Self {
            path: tab.path.to_string_lossy().to_string(),
            title: tab.title.clone(),
            dirty: tab.dirty,
        }
    }
}

// ---------------------------------------------------------------------
// Application state.
// ---------------------------------------------------------------------

struct AppState {
    /// Folder currently open in the file tree, if any.
    root_folder: Mutex<Option<PathBuf>>,
    /// Tab list, kept in shared-core so reopening a file focuses it.
    tabs: Mutex<TabManager>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            root_folder: Mutex::new(None),
            tabs: Mutex::new(TabManager::new()),
        }
    }
}

// ---------------------------------------------------------------------
// Plain helpers (unit-testable without Tauri state).
// ---------------------------------------------------------------------

/// Parse a keymap TOML document into a flat binding list.
/// `Keymap` itself keeps its tables private, so the binding list the
/// frontend needs is built by parsing the same TOML text here; the
/// text is always validated first with `Keymap::from_toml`.
fn keymap_bindings_from_toml(text: &str) -> Result<Vec<KeyBindingDto>, String> {
    Keymap::from_toml(text)?;
    let value: toml::Value =
        toml::from_str(text).map_err(|error: toml::de::Error| error.to_string())?;
    let table = value
        .as_table()
        .ok_or_else(|| "keymap must be a TOML table".to_string())?;
    let mut bindings = Vec::new();
    for (section, section_value) in table {
        let section_table = section_value
            .as_table()
            .ok_or_else(|| format!("keymap section [{section}] must be a table"))?;
        for (key, command) in section_table {
            let command = command
                .as_str()
                .ok_or_else(|| format!("keymap binding {key} must be a string"))?;
            bindings.push(KeyBindingDto {
                key: key.clone(),
                command: command.to_string(),
                area: section.clone(),
            });
        }
    }
    Ok(bindings)
}

/// Folder the app opens on startup: `MICA_FOLDER` wins, then the
/// shared `sample-notes` folder next to this PoC (when the app is
/// run from the repo checkout), then the user's home directory.
fn resolve_default_folder() -> PathBuf {
    if let Ok(folder) = std::env::var("MICA_FOLDER") {
        let path = PathBuf::from(&folder);
        if path.is_dir() {
            return path;
        }
    }
    if let Ok(current_dir) = std::env::current_dir() {
        for candidate in [
            current_dir.join("sample-notes"),
            current_dir.join("../../sample-notes"),
            current_dir.join("../../../sample-notes"),
        ] {
            if candidate.is_dir() {
                // Canonicalise so the UI shows a clean absolute path
                // (no `../..` segments from the dev-run cwd).
                if let Ok(canonical) = candidate.canonicalize() {
                    return canonical;
                }
                return candidate;
            }
        }
    }
    home_dir()
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

/// Short descriptions shown under command names in the palette.
fn command_detail(command: &str) -> &'static str {
    match command {
        "quick_open" => "Open a file by name (Ctrl+P)",
        "command_palette" => "Run a command by name (Ctrl+Shift+P)",
        "toggle_file_tree" => "Show or hide the file tree (Ctrl+B)",
        "tab_recent_next" => "Go to the next tab (Ctrl+Tab)",
        "tab_close" => "Close the active tab (Ctrl+W)",
        "search_in_folder" => "Search text in the open folder (Ctrl+Shift+F)",
        "cursor_down" => "File tree: move down (j)",
        "cursor_up" => "File tree: move up (k)",
        "expand_or_open" => "File tree: expand or open (l)",
        "collapse" => "File tree: collapse (h)",
        "new_file" => "File tree: create a new file (a) — not in this PoC",
        "rename" => "File tree: rename (r) — not in this PoC",
        "save" => "Save the active file (Ctrl+S)",
        "open_folder" => "Open a folder",
        _ => "",
    }
}

fn known_command_entries() -> Vec<PaletteEntry> {
    KNOWN_COMMANDS
        .iter()
        .map(|command| PaletteEntry {
            title: (*command).to_string(),
            detail: command_detail(command).to_string(),
            action: (*command).to_string(),
        })
        .collect()
}

fn file_entries(entries: &[FileTreeEntry]) -> Vec<PaletteEntry> {
    entries
        .iter()
        .filter(|entry| !entry.is_folder && entry.is_markdown)
        .map(|entry| PaletteEntry {
            title: entry.name.clone(),
            detail: entry.path.to_string_lossy().to_string(),
            action: entry.path.to_string_lossy().to_string(),
        })
        .collect()
}

// ---------------------------------------------------------------------
// Tauri commands.
// ---------------------------------------------------------------------

/// Folder to open on startup (see `resolve_default_folder`).
#[tauri::command]
fn default_folder() -> Result<String, String> {
    Ok(resolve_default_folder().to_string_lossy().to_string())
}

/// Open a folder as the tree root and return its scanned entries.
#[tauri::command]
fn open_folder(path: String, state: State<'_, AppState>) -> Result<OpenFolderResult, String> {
    let folder = Path::new(&path);
    if !folder.is_dir() {
        return Err(format!("not a folder: {path}"));
    }
    let folder = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
    let entries = scan_folder(&folder);
    *state
        .root_folder
        .lock()
        .map_err(|error| error.to_string())? = Some(folder.clone());
    Ok(OpenFolderResult {
        path: folder.to_string_lossy().to_string(),
        entries: entries.iter().map(FileTreeEntryDto::from).collect(),
    })
}

/// Re-scan a folder without changing the open root.
#[tauri::command]
fn list_files(path: String) -> Result<Vec<FileTreeEntryDto>, String> {
    let folder = Path::new(&path);
    if !folder.is_dir() {
        return Err(format!("not a folder: {path}"));
    }
    Ok(scan_folder(folder)
        .iter()
        .map(FileTreeEntryDto::from)
        .collect())
}

/// Read one file as text.
#[tauri::command]
fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|error| error.to_string())
}

/// Save one file atomically (temp file + rename, via shared-core).
/// Marks the tab clean when the file is open in a tab.
#[tauri::command]
fn save_file(path: String, content: String, state: State<'_, AppState>) -> Result<(), String> {
    let file = Path::new(&path);
    save_atomically(file, &content).map_err(|error| error.to_string())?;
    if let Ok(mut tabs) = state.tabs.lock() {
        tabs.set_dirty(file, false);
    }
    Ok(())
}

/// Full-text search of `.md` files under the open folder, answered
/// through the shared-core WSL bridge stub (`LocalBridgeBackend`),
/// i.e. the same request shape the Windows GUI would send over
/// stdio to the WSL server.
#[tauri::command]
fn search_files(query: String, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let root = state
        .root_folder
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "no folder is open".to_string())?;
    let backend = LocalBridgeBackend::new(&root);
    let request = BridgeRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: METHOD_SEARCH.to_string(),
        params: serde_json::json!({ "query": query }),
    };
    let response = backend.handle(&request);
    if let Some(error) = response.error {
        return Err(error["message"]
            .as_str()
            .unwrap_or("search failed")
            .to_string());
    }
    let matches = response
        .result
        .and_then(|result| result["matches"].as_array().cloned())
        .unwrap_or_default();
    Ok(matches
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect())
}

/// All commands the palette can run.
#[tauri::command]
fn list_commands() -> Vec<PaletteEntryDto> {
    known_command_entries()
        .iter()
        .map(PaletteEntryDto::from)
        .collect()
}

/// Fuzzy-filter palette entries for Quick Open (`kind = "file"`) or
/// the command palette (`kind = "command"`), using shared-core's
/// matcher — the same matcher every PoC uses.
#[tauri::command]
fn search_palette(
    query: String,
    kind: String,
    state: State<'_, AppState>,
) -> Result<Vec<PaletteEntryDto>, String> {
    let entries = match kind.as_str() {
        "command" => known_command_entries(),
        "file" => {
            let root = state
                .root_folder
                .lock()
                .map_err(|error| error.to_string())?
                .clone()
                .ok_or_else(|| "no folder is open".to_string())?;
            file_entries(&scan_folder(&root))
        }
        other => return Err(format!("unknown palette kind: {other}")),
    };
    Ok(filter_entries(&query, &entries)
        .iter()
        .map(|entry| PaletteEntryDto::from(*entry))
        .collect())
}

/// Load the global keymap: `~/.config/mica/keymap.toml` when it
/// exists, otherwise the shared-core built-in defaults (Notion §7).
#[tauri::command]
fn load_keymap() -> Result<KeymapInfoDto, String> {
    let keymap_path = home_dir().join(".config").join("mica").join("keymap.toml");
    let (source, location, text) = match std::fs::read_to_string(&keymap_path) {
        Ok(text) => (
            "file".to_string(),
            keymap_path.to_string_lossy().to_string(),
            text,
        ),
        Err(_) => (
            "defaults".to_string(),
            "built-in defaults (Notion §7)".to_string(),
            DEFAULT_KEYMAP_TOML.to_string(),
        ),
    };
    let bindings = keymap_bindings_from_toml(&text)?;
    Ok(KeymapInfoDto {
        source,
        location,
        bindings,
    })
}

/// Open a file in a tab (reopening focuses the existing tab) and
/// return its tab index.
#[tauri::command]
fn open_tab(path: String, state: State<'_, AppState>) -> Result<usize, String> {
    let mut tabs = state.tabs.lock().map_err(|error| error.to_string())?;
    Ok(tabs.open_file(Path::new(&path)))
}

/// Close the tab at `index`; returns the closed tab, if any.
#[tauri::command]
fn close_tab(index: usize, state: State<'_, AppState>) -> Result<Option<TabDto>, String> {
    let mut tabs = state.tabs.lock().map_err(|error| error.to_string())?;
    Ok(tabs.close(index).as_ref().map(TabDto::from))
}

/// All open tabs.
#[tauri::command]
fn list_tabs(state: State<'_, AppState>) -> Result<Vec<TabDto>, String> {
    let tabs = state.tabs.lock().map_err(|error| error.to_string())?;
    Ok(tabs.tabs().iter().map(TabDto::from).collect())
}

/// Start the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            default_folder,
            open_folder,
            list_files,
            read_file,
            save_file,
            search_files,
            list_commands,
            search_palette,
            load_keymap,
            open_tab,
            close_tab,
            list_tabs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Mica Tauri application");
}

// ---------------------------------------------------------------------
// Tests — headless smoke test of the backend commands against the
// shared sample notes (no display needed).
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// The repo's shared sample notes, resolved from this crate's
    /// manifest directory. Present in every repo checkout.
    fn sample_notes_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../sample-notes")
    }

    #[test]
    fn file_tree_lists_sample_notes() {
        let samples = sample_notes_dir();
        assert!(
            samples.is_dir(),
            "sample notes missing: {}",
            samples.display()
        );
        let entries = scan_folder(&samples);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"welcome.md"));
        assert!(names.contains(&"second-note.md"));
    }

    #[test]
    fn read_welcome_and_save_atomically_round_trip() {
        let samples = sample_notes_dir();
        let content = read_file(samples.join("welcome.md").to_string_lossy().to_string())
            .expect("welcome.md must be readable");
        assert!(content.contains("# Welcome to Mica"));
        assert!(content.contains("**sample note**"));

        // Save round-trip in a scratch dir (never the real samples).
        let scratch = tempfile::tempdir().unwrap();
        let target = scratch.path().join("edited.md");
        save_atomically(&target, "# Edited\n\n日本語のテキスト\n").unwrap();
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "# Edited\n\n日本語のテキスト\n"
        );
        // No temp file left behind.
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1);
    }

    #[test]
    fn bridge_search_finds_sample_text() {
        let samples = sample_notes_dir();
        let backend = LocalBridgeBackend::new(&samples);
        let request = BridgeRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: METHOD_SEARCH.to_string(),
            params: serde_json::json!({ "query": "Live preview" }),
        };
        let response = backend.handle(&request);
        assert!(response.error.is_none());
        let matches = response.result.unwrap()["matches"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        assert!(
            matches.iter().any(|path| path.ends_with("welcome.md")),
            "expected welcome.md in search results, got {matches:?}"
        );
    }

    #[test]
    fn tabs_reopen_focuses_and_palette_filters() {
        let mut tabs = TabManager::new();
        let first = tabs.open_file(Path::new("/notes/a.md"));
        tabs.open_file(Path::new("/notes/b.md"));
        assert_eq!(tabs.open_file(Path::new("/notes/a.md")), first);
        assert_eq!(tabs.tabs().len(), 2);

        let entries = known_command_entries();
        // "tab" matches both tab_recent_next and tab_close (equal
        // scores, original order kept); "tab_c" isolates tab_close.
        let filtered = filter_entries("tab", &entries);
        assert!(filtered.len() >= 2);
        assert!(filtered.iter().any(|entry| entry.title == "tab_close"));
        let narrow = filter_entries("tab_c", &entries);
        assert_eq!(narrow[0].title, "tab_close");
    }

    #[test]
    fn keymap_defaults_parse_to_bindings() {
        let bindings = keymap_bindings_from_toml(DEFAULT_KEYMAP_TOML).unwrap();
        let quick_open = bindings
            .iter()
            .find(|binding| binding.command == "quick_open")
            .expect("defaults must bind quick_open");
        assert_eq!(quick_open.key, "ctrl+p");
        assert_eq!(quick_open.area, "global");
        let tree_down = bindings
            .iter()
            .find(|binding| binding.command == "cursor_down")
            .expect("defaults must bind cursor_down");
        assert_eq!(tree_down.area, "file_tree");
    }

    #[test]
    fn keymap_file_is_used_when_present() {
        // Build a keymap the way load_keymap reads it, from a temp
        // home directory, and check the binding list reflects it.
        let scratch = tempfile::tempdir().unwrap();
        let config_dir = scratch.path().join(".config").join("mica");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("keymap.toml"),
            "[global]\n\"ctrl+o\" = \"open_folder\"\n",
        )
        .unwrap();
        let text = fs::read_to_string(config_dir.join("keymap.toml")).unwrap();
        let bindings = keymap_bindings_from_toml(&text).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].command, "open_folder");
    }
}
