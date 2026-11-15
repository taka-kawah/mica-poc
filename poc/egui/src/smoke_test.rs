//! Headless smoke test + Japanese IME simulation for the egui PoC.
//!
//! The smoke test exercises the exact path the GUI uses — shared-core
//! `TextBuffer`, `live_preview` decorations, `AutoSaver` +
//! `save_atomically`, `TabManager`, `scan_folder` — but without a
//! window, so it runs in CI with no display. The GUI's `--smoke-test`
//! flag calls [`run_smoke_test`] directly.
//!
//! The IME simulation feeds composition-like input through the buffer
//! and decoration path: preedit text is inserted, edited mid-composition
//! (the egui issue #7908 scenario: Backspace during composition), and
//! then committed. It asserts the buffer is never corrupted. It does
//! NOT test the real winit IME event path — that needs a live window
//! and a real IME, which this VM does not have (see README).

use mica_shared_core::auto_save::{save_atomically, AutoSaver, SaveState};
use mica_shared_core::file_tree::scan_folder;
use mica_shared_core::live_preview::decorate_line;
use mica_shared_core::tabs::TabManager;
use mica_shared_core::text_buffer::{CursorPosition, TextBuffer};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One check result inside the smoke test. `name`/`detail` are
/// printed by [`run_smoke_test`] and shown in test failure output.
#[allow(dead_code)]
#[derive(Debug)]
pub struct SmokeCheck {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

/// Simulate one Japanese IME composition through the buffer path.
///
/// Sequence (mirrors what winit delivers to egui's `TextEdit`):
/// 1. Preedit starts: the in-progress reading is inserted at the
///    cursor, e.g. `こんにちは` while the user is still converting.
/// 2. Backspace during composition deletes exactly one preedit char
///    (egui #7908: on some platforms the Backspace is double-consumed
///    and deletes two chars — the buffer layer must delete exactly
///    one; the winit-side double-consume is outside this crate).
/// 3. Commit: the final text is in place; the buffer must contain it
///    verbatim and the decoration mapping must round-trip it.
///
/// Returns `(passed, detail)`.
pub fn simulate_ime_composition(buffer: &mut TextBuffer, at: CursorPosition) -> (bool, String) {
    // 1. Preedit insert: user typed "konnichiha" and the IME shows
    //    the reading as preedit text.
    let after_preedit = buffer.insert(at, "こんにちは");
    let preedit_text = buffer.to_text();
    if !preedit_text.contains("こんにちは") {
        return (false, format!("preedit text missing: {preedit_text:?}"));
    }

    // 2. Backspace during composition: delete exactly one char — the
    //    last preedit char ("は"). The buffer layer deletes one char;
    //    if egui/winit delivered the Backspace twice, the *widget*
    //    would delete two, which is the #7908 bug (not reproducible
    //    at this layer, but the assertion pins the correct behaviour).
    let before_backspace = buffer.to_text();
    let deleted = buffer.delete_range(
        CursorPosition {
            line: after_preedit.line,
            column: after_preedit.column.saturating_sub(1),
        },
        after_preedit,
    );
    if deleted != "は" {
        return (
            false,
            format!("backspace during composition deleted {deleted:?}, expected \"は\" (before: {before_backspace:?})"),
        );
    }
    if buffer.to_text().contains('は') {
        return (false, "preedit char survived backspace".to_string());
    }

    // 3. Commit: user re-types / confirms the final text.
    let commit_at = CursorPosition {
        line: after_preedit.line,
        column: after_preedit.column.saturating_sub(1),
    };
    let after_commit = buffer.insert(commit_at, "は、世界");
    let final_text = buffer.to_text();
    if !final_text.contains("こんにちは、世界") {
        return (
            false,
            format!("committed text missing from buffer: {final_text:?}"),
        );
    }

    // 4. Decorations must round-trip the committed Japanese text:
    //    display text is identical for a plain line, and the
    //    buffer<->display mapping is the identity.
    let line_index = after_commit.line;
    let line = buffer.line(line_index).unwrap_or("");
    let (display, _) = decorate_line(line, line_index, Some(line_index), false);
    if display.display_text != line {
        return (
            false,
            format!(
                "decoration changed committed text: {:?} -> {:?}",
                line, display.display_text
            ),
        );
    }
    for column in 0..=line.chars().count() {
        if display.buffer_column_for_display(column) != column
            || display.display_column_for_buffer(column) != column
        {
            return (
                false,
                format!("decoration mapping not identity at column {column} on {line:?}"),
            );
        }
    }

    (
        true,
        "preedit insert + backspace-during-composition (1 char) + commit こんにちは、世界, decoration round-trip OK".to_string(),
    )
}

/// Run the headless smoke test against a copy of a notes folder.
///
/// Steps: scan the folder, open the first `.md` file in a tab,
/// insert Japanese text (via the IME simulation), wait out the
/// auto-save debounce, save atomically, re-read from disk and compare.
/// Prints one `PASS`/`FAIL` line per check and returns `Ok(checks)`
/// with every check result, passed or not — use [`all_passed`] to
/// decide the outcome. `Err` means the run could not continue (the
/// first Markdown file could not be read).
pub fn run_smoke_test(folder: &Path) -> Result<Vec<SmokeCheck>, String> {
    let mut checks: Vec<SmokeCheck> = Vec::new();
    let mut check = |name: &'static str, passed: bool, detail: String| {
        println!(
            "{}: {} — {}",
            if passed { "PASS" } else { "FAIL" },
            name,
            detail
        );
        checks.push(SmokeCheck {
            name,
            passed,
            detail,
        });
    };

    // 1. Scan the folder.
    let entries = scan_folder(folder);
    let markdown_files: Vec<&PathBuf> = entries
        .iter()
        .filter(|entry| entry.is_markdown)
        .map(|entry| &entry.path)
        .collect();
    check(
        "scan_folder finds markdown files",
        !markdown_files.is_empty(),
        format!(
            "{} markdown file(s) under {}",
            markdown_files.len(),
            folder.display()
        ),
    );
    if markdown_files.is_empty() {
        return Ok(checks);
    }

    // 2. Open the first file in a tab (reopening must focus it).
    let first_file = markdown_files[0].clone();
    let mut tabs = TabManager::new();
    let first_index = tabs.open_file(&first_file);
    let second_index = tabs.open_file(&first_file);
    check(
        "one file = one tab",
        first_index == second_index && tabs.tabs().len() == 1,
        format!(
            "open twice -> tab {first_index} then {second_index}, {} tab(s)",
            tabs.tabs().len()
        ),
    );

    // 3. Load the buffer and run the IME simulation on it.
    let original = std::fs::read_to_string(&first_file)
        .map_err(|error| format!("cannot read {}: {error}", first_file.display()))?;
    let mut buffer = TextBuffer::from_text(&original);
    let insert_at = CursorPosition {
        line: buffer.line_count().saturating_sub(1),
        column: buffer
            .line(buffer.line_count() - 1)
            .map(|line| line.chars().count())
            .unwrap_or(0),
    };
    let (ime_passed, ime_detail) = simulate_ime_composition(&mut buffer, insert_at);
    check(
        "japanese ime composition simulation",
        ime_passed,
        ime_detail,
    );

    // 4. A heading line still decorates correctly after the edit.
    let (display, _) = decorate_line("# こんにちは", 0, Some(9), false);
    check(
        "live preview hides heading marker off cursor line",
        display.display_text == "こんにちは" && display.buffer_column_for_display(0) == 2,
        format!(
            "display text {:?}, first column maps to buffer {}",
            display.display_text,
            display.buffer_column_for_display(0)
        ),
    );

    // 5. Auto-save: debounce, atomic write, re-read from disk.
    let mut auto_saver = AutoSaver::with_delay(Duration::from_millis(10));
    auto_saver.note_edit();
    std::thread::sleep(Duration::from_millis(30));
    let due = auto_saver.save_is_due();
    let mut save_ok = false;
    let mut reread_ok = false;
    if due {
        auto_saver.begin_save();
        match save_atomically(&first_file, &buffer.to_text()) {
            Ok(()) => {
                auto_saver.finish_save();
                save_ok = auto_saver.state() == SaveState::Saved;
                match std::fs::read_to_string(&first_file) {
                    Ok(on_disk) => reread_ok = on_disk == buffer.to_text(),
                    Err(error) => {
                        check(
                            "re-read saved file from disk",
                            false,
                            format!("read failed: {error}"),
                        );
                    }
                }
            }
            Err(error) => {
                check(
                    "auto-save writes atomically",
                    false,
                    format!("save failed: {error}"),
                );
            }
        }
    }
    if due {
        check(
            "auto-save writes atomically",
            save_ok,
            format!("state after save: {:?}", auto_saver.state()),
        );
        check(
            "re-read saved file from disk",
            reread_ok,
            "disk content equals buffer content after save".to_string(),
        );
        // No temp file left behind.
        let temp_left = std::fs::read_dir(folder)
            .map(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .map(|name| name.contains("mica-tmp"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        check(
            "no temp file left behind",
            !temp_left,
            if temp_left {
                "a .mica-tmp file remains".to_string()
            } else {
                "folder is clean".to_string()
            },
        );
    } else {
        check(
            "auto-save debounce fires",
            false,
            "save was not due after the delay".to_string(),
        );
    }

    // 6. The buffer still holds the original content plus the edit.
    let final_text = buffer.to_text();
    check(
        "buffer keeps original content plus edit",
        keeps_original_plus_edit(&original, &final_text),
        format!("{} line(s) after edit", buffer.line_count()),
    );

    Ok(checks)
}

/// The final buffer must still start with the original content AND
/// contain the committed IME text. Both conditions are required, so
/// losing either the original or the edit fails the check.
fn keeps_original_plus_edit(original: &str, final_text: &str) -> bool {
    final_text.starts_with(original.trim_end_matches('\n'))
        && final_text.contains("こんにちは、世界")
}

/// `true` when every check passed.
pub fn all_passed(checks: &[SmokeCheck]) -> bool {
    checks.iter().all(|check| check.passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ime_composition_does_not_corrupt_buffer() {
        let mut buffer = TextBuffer::from_text("hello\nworld");
        let (passed, detail) =
            simulate_ime_composition(&mut buffer, CursorPosition { line: 1, column: 5 });
        assert!(passed, "{detail}");
        assert!(buffer.to_text().contains("こんにちは、世界"));
    }

    #[test]
    fn ime_composition_on_empty_document() {
        let mut buffer = TextBuffer::new();
        let (passed, detail) =
            simulate_ime_composition(&mut buffer, CursorPosition { line: 0, column: 0 });
        assert!(passed, "{detail}");
        assert_eq!(buffer.to_text(), "こんにちは、世界\n");
    }

    #[test]
    fn ime_backspace_deletes_exactly_one_char() {
        // The egui #7908 scenario at the buffer layer: a Backspace
        // delivered during composition must remove one char, not two.
        let mut buffer = TextBuffer::from_text("こんにちは");
        let deleted = buffer.delete_range(
            CursorPosition { line: 0, column: 4 },
            CursorPosition { line: 0, column: 5 },
        );
        assert_eq!(deleted, "は");
        assert_eq!(buffer.to_text(), "こんにち\n");
    }

    #[test]
    fn keeps_original_plus_edit_can_fail() {
        let original = "# Title\nbody\n";
        assert!(keeps_original_plus_edit(
            original,
            "# Title\nbodyこんにちは、世界\n"
        ));
        // Edit missing.
        assert!(!keeps_original_plus_edit(original, "# Title\nbody\n"));
        // Original content lost, edit present.
        assert!(!keeps_original_plus_edit(original, "こんにちは、世界\n"));
    }

    #[test]
    fn smoke_test_passes_on_a_temp_copy_of_sample_notes() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("sample-notes");
        let temp_dir =
            std::env::temp_dir().join(format!("mica-egui-smoke-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        for entry in std::fs::read_dir(&source).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), temp_dir.join(entry.file_name())).unwrap();
        }
        let checks = run_smoke_test(&temp_dir).expect("smoke test runs");
        assert!(all_passed(&checks), "smoke checks failed: {checks:?}");
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
