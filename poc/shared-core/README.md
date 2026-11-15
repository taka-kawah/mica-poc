# mica-shared-core

Framework-independent core of the Mica Markdown editor PoC.
Every PoC (Tauri + CodeMirror 6, GPUI, egui) builds on this crate,
so the live-preview logic, file model and WSL protocol are identical
everywhere and testable without a display.

## Design

From the Notion spec 「Mica 仕様メモ（自作マークダウンエディタ）」 and
the child page 「ライブプレビューの仕組み（事前調査）」:

- The text buffer always holds **raw Markdown**. Live preview only
  changes how lines are *shown* — it never rewrites the buffer or
  the file. Editing, saving and undo therefore work like a plain
  text editor.
- Live preview pipeline: buffer → parse one line → decorations →
  coordinate mapping → render. Markers (`## `, `**`, …) are hidden
  on every line **except the cursor line** (row-level rule for the
  PoC; element-level hiding is a later step).
- File model: any folder can be opened (no vault), one file = one
  tab, saves are atomic (temp file + rename).
- One global keymap file (`~/.config/mica/keymap.toml`), never one
  per folder. Command palette (Ctrl+Shift+P) and Quick Open (Ctrl+P)
  share one fuzzy matcher.
- WSL bridge: the Windows GUI talks to a small WSL server over the
  child process's stdio with length-framed JSON-RPC (Zed-style).
  SSH is explicitly **not** used for WSL. On Linux the PoCs use the
  in-process `LocalBridgeBackend` in `wsl_bridge.rs`; the Windows
  transport (`wsl.exe -d <distro> --exec ~/.mica_server/mica-server-<version>`)
  is documented here, not faked.

## Modules

| Module | Responsibility |
| --- | --- |
| `text_buffer` | Line-based buffer; positions in `char`s so Japanese text is safe |
| `markdown_parser` | Line structure: headings, bold/italic/code/link, lists, checkboxes, quotes, rules, fenced code |
| `live_preview` | Display segments + buffer↔display column mapping; heading sizes; hidden markers |
| `file_tree` | Recursive `.md` scan; ignores `.git`, `node_modules`, `.DS_Store` |
| `tabs` | One file = one tab; reopening focuses the existing tab |
| `auto_save` | Debounced save (default 1000 ms), atomic write, save state |
| `keymap` | Global TOML keymap; area bindings win over global ones |
| `command_palette` | Subsequence fuzzy match, tight/early matches rank higher |
| `wsl_bridge` | Length-framed JSON-RPC, method names, local-filesystem backend, `..` escape guard |

## Parser coverage (documented subset)

The parser is line-oriented and covers: headings `#`–`######`,
bold `**…**`, italic `*…*`, code spans `` `…` ``, links
`[text](url)`, unordered/ordered list items, checkboxes
`- [ ]` / `- [x]`, block quotes `>`, horizontal rules, fenced code
blocks. It does **not** cover: nested emphasis, tables, footnotes,
HTML blocks, or multi-line constructs. A full incremental parser
(tree-sitter-markdown) is an MVP step, not a PoC step.

## Run the tests

```sh
cargo test -p mica-shared-core
```

No display is needed. Tests cover: Japanese insert/delete,
heading/bold marker hiding, cursor mapping over hidden markers,
checkbox parsing, code-fence state, tab reopening, atomic save,
keymap lookup, fuzzy ranking, bridge framing and the `..` escape guard.

## License

MIT OR Apache-2.0. No GPL code. Zed's `editor` crate is GPL and is
not used or copied here; its display-mapping idea is referenced as
design only.
