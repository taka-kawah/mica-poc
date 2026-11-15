# Mica PoC — Tauri 2 + CodeMirror 6

A Markdown editor prototype: Tauri 2 (Rust backend) + plain TypeScript
frontend + CodeMirror 6, with an Obsidian-style live preview. One of
three Mica PoCs (see `../../docs/plan.md` and
`../../docs/research-2026-11.md`); the other two are `../gpui` and
`../egui`.

The buffer always holds raw Markdown. Live preview only changes how
lines are *shown*: markers (`##`, `**`, `` ` ``, `>`, link URLs, …)
are hidden on every line except the cursor line, where they reappear.
Headings render larger, bold/italic/code/links are styled inline,
task lists render as clickable checkboxes, and horizontal rules render
as rules. Saving writes the raw Markdown back to disk, atomically.

![Mica running under Xvfb on Linux: welcome.md open, live preview
rendered](screenshot-xvfb.png)

## Layout

```
poc/tauri-codemirror/
  package.json                  # pinned npm versions (exact, no ^ ranges)
  vite.config.ts                # Vite dev server on port 1420
  index.html                    # app shell: top bar, tree, tabs, editor, palette
  src/
    main.ts                     # app shell: tree, tabs, palette, autosave, keymap
    live-preview.ts             # CodeMirror 6 decoration layer (~500 lines)
    style.css                   # proportional font, centred 720px body
  scripts/
    composition-roundtrip.mjs   # headless Japanese IME round-trip check
    live-preview-dom-check.mjs  # headless DOM check of the decoration layer
  src-tauri/
    Cargo.toml                  # tauri 2.12.5, tauri-build 2.7.4 (see note below)
    tauri.conf.json             # window 1280x800, frontendDist ../dist
    icons/                      # generated with `npx tauri icon`
    src/
      main.rs                   # thin entry point
      lib.rs                    # Tauri commands + headless smoke tests
  screenshot-xvfb.png           # the app running under Xvfb on this VM
```

This directory is **not** a member of the root Cargo workspace:
Tauri manages its own Rust project under `src-tauri/` (it has its own
empty `[workspace]` table so Cargo does not attach it to the root
workspace). It depends on `mica-shared-core` by path
(`../../shared-core`).

## What the backend does (and does not do)

Every Tauri command in `src-tauri/src/lib.rs` delegates to
`mica-shared-core` instead of reimplementing logic:

| Command | Shared-core piece used |
| --- | --- |
| `open_folder`, `list_files` | `file_tree::scan_folder` |
| `read_file`, `save_file` | `auto_save::save_atomically` (temp file + rename) |
| `search_files` | `wsl_bridge::LocalBridgeBackend` (the stdio JSON-RPC protocol, in-process on Linux) |
| `search_palette`, `list_commands` | `command_palette::{filter_entries, PaletteEntry}`, `keymap::KNOWN_COMMANDS` |
| `load_keymap` | `keymap` (`~/.config/mica/keymap.toml`, built-in defaults fallback) |
| `open_tab`, `close_tab`, `list_tabs` | `tabs::TabManager` (one file = one tab; reopening focuses) |
| `default_folder` | `MICA_FOLDER` env var, then the repo's `sample-notes`, then `$HOME` |

The frontend never touches the filesystem directly; it only calls
these commands. Tabs, auto-save state, the keymap and the fuzzy
matcher all live in shared-core, so they behave identically in the
GPUI and egui PoCs.

## Live preview — what it covers

`src/live-preview.ts` is a CodeMirror 6 `StateField` that walks the
Lezer Markdown syntax tree on every document/selection change and
computes decorations (`Decoration.mark` / `.line` / `.replace` +
widgets). Cursor-aware reveal is row-level: the line containing the
selection head shows its raw markers; all other lines hide theirs.
Replaced widgets (checkboxes, rules) are `atomicRanges`, so the
cursor skips over them instead of stopping on invisible characters.

Covered: ATX headings `#`–`######` and setext headings, bold
`**…**`, italic `*…*`, strikethrough `~~…~~`, inline code, fenced
code blocks (fence + language hidden, code shown monospace),
links `[text](url)` (URL hidden, text styled), task checkboxes
(`- [ ]` / `- [x]`, click to toggle), block quotes, horizontal
rules.

**Not covered (out of PoC scope):** tables, math, images (a link to
an image is styled as a link, the image itself is not rendered
inline), footnotes, nested emphasis, wiki links `[[…]]`, and
multi-line constructs beyond fenced code. The heading marker hides
the `#`s and the spaces after them; a setext underline is hidden on
non-cursor lines.

Two things this PoC found while building the layer (both verified,
not assumed):

1. **`markdown()` defaults to strict CommonMark.** Calling it with
   no arguments parses `- [x]` as a shortcut link, not a task, so
   no checkbox widget is produced. The layer passes the GFM
   language explicitly: `markdown({ base: markdownLanguage })`.
   `scripts/live-preview-dom-check.mjs` guards this.
2. **Composition safety is structural.** CodeMirror owns the
   composing text (EditContext / contenteditable), and composition
   always happens at the cursor — exactly the line where this layer
   applies *no* hiding or replacement. No decoration ever overlaps
   the composition range, so recomputing decorations cannot flicker
   or corrupt composing text.

## Japanese IME

IME handling is inherited from the webview + CodeMirror's
composition handling — there is no custom IME code in this PoC.
That is the mechanism on Windows (WebView2/Chromium) and on Linux
(WebKitGTK).

Verified headlessly on this VM (`npm run test:ime`, 10 checks):
Japanese text typed through `EditorState` transactions round-trips
without corruption — hiragana composition, conversion + confirmation
replacing the composing range, Backspace mid-composition deleting
exactly one character, and edits at multi-byte boundaries (no U+FFFD,
character — not byte — offsets).

**Not verified: real IME input.** No live Japanese input session was
run here (no IBus/Fcitx or MS IME in this VM), and the candidate
window position was not checked. **Do not claim Windows IME works —
untested.** Manual test item for Windows: type kana, convert with
Space, confirm with Enter, Backspace mid-composition, and check the
candidate window sits at the caret. The same session should be run
on Linux with IBus/Fcitx5 (WebKitGTK's IME path is the least
documented of the three engines — see research §5.1).

## How to run

Prerequisites (Linux): Rust ≥ 1.90, Node.js ≥ 20, and the system
libraries listed below. On Windows: MSVC C++ Build Tools + the
WebView2 Runtime (preinstalled on Windows 10 1803+ / 11).

```sh
cd poc/tauri-codemirror
npm install

npm run dev            # Vite dev server only (http://localhost:1420)
npm run tauri dev      # full app: Vite + Tauri window (recommended)
npm run build          # tsc --noEmit + vite build -> dist/
npm run tauri build    # release bundle (runs npm run build first)
```

Headless checks (no display needed):

```sh
npm run test:ime       # Japanese composition round-trip (10 checks)
npm run test:dom       # live-preview DOM check in jsdom (9 checks)
cd src-tauri && cargo test        # backend commands vs sample-notes (6 tests)
cd src-tauri && cargo fmt --check # Rust formatting
```

The app opens `sample-notes/` by default. Set `MICA_FOLDER` to open
a different folder on startup, or type a path in the top bar.
Keyboard: `Ctrl+P` Quick Open, `Ctrl+Shift+P` command palette,
`Ctrl+Shift+F` search in folder, `Ctrl+B` toggle tree, `Ctrl+Tab`
next tab, `Ctrl+W` close tab, `Ctrl+S` save now. The tree uses
`j`/`k`/`l`/`h` when focused. Bindings come from
`~/.config/mica/keymap.toml` when it exists (shown in the status
bar), otherwise the built-in defaults (Notion §7).

## Versions pinned (verified 2026-11-15)

| Component | Pinned version | Note |
| --- | --- | --- |
| Rust toolchain (VM) | 1.100.0 | MSRV for Tauri 2.12 is 1.90 |
| Node.js / npm (VM) | 24.22.1 / 10.9.4 | |
| `tauri` (Rust) | = 2.12.5 | research pin; exists on crates.io |
| `tauri-build` (Rust) | = 2.7.4 | **does not track the tauri version** — tauri 2.12.5 depends on tauri-build ^2.7.4; there is no tauri-build 2.12.x |
| `@tauri-apps/cli` | 2.12.4 | latest 2.x on npm on 2026-11-15 (no 2.12.5 published); pairs with core 2.12.5 |
| `@tauri-apps/api` | 2.12.4 | |
| `@codemirror/view` | 6.43.15 | research pin |
| `@codemirror/state` | 6.7.6 | research pin |
| `@codemirror/lang-markdown` | 6.5.2 | latest on 2026-11-15 |
| `@codemirror/language` | 6.13.1 | |
| `@codemirror/commands` | 6.11.1 | |
| `@lezer/markdown` | 1.8.1 | |
| `vite` | 7.3.8 | |
| `typescript` | 5.9.3 | |
| `jsdom` (dev only) | 31.0.1 | for `test:dom` |

## System dependencies (Linux, already installed on this VM)

Needed to link the Tauri app (WebKitGTK). Recorded from the VM on
2026-11-15 (`pkg-config` / `dpkg -l`):

| Package | Version |
| --- | --- |
| `libwebkit2gtk-4.1-dev` | 2.52.6 |
| `libgtk-3-dev` (`gtk+-3.0`) | 3.24.41 |
| `libayatana-appindicator3-dev` | 0.5.93 |
| `librsvg2-dev` | 2.58.0 |
| `libssl-dev` | 3.0.13 |

(Tauri 2 targets webkit2gtk **4.1**; a 4.0 reference means a stale v1
guide. `libxdo-dev` and `patchelf` from the Tauri prerequisite list
were not needed for `cargo build` on this VM.)

## What was verified on this Linux VM (2026-11-15)

All commands were run in `poc/tauri-codemirror/` unless noted.
Nothing below is claimed without a command behind it.

| Check | Command | Result |
| --- | --- | --- |
| Frontend typecheck + build | `npm run build` | ✅ pass (tsc + vite, 7–10 s; JS bundle 518 kB, 180 kB gzip; chunk-size warning only — single bundle, fine for a PoC) |
| Japanese IME round-trip | `npm run test:ime` | ✅ 10/10 checks pass |
| Live-preview DOM | `npm run test:dom` | ✅ 9/9 checks pass (checkboxes, HR, heading reveal, cursor-line raw, bold, code) |
| Rust tests | `cd src-tauri && cargo test` | ✅ 6/6 pass (first compile 8m47s; incremental seconds) |
| Rust formatting | `cd src-tauri && cargo fmt --check` | ✅ clean |
| Rust debug build | `cd src-tauri && cargo build` | ✅ pass, 2m36s; `target/debug/mica-tauri` is 197 MB (debug, unoptimized) |
| App launches headless | `timeout 20 xvfb-run -a ./target/debug/mica-tauri` | ✅ stayed running the full 20 s (exit 124 = killed by timeout while alive). Only benign output: `libEGL warning: DRI3 error` (no GPU in the VM; software rendering). Note: a plain `cargo build` binary loads the **dev URL** (`http://localhost:1420`), so without a dev server it shows "Could not connect to localhost" — that is expected Tauri debug behaviour, not a bug; use `tauri dev` or the release bundle. |
| App UI renders + works | `npx tauri dev` under Xvfb (display :97) + scripted click on `welcome.md` | ✅ renders and behaves (see `screenshot-xvfb.png`): folder auto-opened, tree lists both sample notes, tab opens, live preview renders (large heading, bold/italic/code/link, ☐/☑ checkboxes, hidden fences, quote border, HR). **One anomaly, recorded honestly:** an earlier run on the shared display :99 produced one frame showing raw Markdown with no decorations; the rerun on an isolated display (:97 — a sibling agent's Xvfb was also live on :98/:99 at the time) renders correctly, and the jsdom DOM check passes deterministically, so the :99 frame is attributed to display contention / a stale frame, not to the decoration layer. |
| Release bundle | `npx tauri build` | ✅ **succeeded**, 9m50s (release compile). Binary `target/release/mica-tauri` is 11.8 MB. Bundles: `Mica_0.1.0_amd64.deb` 3.16 MiB, `Mica-0.1.0-1.x86_64.rpm` 3.17 MiB, `Mica_0.1.0_amd64.AppImage` 78.16 MiB (AppImage downloads linuxdeploy/AppRun at build time — needs network). |
| Release binary launches | `target/release/mica-tauri` under Xvfb (display :97), no dev server running | ✅ renders the bundled frontend (top bar, tree, editor, status bar; keymap loaded). This confirms the release bundle does not depend on the Vite dev server. Note: launched from `$HOME`, it defaulted to opening `$HOME` as the folder (default-folder resolution is cwd-relative; set `MICA_FOLDER` for the release binary). Cold-start time was **not measured** — the window-title poll in the timing script never matched the window, so no startup number is claimed here. |

### `cargo tauri build` result

`npx tauri build` **succeeded on 2026-11-15** (see the table above):
release compile 9m50s, binary 11.8 MB, `.deb` 3.16 MiB, `.rpm`
3.17 MiB, `.AppImage` 78.16 MiB. The release binary was launched
under Xvfb with no dev server and rendered the bundled UI.

## Limits (honest)

- **Windows is untested here.** No visual check, no IME check, no
  `cargo tauri build` on Windows. The Windows build is checked in
  GitHub Actions (see `../../.github/workflows/`); run it before
  trusting any Windows claim.
- **Linux uses WebKitGTK, not Chromium.** Rendering, fonts and IME
  behaviour can differ from Windows' WebView2. The screenshot is
  WebKitGTK 2.52.6 under Xvfb (software rendering — no GPU).
- **No real IME session was run** (see "Japanese IME" above).
- **No performance measurement yet.** Scrolling a 5,000-line file
  and cold-start timing were not measured in this PoC run; CodeMirror
  virtualizes the viewport, but that is a claim from its docs, not a
  measurement here.
- **Commands not implemented in the frontend:** `new_file` and
  `rename` are known commands that report "not implemented in this
  PoC" in the status bar instead of pretending to work. Tab restore
  across restarts, external-change watching, drag & drop, split view
  and vim mode are out of scope (Notion §2 / plan.md).
- **Debug binary is big** (197 MB with debug info). Release size is
  reported with the `cargo tauri build` result above.

## Licenses

- Tauri 2 (incl. `tauri`, `tauri-build`, `tauri-utils`,
  `tauri-runtime-wry`): **MIT OR Apache-2.0**. The windowing crate
  `tao` is Apache-2.0 only — still permissive.
- CodeMirror 6 (`@codemirror/*`, `@lezer/*`): **MIT**.
- Wry (webview layer): MIT OR Apache-2.0.
- `mica-shared-core`: MIT OR Apache-2.0.
- **No GPL code.** Nothing from Zed's GPL editor crates is used,
  copied, or depended on (see `../../docs/research-2026-11.md` §8).
