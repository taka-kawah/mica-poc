//! `mica-shared-core`: framework-independent core of the Mica editor.
//!
//! Every PoC (Tauri + CodeMirror 6, GPUI, egui) builds on this crate,
//! so the live-preview logic, file model and WSL protocol are the
//! same everywhere and can be tested without a display.
//!
//! Design (from the Notion spec 「Mica 仕様メモ」 and the child page
//! 「ライブプレビューの仕組み」):
//!
//! * The [`text_buffer`] always holds raw Markdown. Live preview only
//!   changes how lines are shown, never the buffer or the file.
//! * [`markdown_parser`] finds the structure of one line
//!   (headings, bold, lists, checkboxes, …) as a documented subset.
//! * [`live_preview`] turns a parsed line into display segments and
//!   a buffer↔display coordinate map. Markers are hidden except on
//!   the cursor line (row-level rule for the PoC).
//! * [`file_tree`], [`tabs`] and [`auto_save`] are the file model:
//!   any folder can be opened (no vault), one file = one tab, and
//!   saves are atomic (temp file + rename).
//! * [`keymap`] loads one global TOML keymap
//!   (`~/.config/mica/keymap.toml`); [`command_palette`] fuzzy-matches
//!   commands and files for Ctrl+P / Ctrl+Shift+P.
//! * [`wsl_bridge`] implements the stdio JSON-RPC protocol between
//!   the Windows GUI and the WSL backend (Zed-style, not SSH), plus
//!   an in-process local-filesystem backend for Linux testing.
//!
//! License: MIT OR Apache-2.0. No GPL code is used anywhere in this
//! crate. Zed's editor internals are GPL and are referenced only as a
//! design idea (display mapping), never copied.

pub mod auto_save;
pub mod command_palette;
pub mod file_tree;
pub mod keymap;
pub mod live_preview;
pub mod markdown_parser;
pub mod tabs;
pub mod text_buffer;
pub mod wsl_bridge;
