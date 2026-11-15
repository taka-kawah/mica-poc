//! End-to-end smoke test of the shared core, headless:
//! open a folder, open a file in a tab, edit it (including Japanese
//! IME-style text), check live preview, auto-save, re-read from disk.

use mica_shared_core::auto_save::{save_atomically, AutoSaver, SaveState};
use mica_shared_core::command_palette::{filter_entries, PaletteEntry};
use mica_shared_core::file_tree::scan_folder;
use mica_shared_core::live_preview::decorate_line;
use mica_shared_core::tabs::TabManager;
use mica_shared_core::text_buffer::{CursorPosition, TextBuffer};
use mica_shared_core::wsl_bridge::{
    BridgeRequest, LocalBridgeBackend, METHOD_READ_FILE, METHOD_WRITE_FILE,
};
use std::fs;
use std::path::Path;
use std::time::Duration;

#[test]
fn open_folder_edit_preview_save_and_reload() {
    // 1. Open the shared sample folder.
    let sample_folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-notes");
    let entries = scan_folder(&sample_folder);
    assert!(
        entries.iter().any(|entry| entry.name == "welcome.md"),
        "sample folder should contain welcome.md, got {entries:?}"
    );

    // 2. Open welcome.md in a tab; reopening focuses the same tab.
    let welcome_path = sample_folder.join("welcome.md");
    let mut tabs = TabManager::new();
    let first = tabs.open_file(&welcome_path);
    tabs.open_file(&sample_folder.join("second-note.md"));
    assert_eq!(tabs.open_file(&welcome_path), first);
    assert_eq!(tabs.tabs().len(), 2);
    assert_eq!(tabs.active_tab().unwrap().title, "welcome.md");

    // 3. Edit the buffer, including Japanese text.
    let original = fs::read_to_string(&welcome_path).unwrap();
    let mut buffer = TextBuffer::from_text(&original);
    let end_of_first_line = CursorPosition {
        line: 0,
        column: buffer.line(0).unwrap().chars().count(),
    };
    buffer.insert(end_of_first_line, " — こんにちは");
    assert!(buffer.to_text().contains("こんにちは"));

    // 4. Live preview: heading markers hidden off the cursor line,
    // shown on it.
    let (hidden, _) = decorate_line("# Welcome to Mica", 0, Some(3), false);
    assert_eq!(hidden.display_text, "Welcome to Mica");
    let (shown, _) = decorate_line("# Welcome to Mica", 0, Some(0), false);
    assert_eq!(shown.display_text, "# Welcome to Mica");

    // 5. Auto-save the edited buffer to a temp copy and re-read it.
    let dir = tempfile::tempdir().unwrap();
    let copy_path = dir.path().join("welcome-copy.md");
    let mut saver = AutoSaver::with_delay(Duration::from_millis(1));
    saver.note_edit();
    assert_eq!(saver.state(), SaveState::Dirty);
    std::thread::sleep(Duration::from_millis(5));
    assert!(saver.save_is_due());
    saver.begin_save();
    save_atomically(&copy_path, &buffer.to_text()).unwrap();
    saver.finish_save();
    assert_eq!(saver.state(), SaveState::Saved);
    let reloaded = fs::read_to_string(&copy_path).unwrap();
    assert!(reloaded.contains("こんにちは"));
    assert!(reloaded.contains("**sample note**"));

    // 6. The same file round-trips through the WSL bridge.
    let backend = LocalBridgeBackend::new(dir.path());
    let read = backend.handle(&BridgeRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: METHOD_READ_FILE.to_string(),
        params: serde_json::json!({"path": "welcome-copy.md"}),
    });
    assert!(read.error.is_none());
    assert!(read.result.unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("こんにちは"));
    let write = backend.handle(&BridgeRequest {
        jsonrpc: "2.0".to_string(),
        id: 2,
        method: METHOD_WRITE_FILE.to_string(),
        params: serde_json::json!({"path": "bridge-note.md", "content": "# via bridge\n"}),
    });
    assert!(write.error.is_none());
    assert!(fs::read_to_string(dir.path().join("bridge-note.md"))
        .unwrap()
        .contains("via bridge"));

    // 7. Quick Open finds the file by a fuzzy query.
    let entries = vec![
        PaletteEntry {
            title: "welcome.md".to_string(),
            detail: sample_folder.to_string_lossy().to_string(),
            action: "open:welcome.md".to_string(),
        },
        PaletteEntry {
            title: "second-note.md".to_string(),
            detail: sample_folder.to_string_lossy().to_string(),
            action: "open:second-note.md".to_string(),
        },
    ];
    // "welc" matches only "welcome.md": "second-note.md" has no
    // such subsequence in its title, and both entries share the same
    // folder path as their detail.
    let results = filter_entries("welc", &entries);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "welcome.md");
}
