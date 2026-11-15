# Mica PoC

Proof-of-concept builds for **Mica**, a custom Markdown editor:
Obsidian's pleasant writing experience, but WSL-first,
keyboard-centric, and with no vault.

> **Status:** Shared core, egui PoC and GPUI PoC are in this repo;
> the Tauri PoC and CI workflows land in a follow-up PR.

Spec: Notion 「Mica 仕様メモ（自作マークダウンエディタ）」 —
see [`docs/notion-spec-summary.md`](docs/notion-spec-summary.md).
Framework research as of 2026-11-15:
[`docs/research-2026-11.md`](docs/research-2026-11.md).
Plan: [`docs/plan.md`](docs/plan.md).

Progress is tracked in GitHub Issues
([#1](https://github.com/taka-kawah/mica-poc/issues/1) meta,
[#2](https://github.com/taka-kawah/mica-poc/issues/2) Tauri,
[#3](https://github.com/taka-kawah/mica-poc/issues/3) GPUI,
[#4](https://github.com/taka-kawah/mica-poc/issues/4) egui,
[#5](https://github.com/taka-kawah/mica-poc/issues/5) shared core,
[#6](https://github.com/taka-kawah/mica-poc/issues/6) CI,
[#7](https://github.com/taka-kawah/mica-poc/issues/7) evaluation).
There is no GitHub Project or Wiki yet — creating them is a manual
step in the GitHub web UI; see
[`docs/github-scaffolding-status.md`](docs/github-scaffolding-status.md).

## What is in this repo

| Path | What it is |
| --- | --- |
| `poc/shared-core/` | Framework-independent Rust core: text buffer, Markdown structure, live-preview decorations, file tree, tabs, auto-save, keymap, command palette, WSL stdio bridge stub. Every PoC builds on it. Tests: `cargo test -p mica-shared-core` (no display needed). |
| `poc/tauri-codemirror/` | Tauri 2 + CodeMirror 6 PoC. |
| `poc/gpui/` | GPUI PoC. |
| `poc/egui/` | egui + eframe PoC. |
| `sample-notes/` | Shared sample Markdown folder used by every PoC's tests. |
| `docs/` | Plan, spec summary, framework research, evaluation. |
| `.github/workflows/` | Linux + Windows CI for all PoCs. |

## The three PoCs

Each PoC is a minimal end-to-end editor in its own directory:
open a folder, file tree, tabs (one file = one tab), live-preview
editing (markers hidden off the cursor line, shown on it),
Japanese IME input, proportional font with a centred 720 px body,
auto-save, command palette (Ctrl+Shift+P) and Quick Open (Ctrl+P).
They are decision-making prototypes, not the MVP — see
[`docs/plan.md`](docs/plan.md) for the acceptance criteria.

Run them (Linux):

```sh
# Shared core tests (headless)
cargo test -p mica-shared-core

# egui PoC
cargo run -p mica-egui-poc -- <folder>
cargo run -p mica-egui-poc -- --smoke-test sample-notes

# GPUI PoC
cargo run -p mica-gpui-poc -- <folder>
cargo run -p mica-gpui-poc -- --smoke-test sample-notes

# Tauri PoC (from poc/tauri-codemirror)
npm install
npm run tauri dev
```

Each PoC's own README records exactly what was verified on the
Linux VM, its limits, and its pinned versions.

## Comparison (preliminary — from research, to be replaced by PoC measurements)

From [`docs/research-2026-11.md`](docs/research-2026-11.md) §10.
Scores are 1–5 per axis; the table below is the *preliminary*
research-only view. The measured table lives in
[`docs/evaluation.md`](docs/evaluation.md) once the PoCs have run.

| Axis | Tauri + CodeMirror 6 | GPUI | egui |
| --- | --- | --- | --- |
| Smoothness | 4 | 5 | 4 |
| Live-preview ease | 5 | 1 | 2 |
| Japanese IME | 4* | 2 | 2 |
| Proportional font / layout | 5 | 4 | 3 |
| Visual polish | 5 | 4 | 3 |
| Build / startup | 3 | 3 | 5 |
| Maturity / docs | 5 | 2 | 4 |
| License risk | 5 | 4† | 5 |
| **Total /40 (preliminary)** | **36** | **25** | **28** |

\* Tauri IME is 4, not 5, only because the Linux/WebKitGTK IME path
was untested in research. † GPUI is 4, not 5, because the framework
is Apache-2.0 but the adjacent GPL Zed editor crates are a live
foot-gun: one wrong dependency makes Mica GPL.

**Preliminary ranking: 1. Tauri + CodeMirror 6 · 2. egui · 3. GPUI.**

The Notion spec currently rates GPUI most promising; this research-only
ranking disagrees, and the PoC measurements will decide.

## Licenses

- Mica PoC code is dual-licensed MIT OR Apache-2.0; the full license
  texts are in the repo root as `LICENSE-MIT` and `LICENSE-APACHE-2.0`.
- Tauri: MIT OR Apache-2.0. CodeMirror 6: MIT. egui/eframe: MIT OR Apache-2.0.
- GPUI: Apache-2.0.
- **No GPL code is used anywhere in this repo.** Zed's editor crates
  are GPL-3.0-or-later and are not copied, vendored, or depended on.
  GPUI itself is a different, Apache-2.0 crate set in the same
  monorepo; the boundary is documented in
  [`docs/research-2026-11.md`](docs/research-2026-11.md) §8 and in
  `poc/gpui/README.md`.

## Known limits (honest list)

- The PoCs are verified on a Linux VM (Ubuntu 24.04, no display;
  GUI runs headless under Xvfb). **Windows visual and IME checks
  have not been done** — they need a real Windows machine.
- The WSL bridge in the PoCs is a protocol implementation plus an
  in-process local-filesystem backend. The real Windows transport
  (spawning the server with `wsl.exe`) is documented, not faked.
- GPUI's official crates.io release (0.2.2, Oct 2025) is ~13 months
  stale; the PoC pins its exact source — see `poc/gpui/README.md`.
