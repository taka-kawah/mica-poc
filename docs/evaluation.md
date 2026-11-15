# Mica PoC — Evaluation and decision

Status: **in progress** — this file is filled in after the three PoCs
have been built and run. The preliminary (research-only) scores are
in [`research-2026-11.md`](research-2026-11.md) §10; the table below
replaces them with measured evidence.

## Measured comparison

Score 1–5 per axis (rubric in `research-2026-11.md` §10). Every score
links to its evidence (command, file, CI run).

| Axis | Tauri + CodeMirror 6 | GPUI | egui |
| --- | --- | --- | --- |
| 1 Smoothness | _TBD_ | _TBD_ | _TBD_ |
| 2 Live-preview ease | _TBD_ | _TBD_ | _TBD_ |
| 3 Japanese IME | _TBD_ | _TBD_ | _TBD_ |
| 4 Proportional font / layout | _TBD_ | _TBD_ | _TBD_ |
| 5 Visual polish | _TBD_ | _TBD_ | _TBD_ |
| 6 Build / startup | _TBD_ | _TBD_ | _TBD_ |
| 7 Maturity / docs | _TBD_ | _TBD_ | _TBD_ |
| 8 License risk | _TBD_ | _TBD_ | _TBD_ |
| **Total /40** | _TBD_ | _TBD_ | _TBD_ |

## Per-PoC evidence

### Tauri + CodeMirror 6

- Versions pinned: _TBD_
- Linux VM: build / test / smoke results: _TBD_
- GitHub Actions: _TBD_ (link)
- IME test sheet: _TBD_
- What hurt most: _TBD_

### GPUI

- Dependency choice (gpui-pre / official 0.2.2 / git pin): _TBD_
- Linux VM: build / test / smoke results: _TBD_
- GitHub Actions: _TBD_ (link)
- IME test sheet: _TBD_
- What hurt most: _TBD_

### egui

- Versions pinned: _TBD_
- Linux VM: build / test / smoke results: _TBD_
- GitHub Actions: _TBD_ (link)
- IME test sheet: _TBD_
- What hurt most: _TBD_

## Recommendation

_TBD — one most-promising option, with reasons, and what would change
the decision._

## What still needs a real Windows machine

- Visual check of all three PoCs (layout, fonts, HiDPI, dark theme).
- Japanese IME on Windows (MS IME / Google Japanese Input):
  type kana, convert with Space, confirm with Enter, Backspace
  mid-composition, candidate-window position.
- `mica .` CLI launch from WSL into the Windows GUI.
- Startup-time feel on real hardware.

## License inventory

_TBD — `cargo tree` / `npm ls` licence check per PoC. No GPL code
anywhere; GPUI boundary (Apache-2.0 framework only, no Zed editor
crates) verified in `poc/gpui/README.md`._
