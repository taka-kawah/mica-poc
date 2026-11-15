//! Auto-save: save a dirty document after editing stops, atomically.
//!
//! The PoC rule (Notion §3.1): save automatically after editing
//! stops, and show the save state discreetly. Saving is atomic:
//! write to a temporary file in the same folder, then rename it over
//! the target, so a crash can never leave a half-written file
//! (Notion §11, 「保存の安全性」).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The save state shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveState {
    /// No unsaved changes.
    Clean,
    /// Unsaved changes, waiting for the debounce delay.
    Dirty,
    /// A save is in progress.
    Saving,
    /// Just saved (the UI may show this briefly, then go Clean).
    Saved,
}

/// Default delay between the last edit and the save: 1000 ms
/// (Notion §7, `auto_save_delay_ms = 1000`).
pub const DEFAULT_AUTO_SAVE_DELAY: Duration = Duration::from_millis(1000);

/// Tracks whether a document needs saving and when it is due.
#[derive(Debug)]
pub struct AutoSaver {
    delay: Duration,
    last_edit: Option<Instant>,
    state: SaveState,
}

impl Default for AutoSaver {
    fn default() -> Self {
        Self::new()
    }
}

impl AutoSaver {
    /// Create an auto-saver with the default delay.
    pub fn new() -> Self {
        Self::with_delay(DEFAULT_AUTO_SAVE_DELAY)
    }

    /// Create an auto-saver with a custom delay (useful for tests).
    pub fn with_delay(delay: Duration) -> Self {
        Self {
            delay,
            last_edit: None,
            state: SaveState::Clean,
        }
    }

    /// Current save state.
    pub fn state(&self) -> SaveState {
        self.state
    }

    /// Record an edit. The document becomes Dirty and the save timer
    /// restarts.
    pub fn note_edit(&mut self) {
        self.last_edit = Some(Instant::now());
        self.state = SaveState::Dirty;
    }

    /// `true` when the debounce delay has passed since the last edit
    /// and a save is due.
    pub fn save_is_due(&self) -> bool {
        self.state == SaveState::Dirty
            && self
                .last_edit
                .map(|edited| edited.elapsed() >= self.delay)
                .unwrap_or(false)
    }

    /// Mark the start of a save.
    pub fn begin_save(&mut self) {
        self.state = SaveState::Saving;
    }

    /// Mark a successful save. The timer is cleared.
    pub fn finish_save(&mut self) {
        self.state = SaveState::Saved;
        self.last_edit = None;
    }

    /// After the UI has shown the Saved state, go back to Clean.
    pub fn acknowledge_saved(&mut self) {
        if self.state == SaveState::Saved {
            self.state = SaveState::Clean;
        }
    }
}

/// Save `content` to `path` atomically: write a temporary file next
/// to the target, flush and sync it, then rename over the target.
pub fn save_atomically(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.md");
    let temp_path: PathBuf = parent.join(format!(".{file_name}.mica-tmp"));
    {
        let mut temp_file = std::fs::File::create(&temp_path)?;
        temp_file.write_all(content.as_bytes())?;
        temp_file.sync_all()?;
    }
    std::fs::rename(&temp_path, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn save_is_due_after_delay() {
        let mut saver = AutoSaver::with_delay(Duration::from_millis(1));
        saver.note_edit();
        assert!(!saver.save_is_due());
        std::thread::sleep(Duration::from_millis(5));
        assert!(saver.save_is_due());
        saver.begin_save();
        assert_eq!(saver.state(), SaveState::Saving);
        saver.finish_save();
        assert_eq!(saver.state(), SaveState::Saved);
        saver.acknowledge_saved();
        assert_eq!(saver.state(), SaveState::Clean);
    }

    #[test]
    fn atomic_save_writes_full_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "old").unwrap();
        save_atomically(&path, "new\ncontent\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\ncontent\n");
        // No temporary file is left behind.
        assert!(fs::read_dir(dir.path()).unwrap().count() == 1);
    }
}
