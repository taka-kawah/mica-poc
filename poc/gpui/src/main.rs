//! `mica-gpui` — the GPUI PoC for the Mica Markdown editor.
//!
//! One GPUI window: file tree, tab bar, live-preview editor,
//! status bar, command palette / Quick Open. All editor logic lives
//! in `model` (headless, shared-core based); `view` is the GPUI
//! rendering and input handling.
//!
//! Usage:
//!
//! ```sh
//! mica-gpui [folder]              # open a folder in the GUI
//! mica-gpui --smoke-test [folder] # headless check, no display needed
//! ```
//!
//! License: MIT OR Apache-2.0. GPUI itself is Apache-2.0; no Zed
//! editor crate (GPL-3.0) is used, copied, or depended on — see
//! `README.md` and `docs/research-2026-11.md` §8.

mod model;
mod view;

use std::path::{Path, PathBuf};

use gpui::{
    px, size, App, AppContext, Bounds, Focusable, TitlebarOptions, WindowBounds, WindowOptions,
};
use gpui_platform::application;

use model::{document_text, EditorModel, PaletteKind};
use view::{key_bindings_from_keymap_file, MicaView};

/// Default folder when none is given on the command line: the shared
/// sample notes next to this PoC.
fn default_folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("sample-notes")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if let Some(position) = args.iter().position(|arg| arg == "--smoke-test") {
        let folder = args
            .get(position + 1)
            .map(PathBuf::from)
            .unwrap_or_else(default_folder);
        std::process::exit(smoke_test(&folder));
    }

    let folder = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_folder);
    run_gui(&folder);
}

fn run_gui(folder: &Path) {
    let folder = folder.to_path_buf();
    application().run(move |cx: &mut App| {
        cx.bind_keys(key_bindings_from_keymap_file(cx));

        let mut model = EditorModel::open_folder(&folder);
        // Open the first Markdown file, if the folder has one.
        if let Some(first) = model
            .tree
            .iter()
            .find(|entry| entry.is_markdown)
            .map(|entry| entry.path.clone())
        {
            let _ = model.open_file(&first);
        }

        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Mica — GPUI PoC".into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| MicaView::new(model, cx));
                    // Closing the window saves every dirty document
                    // first (Ctrl+Q and app quit flush in the view).
                    let weak_view = view.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        let _ = weak_view.update(cx, |view, _| {
                            for error in view.model.flush_all() {
                                eprintln!("mica-gpui: save on close failed: {error}");
                            }
                        });
                        true
                    });
                    view
                },
            )
            .expect("failed to open the Mica window");
        window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx), cx);
            })
            .expect("failed to focus the editor");
        cx.activate(true);
    });
}

/// Headless smoke test: open a folder, open a file, edit it (including
/// Japanese text), auto-save, re-read from disk and verify.
///
/// The edited file is restored to its original content afterwards, so
/// running the smoke test against the shared `sample-notes` folder
/// does not mutate the fixture other PoCs use.
///
/// Returns a process exit code: 0 on PASS, 1 on FAIL. Every check
/// prints its own PASS/FAIL line.
fn smoke_test(folder: &Path) -> i32 {
    let mut failures = 0;
    let mut check = |name: &str, passed: bool, detail: &str| {
        println!(
            "[{}] {} — {}",
            if passed { "PASS" } else { "FAIL" },
            name,
            detail
        );
        if !passed {
            failures += 1;
        }
    };

    println!("mica-gpui --smoke-test {}", folder.display());

    let mut model = EditorModel::open_folder(folder);
    check(
        "open folder and scan tree",
        !model.tree.is_empty(),
        &format!("{} entries", model.tree.len()),
    );

    let Some(note) = model
        .tree
        .iter()
        .find(|entry| entry.name == "welcome.md")
        .or_else(|| model.tree.iter().find(|entry| entry.is_markdown))
        .map(|entry| entry.path.clone())
    else {
        println!("[FAIL] open a Markdown file — no .md file in the folder");
        return 1;
    };
    let original = std::fs::read_to_string(&note).unwrap_or_default();
    let opened = model.open_file(&note).is_ok();
    check("open file in a tab", opened, &note.to_string_lossy());
    check(
        "one file = one tab",
        model.tabs.tabs().len() == 1,
        &format!("{} tab(s)", model.tabs.tabs().len()),
    );

    // Edit: append Japanese text to the first line, as an IME commit
    // would arrive through the input handler.
    let first_line_length = model
        .active_buffer()
        .and_then(|buffer| buffer.line(0))
        .map(|line| line.chars().count())
        .unwrap_or(0);
    model.set_cursor(mica_shared_core::text_buffer::CursorPosition {
        line: 0,
        column: first_line_length,
    });
    model.insert_at_cursor(" — こんにちは");
    let text = model.active_buffer().map(document_text).unwrap_or_default();
    check(
        "Japanese insert does not corrupt the buffer",
        text.contains("こんにちは") && text.matches("こんにちは").count() == 1,
        "buffer contains exactly one こんにちは",
    );

    // Live-preview decorations: move the cursor off the heading line
    // and check its marker is hidden while the text survives.
    model.set_cursor(mica_shared_core::text_buffer::CursorPosition { line: 2, column: 0 });
    let lines = model.display_lines();
    let heading_hidden = lines
        .first()
        .map(|line| line.display_text == "Welcome to Mica — こんにちは")
        .unwrap_or(false);
    check(
        "heading marker hidden off the cursor line",
        heading_hidden,
        lines
            .first()
            .map(|line| line.display_text.as_str())
            .unwrap_or("<no lines>"),
    );

    // Auto-save: wait past the 1000 ms debounce, save, re-read.
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let saved = model.maybe_auto_save();
    check(
        "auto-save fires after the debounce",
        saved,
        "save_is_due was true",
    );
    let on_disk = std::fs::read_to_string(&note).unwrap_or_default();
    check(
        "re-read from disk matches the edited buffer",
        on_disk.contains("こんにちは"),
        "disk content contains こんにちは",
    );

    // Palette: Quick Open finds the note, the command palette finds
    // the tab command.
    let files = model.filtered_palette_entries(PaletteKind::QuickOpen, "welc");
    check(
        "Quick Open finds welcome.md",
        files.iter().any(|entry| entry.title == "welcome.md"),
        "query 'welc'",
    );
    let commands = model.filtered_palette_entries(PaletteKind::Commands, "tab");
    check(
        "command palette finds tab_close",
        commands.iter().any(|entry| entry.action == "tab_close"),
        "query 'tab'",
    );

    // Restore the fixture.
    if std::fs::write(&note, &original).is_ok() {
        println!("(restored {} to its original content)", note.display());
    } else {
        println!("[FAIL] restore original file content");
        failures += 1;
    }

    if failures == 0 {
        println!("SMOKE TEST: PASS");
        0
    } else {
        println!("SMOKE TEST: FAIL ({failures} check(s) failed)");
        1
    }
}
