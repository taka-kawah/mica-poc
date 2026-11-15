//! File tree: list the Markdown files under a folder.
//!
//! Any folder can be opened — there is no vault. Hidden entries
//! (`.git`, `node_modules`, `.DS_Store`, …) are excluded by default
//! and can be extended through settings (see Notion §3.2).

use std::path::{Path, PathBuf};

/// Entries that are never shown in the tree.
pub const DEFAULT_EXCLUDED_NAMES: &[&str] = &[".git", "node_modules", ".DS_Store", ".obsidian"];

/// One entry in the file tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeEntry {
    /// Full path.
    pub path: PathBuf,
    /// Name shown in the tree.
    pub name: String,
    /// `true` for a folder, `false` for a file.
    pub is_folder: bool,
    /// Depth below the opened folder (0 = direct child).
    pub depth: usize,
    /// File entries only: `true` when the file is Markdown (`.md`).
    pub is_markdown: bool,
}

/// Scan a folder recursively and return its entries, folders first,
/// then files, each group sorted by name (case-insensitive).
/// Missing folders return an empty list; I/O errors on single entries
/// are skipped rather than failing the whole scan, so one unreadable
/// file never breaks the tree.
pub fn scan_folder(root: &Path) -> Vec<FileTreeEntry> {
    scan_folder_with_exclusions(root, DEFAULT_EXCLUDED_NAMES)
}

/// Scan a folder with extra excluded names (added to the defaults).
pub fn scan_folder_with_exclusions(root: &Path, extra_excluded: &[&str]) -> Vec<FileTreeEntry> {
    let mut excluded: Vec<&str> = DEFAULT_EXCLUDED_NAMES.to_vec();
    excluded.extend_from_slice(extra_excluded);
    let mut entries = Vec::new();
    scan_recursive(root, root, 0, &excluded, &mut entries);
    entries
}

fn scan_recursive(
    root: &Path,
    dir: &Path,
    depth: usize,
    excluded: &[&str],
    entries: &mut Vec<FileTreeEntry>,
) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(_) => return,
    };
    let mut children: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| !excluded.contains(&name))
                .unwrap_or(false)
        })
        .collect();
    children.sort_by_key(|path| {
        (
            !path.is_dir(),
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_lowercase(),
        )
    });
    for path in children {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        let is_folder = path.is_dir();
        entries.push(FileTreeEntry {
            path: path.clone(),
            name,
            is_folder,
            depth,
            is_markdown: !is_folder
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| extension.eq_ignore_ascii_case("md"))
                    .unwrap_or(false),
        });
        if is_folder {
            let _ = root; // root is kept for future relative-path display.
            scan_recursive(root, &path, depth + 1, excluded, entries);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_lists_markdown_and_ignores_git() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::create_dir(dir.path().join("notes")).unwrap();
        fs::write(dir.path().join("notes/a.md"), "# A").unwrap();
        fs::write(dir.path().join("notes/b.txt"), "plain").unwrap();
        fs::write(dir.path().join("top.md"), "# Top").unwrap();
        let entries = scan_folder(dir.path());
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"notes"));
        assert!(names.contains(&"a.md"));
        assert!(names.contains(&"top.md"));
        assert!(!names.contains(&".git"));
        let markdown = entries.iter().find(|entry| entry.name == "a.md").unwrap();
        assert!(markdown.is_markdown);
        let text = entries.iter().find(|entry| entry.name == "b.txt").unwrap();
        assert!(!text.is_markdown);
    }

    #[test]
    fn missing_folder_returns_empty_list() {
        assert!(scan_folder(Path::new("/definitely/not/a/real/folder")).is_empty());
    }
}
