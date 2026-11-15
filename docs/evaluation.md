# Mica PoC — Evaluation and decision

Status: **published 2026-11-15** — this evaluation is in the repo as its
own PR; PR #10 (Tauri) is merged, so the Tauri PoC files it links to are
on `main`. Scores are measured, not research-only:
the preliminary (research-only) scores are in
[`research-2026-11.md`](research-2026-11.md) §10; the table below
replaces them with PoC evidence.

## Measured comparison

Score 1–5 per axis (rubric in `research-2026-11.md` §10). Every score
links to its evidence (command, file, CI run). Axis 1 (smoothness) was
**not measured** — no 5,000-line scroll / fps test was run in any PoC,
so no score is given for it; the totals below are out of 35 (7 axes),
not 40. Do not compare these totals with the preliminary /40 totals.

| Axis | Tauri + CodeMirror 6 | GPUI | egui |
| --- | --- | --- | --- |
| 1 Smoothness | _not measured_ | _not measured_ | _not measured_ |
| 2 Live-preview ease | **4** | **2** | **3** |
| 3 Japanese IME | **3** | **2** | **2** |
| 4 Proportional font / layout | **5** | **4** | **3** |
| 5 Visual polish | **4** | **3** | **3** |
| 6 Build / startup | **3** | **3** | **4** |
| 7 Maturity / docs | **5** | **2** | **4** |
| 8 License risk | **5** | **4** | **5** |
| **Total /35 (axis 1 excluded)** | **29** | **20** | **24** |

### Why each score

**Axis 2 — Live-preview ease.**
Tauri 4: the Lezer syntax tree + decoration layer (`poc/tauri-codemirror/src/live-preview.ts`,
~500 lines) gives the most complete preview — headings, bold/italic/strikethrough,
inline + fenced code, links, GFM checkboxes (click to toggle), quotes, rules,
setext headings. Not a 5: it needed two real fixes found in review —
explicit GFM base (CommonMark default parses `- [x]` as a link, not a task)
and the cursor-line rule for constructs spanning lines
(`touchesCursorLine`). Both are documented in the Tauri README.
GPUI 2: everything is hand-built on shared-core decorations; the PoC renders
styled lines and a raw cursor line, but there is no selection, no
virtualization, no undo/redo — a real editor on GPUI is a multi-year-scale
subsystem, and the PoC only proves the first step. egui 3: the per-line
widget works (styled paragraphs off the cursor line, raw editor on it), but
with visible compromises — no multi-line selection/paste, no virtualization,
bold renders as strong colour (no bold face in the bundled fonts), checkboxes
are plain text.

**Axis 3 — Japanese IME.** No live IME session was run for any PoC (no
IBus/Fcitx/MS IME in the VM); candidate-window position is untested
everywhere. Tauri 3: headless round-trip 10/10 (`npm run test:ime`) and the
composition-safety argument is structural plus DOM-tested (26 DOM checks) —
but live input is unconfirmed, so it cannot score higher. GPUI 2: the review
caught a real preedit bug (every intermediate preedit string stayed in the
buffer: か→かん→漢 produced かかん漢); fixed at model level with a
preedit→preedit→commit test, still only model-level, and the IME path is a
DIY `EntityInputHandler`. egui 2: composition simulation only (smoke test
8/8); upstream CJK issues are open (#7908 Backspace double-consumed during
composition, #7485 IBus/Wayland).

**Axis 4 — Proportional font / layout.** Tauri 5: proportional font, centred
720 px body, heading sizes, monospace code spans — CSS does the work, nothing
fought back. GPUI 4: proportional font and centred layout work, but the font
is hard-coded to Noto Sans and there is no font configuration. egui 3: works,
but bold-as-colour, a centred-text bug found and fixed in review, and no
virtualization.

**Axis 5 — Visual polish.** Tauri 4: closest to an Obsidian-class look
(screenshot: large headings, styled inline text, checkboxes, quote border,
HR); no dark theme yet. GPUI 3: clean but plain — no dark theme, no
selection rendering, hard-coded font. egui 3: functional dev-tool look;
bold-as-colour and plain-text checkboxes keep it from Obsidian-class.

**Axis 6 — Build / startup.** Tauri 3: frontend build ~2.5 s; `src-tauri`
first compile 8m47s, incremental seconds; release bundle build 9m50s; debug
binary 197 MB, release 11.8 MB; system deps needed webkit2gtk (with a
`libzstd1` downgrade and the libappindicator/ayatana conflict on this VM);
cold start never measured. GPUI 3: cold start ~0.70 s (debug, lavapipe) is
the best of the three, debug binary 347 MB — but the build fought back hard:
`gpui-pre` needs `wayland` + `x11` features or the app silently starts
headless (~40 min debugging with `strace`). egui 4: first build 4m36s, cold
start ~2.1 s (slowest), debug binary 432 MB (biggest); otherwise the
smoothest build (one missing system library, fixed).

**Axis 7 — Maturity / docs.** Tauri 5: built from docs alone; one gotcha
(`tauri-build` does not track the `tauri` version) documented in the README.
GPUI 2: official crates.io 0.2.2 is ~13 months stale; current code comes via
the unofficial `gpui-pre` 0.3.12 fork; docs are thin; the PoC needed
source-diving (platform IME code, `examples/input.rs`) and issue archaeology.
egui 4: current 0.37.0, good docs, known issues documented upstream.

**Axis 8 — License risk.** Tauri 5 and egui 5: permissive throughout
(MIT OR Apache-2.0; CodeMirror MIT), clean dependency closures. GPUI 4: the
framework itself is Apache-2.0 and the `Cargo.lock` was checked (no GPL Zed
editor crates; only `zed-font-kit`/`zed-scap`/`zed-xim`, MIT or Apache) — but
the adjacent GPL editor crates are a live foot-gun: one wrong dependency
makes Mica GPL. The risk is procedural, not legal, if the boundary in
`poc/gpui/README.md` is respected.

## Per-PoC evidence

### Tauri + CodeMirror 6

- Versions pinned: tauri =2.12.5, tauri-build =2.7.4, `@codemirror/view`
  6.43.15, `@codemirror/state` 6.7.6, `@codemirror/lang-markdown` 6.5.2,
  `@tauri-apps/api`/`cli` 2.12.4. See `poc/tauri-codemirror/README.md`.
- Linux VM: `npm run build` pass (2.5 s, 518 kB JS); `npm run test:ime`
  10/10; `npm run test:dom` 26/26; `cargo test` in `src-tauri` 7/7;
  `cargo fmt --check` clean; debug binary launches under Xvfb and stays
  alive; release bundle built (`.deb` 3.16 MiB, `.rpm` 3.17 MiB,
  `.AppImage` 78.16 MiB; release binary 11.8 MB); screenshot visually
  confirmed. PR #10 (merged `fbc8c0a`).
- GitHub Actions: workflows are in the repo (`.github/workflows/poc-linux.yml`,
  `poc-windows.yml`; PR #11, merged `4114922`) and run on push/PR to `main`;
  first CI run result on `main` not yet recorded here.
- IME test sheet: headless round-trip only (hiragana composition,
  conversion + confirmation, Backspace mid-composition, multi-byte
  boundaries). Live IME untested; candidate window untested.
- What hurt most: the two review-caught bugs (multi-line cursor-line rule,
  auto-save race) and the icon/build coupling (`generate_context!()` fails
  without `src-tauri/icons/`). WebKitGTK system-dependency friction on
  Linux; cold start never measured.

### GPUI

- Dependency choice: `gpui-pre` 0.3.12 + `gpui-pre-platform` 0.3.12 (the
  unofficial fork tracking Zed; official crates.io 0.2.2 is stale).
  `wayland` + `x11` features are load-bearing.
- Linux VM: `cargo test -p mica-gpui-poc` 19 passed; smoke test 9/9;
  Xvfb GUI launch verified; `cargo fmt` clean; cold start ~0.70 s
  (debug, lavapipe); debug binary 347 MB. PR #9 (merged `2a08368d`).
- GitHub Actions: workflows in the repo via PR #11 (see Tauri above); first
  CI run result on `main` not yet recorded here.
- IME test sheet: model-level preedit→preedit→commit test only
  (か → かん → commit 漢). Live IME untested; Windows candidate-window
  issue (#56149) known.
- What hurt most: the silent headless start without the platform features
  (~40 min); the IME preedit bug; every editor behaviour hand-built
  (no selection, no virtualization, no undo/redo); per-area keymaps not
  wired (global only); font hard-coded.

### egui

- Versions pinned: eframe =0.37.0, egui =0.37.0. See `poc/egui/README.md`.
- Linux VM: `cargo test -p mica-egui-poc` 20 passed; smoke test 8/8
  (incl. Japanese IME composition simulation); Xvfb GUI launch + screenshot
  visually confirmed; cold start ~2.1 s; first build 4m36s; debug binary
  432 MB. PR #9 (merged `2a08368d`).
- GitHub Actions: workflows in the repo via PR #11 (see Tauri above); first
  CI run result on `main` not yet recorded here.
- IME test sheet: composition simulation only. Live IME untested;
  upstream CJK issues #7908, #7485 open.
- What hurt most: the per-line widget's limits (no multi-line
  selection/paste, no virtualization); bold-as-colour; the review-caught
  tab-close data loss and Backspace empty-line bugs (both fixed).

## Recommendation

**Tauri + CodeMirror 6 is the most promising option on the measured
evidence.** It wins the axes that are about being an editor — live-preview
ease, proportional font/layout, visual polish, maturity/docs, license —
by a clear margin (29/35 vs egui 24, GPUI 20), and its PoC is the most
complete end-to-end editor of the three (the only one with clickable
task checkboxes, setext headings, and a real text-editing engine underneath).
Its weaknesses are known and bounded: build/startup friction on Linux,
cold start and 5,000-line smoothness unmeasured, and live IME untested on
every platform.

GPUI's raw-performance promise did not survive contact with the editor
work: it is the fastest to start, but every editor behaviour is hand-built,
the docs are thin, the current crates come from an unofficial fork, and the
IME path needed a correctness fix found only in review. It remains the
interesting long-term bet if raw smoothness turns out to matter more than
the PoC could measure — but it is not the choice on today's evidence.

egui is the credible pure-Rust second: the simplest build, the most honest
limitations, and no webview. Pick it if the project decides against a
webview (or against npm) — but expect to hand-build live-preview richness,
selection, and virtualization that Tauri gets from CodeMirror.

### What would change the decision

1. **Windows live IME results.** If Tauri/WebView2 IME proves poor and
   GPUI's DIY handler proves solid on Windows (or vice versa), the IME
   axis moves and could close the gap.
2. **A 5,000-line smoothness measurement.** Axis 1 is unscored. If Tauri
   janks where GPUI holds 120 fps, that matters for an editor whose spec
   demands refresh-rate-following scrolling; if all three hold up, the
   gap stays.
3. **A no-webview decision.** If the project rules out a webview (or npm),
   egui wins by elimination.
4. **GPUI upstreaming.** If GPUI gets an official current crates.io release
   with real docs, its maturity score rises — but that fixes only one axis.

## What still needs a real Windows machine

- Visual check of all three PoCs (layout, fonts, HiDPI, dark theme).
- Japanese IME on Windows (MS IME / Google Japanese Input):
  type kana, convert with Space, confirm with Enter, Backspace
  mid-composition, candidate-window position.
- `mica .` CLI launch from WSL into the Windows GUI.
- Startup-time feel on real hardware.
- 5,000-line scroll / typing smoothness (axis 1) for all three PoCs.

## License inventory

- Tauri PoC: Rust deps tauri/tauri-build/serde/serde_json/toml/tempfile
  (MIT OR Apache-2.0) + path dependency on shared-core; npm deps
  `@codemirror/*`, `@lezer/markdown`, `@tauri-apps/*`, vite, typescript,
  jsdom (MIT). No GPL anywhere; no Zed code.
- GPUI PoC: `gpui-pre` 0.3.12 + `gpui-pre-platform` (Apache-2.0). 726 of
  757 `Cargo.lock` packages checked from the registry: all `gpui-pre-*`
  Apache-2.0; only `zed-*` crates are `zed-font-kit`, `zed-scap`,
  `zed-xim` (MIT or Apache). No `editor`/`text`/`rope`/`language`/
  `multi_buffer`/`workspace`/`ui`/`ui_input`. GPL appears only as one
  choice alongside permissive licenses (`self_cell`, `r-efi`).
- egui PoC: eframe/egui 0.37.0 (MIT OR Apache-2.0). Clean.
- Mica's own code (`poc/shared-core`, all three PoCs): MIT OR Apache-2.0
  (`LICENSE-MIT`, `LICENSE-APACHE-2.0` — the Apache text is byte-identical
  to the canonical apache.org text except the copyright line).
- CI / workflow files: no license implications.
- **No GPL code anywhere in the repo.**
