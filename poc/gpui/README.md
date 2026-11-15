# mica-gpui-poc — GPUI PoC for Mica

A minimal Markdown editor in one GPUI window, built on
`mica-shared-core`: file tree, tab bar (one file = one tab), a
multi-line live-preview editor, status bar with save state, command
palette (Ctrl+Shift+P) and Quick Open (Ctrl+P), auto-save, and a
global TOML keymap. It is a **proof of concept that measures the
cost of building an editor on GPUI**, not a full editor.

- GUI entry: `src/main.rs` (CLI, `--smoke-test`, GPUI bootstrap)
- Headless model: `src/model.rs` (shared-core wiring, UTF-16
  conversions for the input handler, auto-save, palette)
- GPUI view: `src/view.rs` (rendering, custom editor element,
  `EntityInputHandler`, keybindings from the keymap file)
- License of Mica code: MIT OR Apache-2.0.

## Dependency decision

| Choice | Version | Reason |
|---|---|---|
| `gpui` (package `gpui-pre`) | `=0.3.12` | Longbridge snapshot of Zed main (`zed@d2a4fb6`, published 9 Nov 2026), Apache-2.0. The official crates.io `gpui` is `0.2.2` (22 Oct 2025, ~13 months stale as of Nov 2026) and predates the wgpu Linux renderer, the crate split, and a year of fixes — it was not tried beyond this reasoning, because the research (see `docs/research-2026-11.md` §1.3) already showed it is a dead branch for current GPUI. A git dependency on `zed-industries/zed` would also work, but a pinned crates.io release is more reproducible for a PoC. |
| `gpui_platform` (package `gpui-pre-platform`) | `=0.3.12`, features `["wayland", "x11"]` | Same snapshot family. **The features are load-bearing — see the gotcha below.** |

### Gotcha: `gpui-pre-platform` defaults do not include a display backend

`gpui-pre-platform`'s `default` features are just `["multithreaded"]`;
`wayland` and `x11` are opt-in, and it depends on `gpui-pre-linux`
with `default-features = false`. With neither feature enabled,
`gpui-pre-linux` builds with **no display backend at all** and the
app silently starts headless: the process runs, `open_window`
succeeds, but no window ever appears and no connection to the X
server is even attempted (verified with `strace`: no `connect()` to
the X socket, no open of any Wayland socket, process idle in
`epoll_wait`). There is no error message. If the window does not
appear, check the features first. This cost ~40 minutes of
debugging; it is the single most important finding for anyone
repeating this PoC.

### What was NOT used (license boundary)

Read `docs/research-2026-11.md` §8 twice before writing any code
here, as instructed. The rules, as applied:

- **GPUI itself is Apache-2.0 and safe to use.** This PoC depends
  only on the `gpui-pre` family (`gpui-pre`, `gpui-pre-platform`,
  `gpui-pre-linux`, `gpui-pre-wgpu`, …) plus `mica-shared-core`.
- **No Zed editor crate is copied, vendored, or depended on.**
  Zed's `editor`, `text`, `rope`, `language`, `multi_buffer`,
  `workspace`, `ui`, `ui_input`, … crates are GPL-3.0-or-later; a
  distributed Mica binary linking any of them would be a GPL work.
  Verified in `Cargo.lock` (757 packages): none of those crate
  names appear. The only `zed-*` names in the tree are small
  permissive support crates pulled in by GPUI itself:
  `zed-font-kit` (MIT OR Apache-2.0), `zed-scap` (MIT),
  `zed-xim` (MIT) — checked in their registry `Cargo.toml` files.
- **Design-only reference.** The shaped-line-per-paragraph
  rendering, the display-coordinate mapping, and the
  `EntityInputHandler` shape of the editor come from the *concepts*
  in GPUI's own Apache-2.0 `examples/input.rs` sample and from the
  shared-core decoration model (which itself notes Zed's DisplayMap
  as design-only). No Zed editor code was read for copying, pasted,
  or transliterated; no Zed source lives in this repo.
- `gpui-component` was not needed: the PoC's widgets (tree, tabs,
  palette) are a few dozen lines of `div()`s each.

## How to run

```sh
# GUI: open a folder (defaults to ../../sample-notes)
cargo run -p mica-gpui-poc -- ~/workspace/mica-poc/sample-notes

# Headless smoke test (no display needed): open the folder, open a
# file, edit it (including Japanese text), auto-save, re-read from
# disk and verify. The edited file is restored afterwards, so the
# shared sample-notes fixture is not mutated.
cargo run -p mica-gpui-poc -- --smoke-test ~/workspace/mica-poc/sample-notes

# Unit tests (headless)
cargo test -p mica-gpui-poc
cargo test -p mica-shared-core
```

Keyboard (defaults from Notion §7; overridden by
`~/.config/mica/keymap.toml` when present — the `[global]` table is
turned into GPUI bindings at startup):

| Key | Action |
|---|---|
| Ctrl+P | Quick Open (fuzzy file search) |
| Ctrl+Shift+P | Command palette |
| Ctrl+B | Toggle file tree |
| Ctrl+W | Close active tab (unsaved edits are saved first; if that fails the tab stays open and the status bar says why) |
| Ctrl+Tab | Next tab |
| Ctrl+S | Save now (auto-save also runs) |
| F6 | Switch keyboard focus between editor and file tree |
| Arrows / Home / End / Backspace / Delete / Enter | Cursor movement and editing (in the tree: Up/Down move the selection, Enter opens the file) |
| Esc | Close the palette |
| Ctrl+Q | Save every dirty document, then quit (if a save fails it stays open; a second Ctrl+Q quits anyway). Closing the window also saves first. |

## What was verified on this Linux VM

VM: Ubuntu 24.04.5 LTS, Rust 1.100.0 (`~/.cargo/bin`), 4 CPUs,
~8 GB RAM, no GPU, no display. All commands run from
`~/workspace/mica-poc` unless noted.

### Headless tests — PASS

```sh
cargo test -p mica-gpui-poc
# test result: ok. 7 passed; 0 failed
```

The 7 tests cover: Japanese text surviving the buffer +
decoration path, UTF-16 offset round-trips with Japanese and an
emoji (surrogate pair), input-handler replace at the cursor, bold /
italic segments carrying their styles, auto-save writing after the
debounce, keymap defaults, palette filtering.

```sh
cargo test -p mica-shared-core
# test result: ok. 25 passed; 0 failed (plus 1 doc-test)
```

### Headless smoke test — PASS

```sh
./target/debug/mica-gpui --smoke-test ~/workspace/mica-poc/sample-notes
# [PASS] open folder and scan tree — 2 entries
# [PASS] open file in a tab
# [PASS] one file = one tab — 1 tab(s)
# [PASS] Japanese insert does not corrupt the buffer
# [PASS] heading marker hidden off the cursor line
# [PASS] auto-save fires after the debounce
# [PASS] re-read from disk matches the edited buffer
# [PASS] Quick Open finds welcome.md
# [PASS] command palette finds tab_close
# SMOKE TEST: PASS
```

`sample-notes/welcome.md` was byte-identical afterwards (md5
unchanged) — the smoke test restores the file it edits.

### GUI under Xvfb — WORKS (after the feature fix)

```sh
timeout 60 xvfb-run -a -s "-screen 0 1280x900x24" ./target/debug/mica-gpui ~/workspace/mica-poc/sample-notes
```

- `xwininfo -root -tree` shows the Mica window (`1200x800+42+50`,
  the bounds requested at startup); the process stays alive and
  the window is painted (screenshots inspected).
- Screenshot checks: tab bar, file tree with selection, editor with
  centred max-width body; cursor line shows its `# ` markers, other
  lines hide them; headings render larger and bold; bold/italic
  spans render; code spans/fences render monospace; quote markers
  hidden; horizontal rule rendered. Status bar shows folder, file,
  save state, cursor position, focus area.
- Interactive check with `xdotool` under Xvfb: click places the
  cursor, typing inserts text, auto-save writes it to disk
  (verified by reading `second-note.md`: `Another note for tab
  switching tests. XYZ`), Ctrl+P opens Quick Open, typing `welc`
  filters to `welcome.md` first, Enter opens it in a second tab.
- Cold start (debug build, warm page cache, software rendering):
  window appears **~0.70 s** after launch.
- The app prints two harmless warnings under Xvfb (no GPU):
  `libEGL warning: DRI3 error: Could not get DRI3 device` /
  `Ensure your X server supports DRI3 to get accelerated rendering`.
  Rendering falls back to software (lavapipe) and works.

### Failures found and fixed during the build

1. **Silent headless start** (above): missing `wayland`/`x11`
   features on `gpui-pre-platform`. No error, no window.
   Fixed by enabling the features.
2. **Palette typed text dropped**: the palette first used its own
   focus handle, but the input handler is registered for the
   editor's focus handle, so keystrokes went nowhere. Fixed by
   keeping editor focus while the palette is open and routing input
   by the `palette_open` flag.
3. **Bold/italic did not render with `.SystemUIFont`**: runs using
   the virtual `.SystemUIFont` family rendered all weights/styles
   identically on this Linux VM. Using the explicit family
   `Noto Sans` (the fontconfig `sans-serif` match here) renders
   bold/italic correctly. The code uses `Noto Sans` with a comment;
   this is a snapshot quirk, not a spec decision — a real product
   should resolve the body font properly instead of hard-coding.
4. **Fenced code-block content rendered blank**: shared-core's
   `parse_line` returned `spans: []` for code-block content lines,
   so `decorate_line` produced an empty display line. Fixed in
   shared-core: interior lines now carry one code span, so they
   decorate as one monospace segment. The view's fallback (render
   the raw line as one monospace segment when the display line is
   empty but the buffer line is not) remains only for lines whose
   whole content is a hidden marker, such as a bare `# `.

## System dependencies

No new build dependencies had to be installed: the VM already had
`libvulkan1`, `libwayland-dev`, `libxkbcommon-dev`,
`libfontconfig-dev`, `libxcb1-dev`, `libxcb-render0-dev`,
`libxcb-shm0-dev`, `pkg-config`, `cmake`, and CJK fonts
(`Noto Serif CJK JP`, `Noto Sans CJK`). Installed during
verification only (not needed to build or run the PoC):
`x11-utils` (for `xwininfo`) and `xdotool` (for the scripted
typing test) via apt.

Linux GPU notes: GPUI on Linux renders through **wgpu/Vulkan**
(the `gpui-pre-wgpu` renderer in this snapshot). It needs a Vulkan
loader (`libvulkan.so.1`) and an ICD; on a headless VM the
`lvp_icd.json` (lavapipe, software Vulkan) driver is used, which is
why the GUI works under Xvfb at all. On real hardware, a GPU Vulkan
driver is required; there is no OpenGL fallback in this snapshot.
The `~/.config/mica/keymap.toml` file did not exist on this VM, so
the built-in defaults were used (confirmed in the `strace`: the
file open returns `ENOENT` and the app proceeds).

## Timings (measured on this VM, `-j2` because RAM is tight)

| Measurement | Result |
|---|---|
| First `cargo check` (cold cache, deps + `gpui-pre`) | 11 min 01 s (also competing with another agent's build on the same VM) |
| First `cargo test` profile build | 4 min 26 s |
| Rebuild after enabling `wayland`/`x11` features | 4 min 51 s |
| Incremental rebuild (one file changed) | 15–48 s |
| `cargo test -p mica-gpui-poc` (tests only) | ~1.1 s test time |
| Debug binary size | 347 MB (`target/debug/mica-gpui`) |
| Cold start to window (debug, Xvfb, lavapipe) | ~0.70 s |

No release build was measured (a full release build of the wgpu
stack was out of the timebox; expect it to be substantially slower
than the debug builds above).

## Limits (honest)

- **Windows not tested.** In particular, Japanese IME was verified
  headlessly for buffer/decoration integrity (Japanese text
  inserted through the input-handler path does not corrupt the
  buffer, decorations, or UTF-16 offsets), and ASCII typing was
  verified in the GUI under Xvfb — but **live IME composition was
  not tested**: no IBus/Fcitx input was driven under Xvfb, and the
  Windows IME candidate-window position is GPUI's known open issue
  **#56149** (candidate window position on Windows). Do not claim
  Windows IME works. The input handler implements the full
  `EntityInputHandler` surface (marked text, `bounds_for_range`
  for the candidate window, selection), so the plumbing exists; it
  is unverified on real IME.
- **No text selection.** The cursor is a caret only; there is no
  selection rendering, no mouse-drag selection, and no clipboard
  paste wiring beyond the default `paste` implementation. The
  input handler reports an empty selection at the caret.
- **No virtualization.** Every line is re-shaped on every render.
  Fine for notes; a 5,000-line document would be slow. A real
  editor needs a shaped-line cache invalidated per edited
  paragraph plus exact-height virtualization (the design Zed's
  DisplayMap embodies; not copied).
- **No undo/redo, no search-in-folder, no file create/rename/
  delete UI.** The palette lists `search_in_folder`, `new_file`,
  `rename` as commands, but they are not wired (they no-op).
- **Per-area keymap tables are not wired.** Only the `[global]`
  table of `~/.config/mica/keymap.toml` becomes GPUI bindings;
  the `[file_tree]` `j`/`k` bindings from Notion §7 are not active
  (binding single letters globally would swallow typed text). Tree
  navigation is mouse click, F6 + arrows + Enter.
- **Scrolling** is a plain scrollable container with no scroll
  position sync, no minimap, no scroll-to-cursor.
- **Proportional font is hard-coded to `Noto Sans`** (see the
  `.SystemUIFont` quirk above) and code to `monospace`; there is
  no settings file support yet (`settings.toml` from Notion §7 is
  not read — font size 16, line height 1.8, max width 720 are
  constants matching the spec defaults).
- **Single window, no WSL bridge use.** The PoC reads the folder
  directly from the local filesystem; the `wsl_bridge` module is
  not exercised here (its own tests cover the protocol).
