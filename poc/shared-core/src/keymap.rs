//! Keymap: one global key binding file for the whole app.
//!
//! The keymap lives in a single file for all folders
//! (`~/.config/mica/keymap.toml`, Notion §7) — never one per vault,
//! because Mica has no vaults. Bindings map a key description such as
//! `ctrl+p` to a command name such as `quick_open`.
//!
//! Key descriptions are normalised to lower case, with `+` separators
//! (`Ctrl+P` and `ctrl+p` are the same binding). The PoC parses the
//! TOML `[global]` table and optional per-area tables (`[file_tree]`,
//! …); a binding in an area table wins over the global one when the
//! focus is in that area.

use std::collections::HashMap;

/// One command name, for example `quick_open` or `tab_close`.
pub type CommandName = String;

/// A keymap: key description -> command name, per area.
#[derive(Debug, Default, Clone)]
pub struct Keymap {
    /// Bindings that apply everywhere.
    global: HashMap<String, CommandName>,
    /// Bindings that apply only in one area (`file_tree`, `editor`, …).
    areas: HashMap<String, HashMap<String, CommandName>>,
}

impl Keymap {
    /// Parse a keymap from TOML text. Unknown tables are kept as area
    /// tables; unknown keys inside a table are ignored gracefully so
    /// one typo never breaks the whole file.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let value: toml::Value =
            toml::from_str(text).map_err(|error: toml::de::Error| error.to_string())?;
        let mut keymap = Keymap::default();
        let table = value
            .as_table()
            .ok_or_else(|| "keymap must be a TOML table".to_string())?;
        for (section, section_value) in table {
            let bindings = section_value
                .as_table()
                .ok_or_else(|| format!("keymap section [{section}] must be a table"))?;
            for (key, command) in bindings {
                let command = command
                    .as_str()
                    .ok_or_else(|| format!("keymap binding {key} must be a string"))?
                    .to_string();
                let normalised = normalise_key(key);
                if section == "global" {
                    keymap.global.insert(normalised, command);
                } else {
                    keymap
                        .areas
                        .entry(section.clone())
                        .or_default()
                        .insert(normalised, command);
                }
            }
        }
        Ok(keymap)
    }

    /// Look up the command for a key press. `area` is the focused area
    /// (`editor`, `file_tree`, …); area bindings win over global ones.
    /// Returns `None` when the key is unbound.
    pub fn command_for_key(&self, area: &str, key: &str) -> Option<&str> {
        let normalised = normalise_key(key);
        if let Some(area_bindings) = self.areas.get(area) {
            if let Some(command) = area_bindings.get(&normalised) {
                return Some(command);
            }
        }
        self.global.get(&normalised).map(String::as_str)
    }

    /// The built-in default keymap from Notion §7.
    pub fn default_keymap() -> Self {
        Self::from_toml(DEFAULT_KEYMAP_TOML).expect("default keymap is valid TOML")
    }
}

/// Lower-case a key description and normalise its separators.
fn normalise_key(key: &str) -> String {
    key.trim().to_lowercase().replace(' ', "")
}

/// The default keymap, taken from Notion §7.
pub const DEFAULT_KEYMAP_TOML: &str = r#"[global]
"ctrl+p"       = "quick_open"
"ctrl+shift+p" = "command_palette"
"ctrl+b"       = "toggle_file_tree"
"ctrl+tab"     = "tab_recent_next"
"ctrl+w"       = "tab_close"
"ctrl+shift+f" = "search_in_folder"

[file_tree]
"j" = "cursor_down"
"k" = "cursor_up"
"l" = "expand_or_open"
"h" = "collapse"
"a" = "new_file"
"r" = "rename"
"#;

/// Built-in command names the PoCs understand.
pub const KNOWN_COMMANDS: &[&str] = &[
    "quick_open",
    "command_palette",
    "toggle_file_tree",
    "tab_recent_next",
    "tab_close",
    "search_in_folder",
    "cursor_down",
    "cursor_up",
    "expand_or_open",
    "collapse",
    "new_file",
    "rename",
    "save",
    "open_folder",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_and_area_lookup() {
        let keymap = Keymap::default_keymap();
        assert_eq!(
            keymap.command_for_key("editor", "Ctrl+P"),
            Some("quick_open")
        );
        assert_eq!(
            keymap.command_for_key("editor", "ctrl+shift+p"),
            Some("command_palette")
        );
        assert_eq!(
            keymap.command_for_key("file_tree", "j"),
            Some("cursor_down")
        );
        assert_eq!(keymap.command_for_key("editor", "j"), None);
        assert_eq!(keymap.command_for_key("editor", "ctrl+z"), None);
    }

    #[test]
    fn invalid_toml_is_reported() {
        assert!(Keymap::from_toml("[global\n").is_err());
    }
}
