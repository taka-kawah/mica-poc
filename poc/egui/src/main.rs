//! `mica-egui` — the Mica Markdown editor PoC on egui + eframe 0.37.
//!
//! Usage:
//! ```sh
//! mica-egui [folder] [--quit-after <seconds>]
//! mica-egui --smoke-test [folder]
//! ```
//!
//! `--smoke-test` runs the headless smoke test (scan, open, edit with
//! a Japanese IME simulation, auto-save, re-read from disk) with no
//! display and prints `PASS`/`FAIL` per check. It works on a temporary
//! copy of the folder so the original notes are never modified.

mod app;
mod document;
mod editor_view;
mod smoke_test;

use app::MicaApp;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

fn print_usage() {
    eprintln!(
        "mica-egui — Mica Markdown editor PoC (egui + eframe 0.37)\n\
         \n\
         Usage:\n\
         \x20 mica-egui [folder] [--quit-after <seconds>]\n\
         \x20 mica-egui --smoke-test [folder]\n\
         \n\
         folder defaults to the shared sample notes next to this binary's\n\
         workspace (poc/../../sample-notes) when it exists, otherwise the\n\
         current directory. --smoke-test runs headless (no display) on a\n\
         temporary copy of the folder and prints PASS/FAIL per check.\n\
         --quit-after closes the window after N seconds (used by the\n\
         xvfb smoke run)."
    );
}

fn default_folder() -> PathBuf {
    let sample_notes = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("sample-notes");
    if sample_notes.is_dir() {
        sample_notes
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut folder: Option<PathBuf> = None;
    let mut smoke_test = false;
    let mut quit_after: Option<Duration> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke-test" => smoke_test = true,
            "--quit-after" => {
                let seconds: f64 = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0.0);
                quit_after = Some(Duration::from_secs_f64(seconds.max(0.0)));
            }
            "--help" | "-h" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            other if other.starts_with('-') => {
                eprintln!("unknown option: {other}");
                print_usage();
                return ExitCode::FAILURE;
            }
            other => folder = Some(PathBuf::from(other)),
        }
    }

    let folder = folder.unwrap_or_else(default_folder);

    if smoke_test {
        return run_headless_smoke_test(&folder);
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_title("Mica — egui PoC"),
        ..Default::default()
    };
    let quit_after_value = quit_after;
    match eframe::run_native(
        "Mica (egui PoC)",
        native_options,
        Box::new(move |creation_context| {
            MicaApp::apply_style(&creation_context.egui_ctx);
            Ok(Box::new(MicaApp::new(folder.clone(), quit_after_value)))
        }),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mica-egui failed to start: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Headless smoke test: copy `folder` to a temp dir (the original is
/// never modified), run the checks, print the result, exit 0/1.
fn run_headless_smoke_test(folder: &std::path::Path) -> ExitCode {
    let temp_dir = std::env::temp_dir().join(format!("mica-egui-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    if let Err(error) = std::fs::create_dir_all(&temp_dir) {
        eprintln!(
            "FAIL: cannot create temp dir {}: {error}",
            temp_dir.display()
        );
        return ExitCode::FAILURE;
    }
    if let Err(error) = copy_folder_contents(folder, &temp_dir) {
        eprintln!("FAIL: cannot copy {}: {error}", folder.display());
        let _ = std::fs::remove_dir_all(&temp_dir);
        return ExitCode::FAILURE;
    }
    println!(
        "smoke test folder: {} (copy of {})",
        temp_dir.display(),
        folder.display()
    );
    let outcome = smoke_test::run_smoke_test(&temp_dir);
    let _ = std::fs::remove_dir_all(&temp_dir);
    match outcome {
        Ok(checks) => {
            let passed = checks.iter().filter(|check| check.passed).count();
            println!("smoke test: {passed}/{} checks passed", checks.len());
            if smoke_test::all_passed(&checks) {
                println!("SMOKE TEST: PASS");
                ExitCode::SUCCESS
            } else {
                println!("SMOKE TEST: FAIL");
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            println!("FAIL: smoke test error: {error}");
            println!("SMOKE TEST: FAIL");
            ExitCode::FAILURE
        }
    }
}

/// Recursively copy a folder's contents (one level of subfolders is
/// enough for the sample notes, but this handles nesting anyway).
fn copy_folder_contents(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target_path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target_path)?;
            copy_folder_contents(&entry.path(), &target_path)?;
        } else {
            std::fs::copy(entry.path(), target_path)?;
        }
    }
    Ok(())
}
