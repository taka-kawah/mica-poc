//! Tabs: one file = one tab.
//!
//! Opening a file that is already open focuses the existing tab
//! instead of opening a duplicate (Notion §3.3). There are no preview
//! tabs: opening a file always creates a pinned tab.

use std::path::{Path, PathBuf};

/// One open tab.
#[derive(Debug, Clone)]
pub struct Tab {
    /// The file this tab shows.
    pub path: PathBuf,
    /// Title shown on the tab (the file name).
    pub title: String,
    /// `true` when the buffer has unsaved changes.
    pub dirty: bool,
}

/// The tab list, with one active tab.
#[derive(Debug, Default)]
pub struct TabManager {
    tabs: Vec<Tab>,
    active: Option<usize>,
}

impl TabManager {
    /// Create an empty tab manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// All tabs, in order.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// The active tab, if any.
    pub fn active_tab(&self) -> Option<&Tab> {
        self.active.and_then(|index| self.tabs.get(index))
    }

    /// Open a file. When the file is already open, focus its tab and
    /// return its index; otherwise append a tab, focus it, and return
    /// the new index.
    pub fn open_file(&mut self, path: &Path) -> usize {
        if let Some(index) = self.tabs.iter().position(|tab| tab.path == path) {
            self.active = Some(index);
            return index;
        }
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("untitled")
            .to_string();
        self.tabs.push(Tab {
            path: path.to_path_buf(),
            title,
            dirty: false,
        });
        let index = self.tabs.len() - 1;
        self.active = Some(index);
        index
    }

    /// Focus the tab at `index`. Returns `false` when out of range.
    pub fn focus(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active = Some(index);
            true
        } else {
            false
        }
    }

    /// Focus the next tab (wrapping around). Returns `false` when empty.
    pub fn focus_next(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        let next = self
            .active
            .map(|index| (index + 1) % self.tabs.len())
            .unwrap_or(0);
        self.active = Some(next);
        true
    }

    /// Focus the previous tab (wrapping around).
    pub fn focus_previous(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        let previous = self
            .active
            .map(|index| (index + self.tabs.len() - 1) % self.tabs.len())
            .unwrap_or(0);
        self.active = Some(previous);
        true
    }

    /// Mark the tab for `path` dirty or clean. Returns `false` when
    /// the file is not open.
    pub fn set_dirty(&mut self, path: &Path, dirty: bool) -> bool {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.path == path) {
            tab.dirty = dirty;
            true
        } else {
            false
        }
    }

    /// Close the tab at `index`. Returns the closed tab, or `None`.
    /// The active tab moves to a neighbour when the active tab closes.
    pub fn close(&mut self, index: usize) -> Option<Tab> {
        if index >= self.tabs.len() {
            return None;
        }
        let closed = self.tabs.remove(index);
        self.active = match self.active {
            None => None,
            Some(active) if active == index => {
                if self.tabs.is_empty() {
                    None
                } else {
                    Some(index.min(self.tabs.len() - 1))
                }
            }
            Some(active) if active > index => Some(active - 1),
            Some(active) => Some(active),
        };
        Some(closed)
    }

    /// Close the active tab.
    pub fn close_active(&mut self) -> Option<Tab> {
        self.active.and_then(|index| self.close(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopening_focuses_existing_tab() {
        let mut manager = TabManager::new();
        let first = manager.open_file(Path::new("/notes/a.md"));
        manager.open_file(Path::new("/notes/b.md"));
        assert_eq!(manager.open_file(Path::new("/notes/a.md")), first);
        assert_eq!(manager.tabs().len(), 2);
        assert_eq!(manager.active_tab().unwrap().title, "a.md");
    }

    #[test]
    fn close_active_moves_to_neighbour() {
        let mut manager = TabManager::new();
        manager.open_file(Path::new("/notes/a.md"));
        manager.open_file(Path::new("/notes/b.md"));
        manager.close_active();
        assert_eq!(manager.active_tab().unwrap().title, "a.md");
    }
}
