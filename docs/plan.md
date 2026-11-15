# Mica PoC — Plan (2026-11-15)

Coordinator: subagent session for goal_50c23ca0195f. User: 河原.
Repo: `taka-kawah/mica-poc` (public, was empty). Spec: Notion
「Mica 仕様メモ（自作マークダウンエディタ）」 + child 「ライブプレビューの仕組み（事前調査）」.
Full spec extract: `docs/notion-spec-summary.md`.
Framework research: `docs/research-2026-11.md` (pending).

## Goal of the PoC

Not a full MVP. A decision-making prototype: build the same small
end-to-end editor three ways and compare, so the GUI-technology
decision (GPUI vs Tauri+CodeMirror 6 vs egui) can be made on evidence,
not on paper. Notion's own next steps say exactly this:
(1) egui prototype for Japanese IME + proportional-font multiline editing,
(2) same in Tauri+CodeMirror 6 and compare, (3) decide, (4) MVP.

## What every PoC must do (acceptance)

Each PoC lives in its own directory and must, at minimum:

1. Open a folder (any folder, not a vault) and list Markdown files as a tree.
2. Open a file in a tab; one file = one tab; reopening focuses the existing tab.
3. Edit plain Markdown in a live-preview editor:
   - raw text stays raw Markdown in the buffer and on disk;
   - non-cursor lines hide heading/bold markers and render headings larger / bold;
   - the cursor line shows its markers (row-level rule for the PoC — see child page).
4. Accept Japanese IME input without breaking (composition text must not
   corrupt the buffer or flicker decorations).
5. Proportional font for body text, monospace only in code spans/blocks;
   centred body with a max width (720 px target).
6. Auto-save after editing stops (debounced, e.g. 1000 ms) with a visible save state.
7. Command palette (Ctrl+Shift+P) and Quick Open (Ctrl+P), at least for
   file-open / command search; full keyboard operation for tree + tabs.
8. A single global keymap file (TOML) under the user config dir, not per-folder.
9. Headless smoke test: open a folder, open a file, edit, auto-save,
   re-read from disk and verify — runnable in CI without a display.
10. README in its own directory: how to run, what was verified, limits.

Out of scope for the PoC (per Notion §2): vault, graph/Canvas, plugins,
sync/mobile, LSP/debugger/Git UI, split view, vim mode.

## Directory layout (readable names, no cryptic abbreviations)

```
mica-poc/
  README.md                  # project overview + comparison table + how to run each PoC
  docs/
    plan.md                  # this file
    notion-spec-summary.md   # spec extract (from Notion)
    research-2026-11.md      # Tauri / GPUI / egui status as of Nov 2026
    evaluation.md            # comparison table + recommendation (filled at the end)
    github-scaffolding-status.md
  poc/
    shared-core/             # framework-independent Rust library (see below)
    tauri-codemirror/        # Tauri 2 + CodeMirror 6
    gpui/                    # GPUI (Rust, GPU-drawn)
    egui/                    # egui + eframe (Rust, immediate-mode)
  .github/workflows/
    poc-linux.yml            # Linux build+test for all PoCs
    poc-windows.yml          # Windows build check (best effort)
  sample-notes/              # shared sample Markdown folder used by all PoCs' tests
```

Naming rules (user's explicit request): clear English names, no
abbreviation soup, no confusingly similar names. `shared-core` not
`core`; `tauri-codemirror` not `tc`; `gpui` is the framework's own
name and is fine; `egui` is the framework's own name and is fine.

## Shared core (`poc/shared-core`)

Framework-independent Rust library, no GUI dependency, so the
live-preview logic is identical across PoCs and testable headlessly.
This is the part the Notion child page describes as stages 1–4.

Modules (one responsibility each, names in plain English):

- `text_buffer` — line-based text buffer (rope-backed later if needed),
  insert/delete, cursor position (line, column in characters).
- `markdown_parser` — line-level Markdown structure: heading level,
  bold/italic spans, code spans, list items, checkboxes, links.
  Full tree-sitter parsing is a later step; the PoC parser is a
  documented, line-oriented subset (see its README for exact coverage).
- `live_preview` — decoration calculation: for each line, given the
  cursor line, produce display segments (hidden markers, heading size,
  bold) and the buffer↔display coordinate mapping. Row-level rule:
  markers are shown on the cursor line, hidden elsewhere.
- `file_tree` — walk a folder, list `.md` files, ignore `.git`,
  `node_modules`, `.DS_Store`; reflect external changes on re-scan.
- `tabs` — one file = one tab; open/focus/close; dirty tracking.
- `auto_save` — debounced save (temp file + rename = atomic save),
  save state (`clean` / `dirty` / `saving` / `saved`).
- `keymap` — load one global TOML keymap (`~/.config/mica/keymap.toml`
  format from Notion §7); commands are plain strings (`quick_open`,
  `command_palette`, `tab_close`, …).
- `command_palette` — fuzzy-match command/file list for Ctrl+P / Ctrl+Shift+P.
- `wsl_bridge` — stdio JSON-RPC stub: length-framed messages
  (`open_folder`, `list_dir`, `read_file`, `write_file`, `rename`,
  `delete`, `search`, server→client `fs_changed`). On Linux the PoC
  runs the bridge in-process against the local filesystem; the Windows
  path (wsl.exe child process) is documented and stubbed, NOT faked
  as working on Windows.

Each module has unit tests. `cargo test -p mica-shared-core` must pass
on Linux CI with no display.

## Per-PoC notes

### poc/tauri-codemirror
- Tauri 2 (Rust backend) + frontend TypeScript + CodeMirror 6.
- Live preview via CodeMirror 6 decorations (the part Obsidian gets
  for free). Backend commands call `shared-core` for file tree, tabs,
  autosave, keymap, WSL bridge stub.
- Frontend: file tree pane, tab bar, editor pane (proportional font,
  720 px max-width, centred), command palette + Quick Open.
- Linux needs WebKitGTK dev libs (installed in CI via apt).
  Windows build is checked in GitHub Actions (windows-latest).
- License: Tauri MIT/Apache-2.0, CodeMirror MIT. No GPL code.

### poc/gpui
- Rust + `gpui` crate (Zed's framework). GPU-drawn, no WebView.
- Live preview is hand-built on top of `shared-core` decorations:
  styled text runs per line, variable line height, cursor-line rule.
- IME goes through GPUI's input handling — this is the key risk to
  measure (Notion: verify Japanese IME with the GPUI input sample first).
- Must NOT copy or depend on Zed's `editor` crate (GPL). GPUI itself
  is Apache-2.0. Design-only reference to Zed's DisplayMap idea is
  allowed and must be noted as design-only, with no GPL code in the repo.
- If the published `gpui` crate cannot be built on this Linux VM in
  reasonable time, the PoC falls back to: a minimal GPUI app that
  compiles and opens a window under Xvfb + a headless test of the
  decoration/layout code, with the failure and its cause recorded
  honestly in the README and in `docs/evaluation.md`. Do not fake a
  working GUI.

### poc/egui
- Rust + `eframe`/`egui` (immediate mode).
- Live preview: custom rendering of `shared-core` decorations
  (heading sizes, hidden markers on non-cursor lines, bold spans);
  egui's text editing is extended, not replaced, where feasible.
- IME via winit (the platform layer under eframe) — verify Japanese
  input under Xvfb with a scripted test where possible; at minimum
  verify that composition events do not corrupt the buffer in tests.
- Expected to be the fastest to build and the easiest to verify
  headlessly; the question is whether the visual polish / smoothness
  reaches the bar, not whether it runs.

## Verification on this VM (Linux, no Windows, no display)

- `cargo test` for `shared-core` and each Rust PoC (headless).
- GUI PoCs: build with `cargo build`, then run briefly under
  `xvfb-run` and check they start, open the sample folder, and exit
  cleanly (or run their `--smoke-test` CLI mode). Screenshot/eyeball
  verification on Windows is NOT possible here — record that as a
  user action.
- Tauri: `cargo tauri build` is heavy; at minimum `cargo build` for
  the Rust side + `npm run build` for the frontend + a headless
  smoke test. Full `tauri build` attempted in CI (Linux) and recorded.
- Every verification step is recorded in the PoC README and in
  `docs/evaluation.md` with commands and results, including failures.

## Coding workflow (from 2026-11-15, user instruction)

Code is written **through Claude Code**; Muse verifies the result.
Direct writing by Muse is the fallback only.

- Claude Code: `claude` 2.1.325 at `~/.local/bin/claude`
  (npm install under `~/workspace/claude-code/npm-global/bin/claude`).
  Use `PATH="$HOME/.local/bin:$HOME/workspace/claude-code/npm-global/bin:$PATH"`.
  Logged in with the user's Pro account (claude.ai OAuth);
  login expires 2026-12-15. Before each use, run
  `claude auth status` and confirm `loggedIn` is `true`. If it is
  expired or not logged in, report to the parent agent; the re-login
  request to the user is the parent's job, not this session's.
- Invoke non-interactively: `claude -p`, from the repo working
  directory, with a clear instruction, the target files, and the
  completion criteria. Never paste secrets into the prompt.
- Verify before committing: read every file Claude Code wrote or
  changed, run the build and the tests, and only then commit.
  Never commit Claude Code output unverified.
- 5-hour limit fallback: if Claude Code hits its 5-hour usage /
  rate limit, fall back immediately to Muse writing the code
  directly so work does not stop. Record the time and the affected
  work in the GitHub Issue, retry Claude Code after 5 hours, and
  return to the Claude Code flow as soon as it works again.
- Authorship in Issue records: every progress note states which
  parts were written by Claude Code and which parts Muse wrote
  directly. The naming-readability and progress-management rules
  are unchanged.
- Transition: work already in progress when this rule arrived
  (the three PoC builds) finishes at its natural breakpoint under
  the old flow; all coding after that goes through Claude Code.
  Already-written code (shared core, docs, CI) was written
  directly by Muse and is recorded as such.

## PR workflow (from 2026-11-15, user instruction)

- **Never ask the user to approve a PR.** The user does not want
  approval work.
- Every change goes through a pull request: push to a feature branch,
  open a PR, review it, then merge it ourselves.
- **Reviewers are Claude Code or a separate reviewer subagent**
  (never the author of the change reviewing their own work).
  Review checks: (1) build and tests pass, (2) code is readable,
  (3) names are clear, (4) no drift from the spec
  (`docs/notion-spec-summary.md` and this plan).
- If the review finds problems, fix them in the same PR, re-review,
  and only then merge.
- Record the review content and the CI results in a PR comment
  and/or the related Issue, stating clearly **who** reviewed it
  (Claude Code, or the reviewer subagent's role).
- **Report to the user only when blocked:** a design decision only
  the user can make, or branch protection that makes user approval
  mandatory for merging. In that case state exactly what is
  blocking. Never leave a PR waiting silently for the user.
- Note (2026-11-15): direct push to `main` already worked
  (scaffolding commit `0345b214`), so no branch protection
  currently blocks the GitHub App. PRs are still used, per the
  user's instruction, with self-merge by the App.

## GitHub progress tracking

- Issues #1–#7 (meta, 3 PoCs, shared core, CI, evaluation) — see
  `docs/github-scaffolding-status.md`.
- Every commit message names the PoC/area and the issue it serves,
  e.g. `poc(egui): add live-preview decoration rendering (#4)`.
- README + `docs/evaluation.md` are the user-facing progress record;
  Wiki is used only if the scaffolding step shows it is available,
  otherwise progress lives in README/docs (recorded in the status file).

## Evaluation axes (scored 1–5 at the end, evidence-linked)

Smoothness · live-preview ease · Japanese IME · proportional
font/layout · visual polish · build/startup · maturity/docs ·
license risk. Plus a short note per PoC on "what would hurt most
in the MVP". Recommendation must name one most-promising option
with reasons, and what would change the decision.
