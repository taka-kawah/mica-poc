# Mica — Notion Spec Summary

Structured summary of the Mica spec, distilled from the two Notion pages.
Written in English; Japanese terms and quotes are preserved verbatim where they matter.
No requirements have been added beyond what the Notion pages state.

## Sources

| Page | Title | ID | URL |
| --- | --- | --- | --- |
| Main | Mica 仕様メモ（自作マークダウンエディタ） | `419a547957908176a738d87bb871c387` | https://app.notion.com/p/Mica-419a547957908176a738d87bb871c387?source=copy_link |
| Child | ライブプレビューの仕組み（事前調査） | `419a547957908167b406e69abcd78011` | https://app.notion.com/p/419a547957908167b406e69abcd78011 |

- **Fetched:** 2026-11-15 19:22 JST (Notion fetch timestamps: main page as of 2026-11-15T10:10:54Z, child page as of 2026-11-15T07:32:59Z).
- **Fetch status:** Both pages fetched successfully and in full via `notion-cli notion-fetch`. No truncation or missing content was reported for either page. Nothing could not be fetched.
- **Status of the spec itself:** The main page states it is 「まだたたき台の段階」 (still a rough draft).

## 1. Project goal

Build a custom Markdown editor that keeps Obsidian's pleasant writing experience — 「Obsidianの『気持ちよく書ける体験』はそのままに」 — but is designed from the start for a WSL-first, keyboard-centric workflow with **no vault** (「WSL前提・キーボード中心・vaultなし」).

The motivation: Obsidian's writing feel is liked, but using it with WSL folders is unsatisfying in many ways. After comparing existing tools (Zed, VS Code, moving Obsidian's location, etc.), the author concluded that most of the dissatisfaction is rooted in Obsidian's fundamental design and cannot be fixed with settings or plugins.

### Explicitly out of scope (作らないもの)

- vault (a mechanism that creates a settings directory inside a folder)
- Graph view, Canvas
- Plugin system (for now — 「当面」)
- Sync features, mobile version
- Coding-oriented features (LSP, debugger, Git UI, etc.)

## 2. Keep vs. fix — Obsidian experiences

### Keep (残したい体験 — what Obsidian does well)

- Modern, simple UI with smooth motion （モダンでシンプルなUI、なめらかな動き）
- Live preview （カーソルのある行だけ記号が見え、他の行は整形表示）
- Proportional fonts （プロポーショナルフォント）
- Margins and line width (body max width limited, centered)
- Clean, uncluttered file tree （すっきりしたファイルツリー）

### Problems to solve (解決したい課題 — what Obsidian does badly)

- Cannot open folders inside WSL / file watching is unstable （WSL内のフォルダを開けない／ファイル監視が不安定）
- Keymap is managed per vault, cannot be shared globally （キーマップがvaultごとの管理で、全体共通にできない）
- Many situations require mouse operation （マウス操作が必要な場面が多い）
- Loose correspondence between files and tabs （ファイルとタブの対応関係がゆるい）
- The vault concept, graph view, and other unneeded features （vaultという概念、グラフビューなど不要な機能がある）

## 3. Feature priorities — MVP vs v1 vs later

Priority legend from the spec: **MVP** = required in the first version, **v1** = needed for daily use, **後** = if there is spare capacity.

### 3.1 Editor

| Feature | MVP | v1 | Later |
| --- | --- | --- | --- |
| Live preview — hide heading/bold/italic/link/code markers except on the cursor line, show formatted | ✅ | | |
| Proportional font — variable-width body font, monospace only in code blocks | ✅ | | |
| Body width limit — max width (e.g. 720px), centered, margins left/right | ✅ | | |
| Japanese IME — no display corruption while composing, candidate window in the correct position | ✅ | | |
| Auto-save — save after editing stops; show save state subtly | ✅ | | |
| Lists & checkboxes — Enter for next item, Tab/Shift+Tab to indent, toggle check | | ✅ | |
| Image display — inline display of relative-path images | | ✅ | |
| Tables — formatted display + simple editing | | | ✅ |
| Vim mode — optionally enabled | | | ✅ |

### 3.2 File tree

| Feature | MVP | v1 | Later |
| --- | --- | --- | --- |
| Open a folder — any folder as root (not a vault) | ✅ | | |
| Tree display — collapsible, restrained icons, thin indent guides | ✅ | | |
| Keyboard operation — move up/down, expand/collapse, open, create, rename, delete all by key | ✅ | | |
| Reflect external changes — additions/deletions/renames made in WSL reflected immediately | ✅ | | |
| Show current file — highlight and auto-expand the active tab's file in the tree | | ✅ | |
| Exclusion settings — hide `.git`, `node_modules`, etc. (user-configurable) | | ✅ | |
| Drag & drop to move | | | ✅ |

### 3.3 Tabs

| Feature | MVP | v1 | Later |
| --- | --- | --- | --- |
| 1 file = 1 tab — same file opens in only one tab; reopening moves to the existing tab | ✅ | | |
| No preview tabs — opening a file always creates a pinned tab (never swapped out) | ✅ | | |
| Keyboard tab movement — next/previous tab, by number, most-recently-used order | ✅ | | |
| External delete/rename — tab follows automatically or closes | | ✅ | |
| Tab order persistence — restore previously open tabs per folder (settings stored outside the folder) | | ✅ | |
| Split view — left/right 2-way split | | | ✅ |

### 3.4 Keyboard operation

| Feature | MVP | v1 | Later |
| --- | --- | --- | --- |
| Command palette — search and run all operations by command name | ✅ | | |
| Quick Open — fuzzy search by file name | ✅ | | |
| User-level keymap — one config file shared across all folders | ✅ | | |
| Focus movement — move between tree / editor / tabs by key | ✅ | | |
| Full-text search — search text within a folder | | ✅ | |

### 3.5 WSL integration / CLI

| Feature | MVP | v1 | Later |
| --- | --- | --- | --- |
| Launch from WSL — `mica .` or `mica note.md` in WSL opens the Windows-side GUI | ✅ | | |
| Send to existing window — if already running, open in that window | | ✅ | |
| Open Windows folders too — `/mnt/c/...` and launching from the Windows side | | | ✅ |

**MVP in brief** (per §9 of the spec): open a folder, file tree, tabs, live preview, auto-save, command palette, CLI launch.

## 4. Non-functional requirements

- **Smoothness （なめらかさ）**: scrolling and typing never stutter; follow the monitor's refresh rate.
- **Startup speed （起動の速さ）**: target under 1 second from command execution to window display.
- **Large files （大きいファイル）**: no input lag even with notes of several thousand lines.
- **Config location （設定の置き場所）**: all settings in the user directory, e.g. `~/.config/mica/`. Nothing is created inside an opened folder.
- **Data （データ）**: files are plain Markdown only. No proprietary format or database.

## 5. Architecture — Windows GUI + WSL backend over stdio RPC

Like VS Code and Zed: "screen is Windows, file operations are inside WSL" （「画面はWindows、ファイル操作はWSL内」に分ける）. The Windows app never watches WSL files directly, so the `\\wsl.localhost` file-watching problem does not occur.

```
[Windows]  GUI app (editor, file tree, tabs)
              ↑↓ stdio RPC via wsl.exe (see §11 of the spec)
[WSL]      Backend (file read/write, inotify change watching, search)
              ↑
           CLI `mica <path>`: starts the backend and tells the GUI "open this path"
```

### Backend (WSL side)

- File read / save / create / delete / rename
- inotify change watching and notification to the GUI
- File listing, full-text search
- Language candidates: Rust (`notify` crate) or Go

### GUI (Windows side)

- Display and editing only. Does not touch the filesystem directly.
- GUI technology candidates compared in §6 below.

### WSL integration design (spec §11) — Zed-style, NOT SSH

Neither VS Code nor Zed uses SSH for WSL. Both start a small server process inside the distribution via `wsl.exe` and send it commands from the Windows-side UI. SSH is the route for connecting to *another machine*; the server-side mechanism is shared.

- **VS Code's way:** Windows VS Code auto-installs and starts a VS Code Server (Node.js, `~/.vscode-server`) in WSL; UI and server communicate over a localhost port (WebSocket); server version must exactly match the client.
- **Zed's way:** Windows Zed starts a remote server (a single Rust binary in `~/.zed_server`) as a child process under `wsl.exe`; communication is **RPC over the child process's stdio** — no network port is opened; all file I/O goes through the server; the UI holds rendering, syntax parsing, and unsaved changes.

**Mica adopts Zed's approach (stdio).** Reasons: no port is opened, so no firewall or port-conflict worries and no authentication is needed (only the parent process can connect, since it is a child process). The same protocol can later be swapped to `ssh host mica-server` to open notes on an SSH host.

```
[Windows] Mica.exe (GUI)
   │  Launch: wsl.exe -d <distro> --exec ~/.mica_server/mica-server-<version>
   │  Communication: that child process's stdin / stdout (length-framed RPC messages)
   ▼
[WSL] mica-server (single Rust binary)
   ├─ File read/write, listing, search
   └─ inotify change watching → push notifications to the GUI
```

| Element | Design |
| --- | --- |
| Server distribution | Windows installer bundles a Linux binary (musl static link). On connect, check the version in `~/.mica_server/`; if missing, copy from `/mnt/c/...` (no network needed) |
| Protocol | Length-framed + JSON-RPC (readability first; switch to MessagePack etc. if slow) |
| Main RPCs | `open_folder` / `list_dir` / `read_file` / `write_file` / `rename` / `delete` / `search`; server→GUI notification `fs_changed` |
| Save safety | Write to a temp file, then rename (atomic save). Detect conflicts with external changes via modification time |
| On disconnect | Unsaved content stays on the GUI side; saved after reconnect |

### CLI — how `mica .` works from WSL

1. The `mica` shell script bundled with the Windows version is callable from WSL's PATH (using the default setting that carries the Windows PATH into WSL, or placed in `~/.local/bin`).
2. The script converts arguments to absolute paths with `realpath` and attaches the distribution name (`$WSL_DISTRO_NAME`) and user name.
3. Via WSL interop it runs `Mica.exe --wsl user@Ubuntu /home/user/notes`.
4. If Mica.exe is already running, it passes "open this" to the existing window via a named pipe and exits (single instance).
5. The GUI starts (or reuses) that distribution's mica-server and opens the folder.

### Config files (spec §7) — all under `~/.config/mica/`

```toml
# ~/.config/mica/keymap.toml
[global]
"ctrl+p"         = "quick_open"
"ctrl+shift+p"   = "command_palette"
"ctrl+b"         = "toggle_file_tree"
"ctrl+tab"       = "tab_recent_next"
"ctrl+w"         = "tab_close"
"ctrl+shift+f"   = "search_in_folder"

[file_tree]
"j"     = "cursor_down"
"k"     = "cursor_up"
"l"     = "expand_or_open"
"h"     = "collapse"
"a"     = "new_file"
"r"     = "rename"
```

```toml
# ~/.config/mica/settings.toml
[editor]
font_family = "Noto Sans JP"
font_size = 16
line_height = 1.8
max_width = 720
auto_save_delay_ms = 1000

[file_tree]
exclude = [".git", "node_modules", ".DS_Store"]
```

## 6. Technology selection (GUI)

Comparison of Tauri + CodeMirror 6 vs egui (Rust), plus GPUI as an additional candidate.

| Aspect | Tauri + CodeMirror 6 | egui (Rust) |
| --- | --- | --- |
| Smoothness | Good (WebView2 rendering) | Very good (direct GPU rendering) |
| Live preview | Easy with CodeMirror 6 decorations (same component as Obsidian) | Text editor part must be built from scratch — high difficulty |
| Japanese IME | Stable, rides on the browser's implementation | Needs verification; composing display / candidate position may be troublesome |
| Proportional font / margins | Freely adjustable with CSS | Possible, but needs tuning (e.g. Japanese font fallback) |
| Visual polish | Easy to fine-tune with CSS | Tends toward a distinctive look; effort needed to look modern |
| Memory / size | Heavier (WebView) | Light |
| Language | TypeScript frontend, Rust backend | All Rust |

### GPUI (Zed's UI framework) — additional candidate

Rust GPU-rendered UI framework built by the Zed team for Zed. In November 2026, Discord announced it is rewriting its desktop app from Electron to Rust + GPUI (explicitly not Tauri).

| Aspect | GPUI |
| --- | --- |
| Smoothness | Very good (GPU rendering; Zed targets 120fps) |
| Text editing | Proven — Zed itself is a text editor. But live-preview editing must be built from scratch |
| Japanese IME | On Windows, handles the Win32 IME API directly. Zed does Japanese input, so promising but needs verification |
| Text rendering | DirectWrite on Windows; Japanese font display quality expected to be high |
| Style of writing | Tailwind-like Rust API for styling; declarative View + low-level Element |
| Maturity | Pre-1.0, many breaking changes. Thin docs — "read the Zed source" state |
| License | GPUI itself is Apache-2.0. Zed's `editor` crate etc. are GPL, so reusing them would make Mica GPL too — needs checking |

**Current assessment (現時点の見立て): GPUI is the most promising candidate.** It is consistent with Mica's direction — smoothness, native, following Zed for WSL integration — and has a track record actually powering a text editor (Zed). Its weaknesses are maturity and scarce information. First verify "Japanese IME + variable-width-font multiline editing" with GPUI's `input` sample; if that fails, switch to Tauri + CodeMirror 6.

### Next steps (spec §9)

1. Build a small egui prototype of 「日本語IME入力＋可変幅フォントの複数行編集」 (Japanese IME input + variable-width-font multiline editing) and evaluate it.
2. Build the same prototype in Tauri + CodeMirror 6 and compare.
3. Decide the GUI technology.
4. Implement the MVP (open folder, file tree, tabs, live preview, auto-save, command palette, CLI launch).

### Existing-product research (spec §10, 2026-11)

**Conclusion: no existing product satisfies the whole spec.** Apps with live-preview writing feel and apps that correctly open WSL as a remote exist separately; none does both.

| Product | Live preview | Opens WSL as remote | Notes |
| --- | --- | --- | --- |
| Obsidian | ◎ | × | `\\wsl.localhost` file watching unstable. Closed source |
| Zed | × (preview in a separate tab) | ◎ | Code editor. Its WSL mechanism is a reference |
| VS Code + WYSIWYG extensions | △ (depends on extension) | ◎ | Closest existing combination, but the "code editor feel" remains |
| mdedit | ○ (CodeMirror Obsidian-style Live mode) | × | Tauri 2 + Svelte 5. Useful as a **reference implementation for the Tauri plan** |
| Ferrite | ○ (WYSIWYG) | × | Rust + egui. Useful as a **reference implementation for the egui plan** (want to check its Japanese IME) |
| Typora / MarkText / Zettlr, etc. | ○〜◎ | × | Run as Windows apps, so the same WSL file-watching problem |

**Approach:** continue building it ourselves. Actually try mdedit (Tauri + CodeMirror) and Ferrite (egui) as decision material for §6.

## 7. Live-preview mechanism (child page)

### Core idea

Live preview does **not** convert to HTML for display. **The content stays plain text; only the presentation changes** （中身はただのプレーンテキストのまま、「見せ方」だけを変えている）. The editing target is always the raw Markdown, so saving and undo history work exactly like an ordinary text editor.

Example — the line `## 見出し`:

- File contents stay `## 見出し`
- On screen, `## ` is hidden and `見出し` is drawn large
- Only when the cursor is on that line is `## ` shown

### Decoration model

Parse the buffer into a syntax tree, then compute decorations from it:

- **Style:** this range is bold, this line is large type
- **Hide:** markers such as `#` and `**` (but not on the line/element where the cursor is)
- **Replace:** images, checkboxes, horizontal rules displayed as widgets

### The 5-stage pipeline

```txt
① Text buffer (raw Markdown. Editing happens only here)
      ↓ On each edit, re-parse only "the part that changed"
② Syntax tree (headings, bold, links… and the positions of their marker parts)
      ↓ Only the visible range
③ Decoration computation
      ・Style: this range is bold, this line is large
      ・Hide: # and ** markers (but don't hide on the cursor's line/element)
      ・Replace: images, checkboxes, rules shown as widgets (components)
      ↓
④ Display coordinate mapping (buffer position ⇔ screen position)
      ↓
⑤ Rendering (layout with different font sizes / line heights per line)
```

Obsidian's editor component, CodeMirror 6, provides stages ②–⑤ almost entirely, so only "the decoration rules" need to be written. GPUI has no components for ②–④, so they must be combined by hand.

### Components and GPUI candidates

| Component | Role | CodeMirror 6 (Obsidian) | GPUI candidate |
| --- | --- | --- | --- |
| Text buffer | Fast insert/delete even in large documents | Built-in tree-structured text | Rope structure (e.g. `ropey`) |
| Parser | Builds the syntax tree. **Incremental update** capability is important | Lezer (markdown) | tree-sitter (`tree-sitter-markdown`) |
| Decoration computation | Decides "what to show how" from syntax tree + cursor position | Decoration (mark / replace / widget) | Build from scratch |
| Coordinate mapping | Skips hidden markers to decide cursor movement / click position | Built-in | Build from scratch (Zed's `DisplayMap` is the same idea; GPL, so use only its design as reference) |
| Rendering | Lays out styled strings line by line | DOM | GPUI text rendering (pass styled spans) |
| IME | Display of composing text and candidate-window position | Left to the browser | Implement in GPUI's input handler |

### Hard parts (難所メモ)

- **Rule for where to show markers:** line-based (show everything on the cursor line) vs element-based (show only the `**` of the bold element the cursor is in). Obsidian leans element-based. Line-based is simpler for MVP.
- **Cursor movement across hidden markers:** left/right keys must not stop on "invisible characters".
- **Variable line heights:** headings and images change line height, complicating scroll-position calculation.
- **Long-document speed:** parse incrementally; restrict decoration and rendering to the visible range.
- **Japanese IME input:** while composing, hold back decoration updates so re-computation does not make the display flicker.

### Staged build plan (段階的な作り方 — each stage leaves something usable)

1. Plain text editor (buffer + cursor + IME)
2. Build a syntax tree with tree-sitter and change only colour/bold (markers stay visible — already quite readable at this point)
3. Change heading font sizes (line height becomes variable)
4. Hide markers except on the cursor line (introduce coordinate mapping)
5. Widgets for images, checkboxes, etc.

## 8. Undecided items (未決事項)

- [ ] Which GUI: GPUI / Tauri + CodeMirror 6 / egui (first candidate GPUI; decided by the Japanese IME verification result)
- [ ] Backend language (Rust / Go)
- [x] **App name and CLI command name → Mica** （雲母。黒曜石のパロディ＋Windows 11のMica素材）; CLI is `mica` — **decided**
- [ ] Message format between WSL backend and Windows GUI (JSON-RPC / binary) — the transport is decided: stdio (§11)
- [ ] Whether to support Obsidian-style `[[wiki links]]`
- [ ] How themes (light/dark) are handled

## 9. Notes / caveats

- The spec is a draft （たたき台）; priorities and details may change.
- The GUI decision (GPUI vs Tauri + CodeMirror 6 vs egui) is explicitly gated on Japanese IME verification, not yet made.
- License caution recorded in the spec: Zed's `editor` crate is GPL — reusing it would make Mica GPL. Only the design of Zed's `DisplayMap` should be referenced, not the code.
- Reference links in spec §11: Microsoft Learn "WSL で VS Code の使用を開始する", VS Code Docs "Developing in WSL", VS Code API "Supporting Remote Development", Zed Docs "Remote Development", Zed Blog "Zed for Windows is here", mdedit (winget), awesome-markdown-editors (GitHub).
