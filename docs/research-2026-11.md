# Mica — GUI Framework Research (as of 15 November 2026)

**Project:** Mica — a custom Markdown editor (Obsidian-like live preview, WSL-first, keyboard-centric, no vault).
**Date of research:** 2026-11-15. All version/license claims below are grounded in sources checked on that date; anything that could not be verified is marked **unverified as of 2026-11-15**.
**Scope:** three candidate stacks — (1) Tauri 2 + CodeMirror 6, (2) GPUI, (3) egui / eframe — plus a one-paragraph note on alternatives. No code was written and no GitHub repository was touched for this report.

---

## 0. Executive summary

| Stack | Verdict in one line |
|---|---|
| **Tauri 2 + CodeMirror 6** | **Recommended for the PoC (and the likely production choice).** The only stack where the editor itself already exists: CodeMirror 6 is the engine Obsidian's Live Preview is built on, IME and fonts come free from the browser, and Tauri 2 is stable and actively released. The price is a webview (not a native GPU canvas) and a two-language codebase (TypeScript + Rust). |
| **egui / eframe 0.37** | **Best pure-Rust fallback; fastest to a working PoC.** Tiny build requirements, wgpu rendering, a real `TextEdit` widget. But there is no markdown editor: live preview, syntax highlighting, virtualization and polished typography must all be hand-built on `LayoutJob`, and CJK IME still has open, reproducible bugs. |
| **GPUI** | **Highest ceiling, highest risk — not recommended as the first PoC.** It is the only framework proven at 120 fps in a shipping editor (Zed), but the official crates.io release is a year stale, there is no first-party text editor or widget set, and the only first-party editor code (Zed's) is GPL and must not be copied. A PoC here means building an editor from scratch on a moving, under-documented API. |

**Preliminary ranking: 1. Tauri + CodeMirror 6 · 2. egui · 3. GPUI.** The PoC should still build all three in miniature, because the ranking hinges on two things that can only be settled by running code: (a) whether Tauri-on-WSL/Linux feels smooth enough, and (b) whether egui's IME behaviour is acceptable for daily Japanese input. GPUI's PoC is worth doing mainly to measure how much editor infrastructure it really demands.

---

## 1. Versions — what is current, and where to get it

### 1.1 Tauri 2

| Item | Finding |
|---|---|
| Latest stable | **Tauri `2.12.5`, released 21 October 2026** (crates.io / GitHub releases). The 2.12 minor itself shipped 26 September 2026. |
| Previous minors | 2.12.0 (26 Sep 2026), 2.12.3 (10 Oct 2026); 2.10.3 (4 Mar 2026), 2.11.5 (1 Jul 2026). |
| Next major | **Tauri 3 is in alpha**: `3.0.0-alpha.6` released 6 Nov 2026. Do not build Mica on it. |
| 2.0 stable | 2 October 2024 — so the 2.x line has had ~2 years of production use and 70 stable releases since Feb 2024. |
| Where to get it | crates.io (`tauri`, `tauri-build`, `@tauri-apps/cli` on npm), `create-tauri-app`. Source: `github.com/tauri-apps/tauri`. |
| MSRV | **Rust 1.90**, raised in 2.12 (policy: stable − 3, updated only in minor/major releases). This was possible because 2.12 dropped official Windows 7 support — the latest WebView2 installer no longer runs on Windows 7 at all. |

Sources: Tauri release list, https://tauri.app/release/core/ ; "Announcing Tauri 2.12", https://v2.tauri.app/blog/tauri-2.12/ (26 Sep 2026); Tauri 2.12.5 release notes via https://dev-news.anonyfox.com/tauri/ (checked 15 Nov 2026).

### 1.2 CodeMirror 6

CodeMirror 6 has **no single version number** — it is ~20 independently versioned npm packages under `@codemirror/*`, plus a thin `codemirror` meta-package. Quote per-package versions, never "CodeMirror 6.x" alone.

| Package | Latest verified (date) |
|---|---|
| `@codemirror/view` (the editor view — the package that matters most) | **6.43.15 (19 Oct 2026)** |
| `@codemirror/state` | **6.7.6 (22 Sep 2026)** |
| `@codemirror/language` | **6.13.1 (7 Oct 2026)** |
| `@codemirror/lint` | **6.9.8 (13 Nov 2026)** — two days before this report |
| `@codemirror/autocomplete` | 6.20.4 (19 Oct 2026) |
| `@codemirror/merge` | 6.12.3 (19 Oct 2026) |
| `@codemirror/commands` | 6.11.1 (15 Sep 2026) |
| `@codemirror/streamparser` | 6.0.0 (7 Oct 2026, first numbered release) |
| Meta-package `codemirror` | **6.0.2 (19 Jun 2025)** — do not mistake this for the CM6 version; it is rarely bumped |
| Legacy CodeMirror 5 | 5.65.21 (7 Feb 2026) — maintenance only |

Cadence: `@codemirror/view` shipped 6.43.0 → 6.43.15 between May and October 2026 — steady, small, bug-fix-driven releases. Where to get it: npm (`npm i @codemirror/view @codemirror/state @codemirror/lang-markdown @lezer/markdown` etc.). Source: official changelog, https://codemirror.net/docs/changelog/ (read 15 Nov 2026).

### 1.3 GPUI

This is the most confusing version story of the three, and it matters for the PoC.

| Item | Finding |
|---|---|
| Official crates.io release | **`gpui 0.2.2`, published 22 October 2025** — still the latest official release as of 15 Nov 2026, i.e. **~13 months stale**. 0.2.0 (9 Oct 2025), 0.2.1 (14 Oct 2025), 0.2.2 (22 Oct 2025), then nothing. |
| What Zed itself runs | Zed **1.28.1, released 11 Nov 2026** (Zed 1.0 shipped 29 Apr 2026). Zed main has diverged massively from 0.2.2: crate split (`gpui`, `gpui_platform`, `gpui_macos`, `gpui_linux`, `gpui_windows`, `gpui_wgpu`, `gpui_web`, `scheduler`…), wgpu Linux renderer (merged 13 Feb 2026, PR #46758), AccessKit (merged 27 May 2026), cosmic-text 0.19, taffy 0.10.1. The version on main is still `0.2.2` — not even bumped. |
| How to get current GPUI | **There is no official current release.** Options, in descending order of "officialness": (a) a **git dependency on `zed-industries/zed`** at a pinned rev; (b) **community republishes** — `gpui-pre` 0.3.12 (9 Nov 2026, Longbridge snapshot of zed@d2a4fb6, Apache-2.0), `gpui-unofficial` 1.28.1 (mirrors the Zed release tag, Apache-2.0, explicitly "not affiliated with or maintained by Zed Industries"), or the `gpui-ce` community fork; (c) the stale official 0.2.2. |
| Widget kit | `gpui-component` (Longbridge, Apache-2.0) **0.7.0, released 28 Sep 2026** — 60+ widgets; 0.5.x was the line current in Feb 2026. Note 0.6+ depends on the `gpui-pre` family, not on official `gpui` 0.2.x. |
| Scaffolding/docs | No changelog is published for gpui itself. `create-gpui-app` exists but was untouched since Apr 2025 (373 stars, July 2026 snapshot). |

Sources: https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/HEAD/report/03-gpui.md (research collected 7 Jul 2026, reconciled 9 Jul 2026); https://baeseokjae.github.io/posts/native-app-for-coding-agents-rust-gpui/ (Oct 2026); crates.io pages for `gpui-unofficial` / `gpui-pre` via https://index.githits.dev/packages/crates/gpui-pre/ ; Zed 1.28.1 via https://dev-news.anonyfox.com/zed/ (checked 15 Nov 2026).

**Implication for Mica:** any GPUI PoC must decide its dependency route *first* (git-pin vs `gpui-pre` vs stale 0.2.2), and that choice is itself a maintenance commitment. Pin the exact rev/tag in the PoC and record it.

### 1.4 egui / eframe

| Item | Finding |
|---|---|
| Latest stable | **egui / eframe 0.37.0, released 15 October 2026** |
| Recent history | 0.34.0 (26 Mar 2026), 0.35.0 (25 Jun 2026), 0.36.0 (5 Aug 2026), 0.36.1 (7 Aug 2026), 0.36.2 (8 Sep 2026), 0.37.0 (15 Oct 2026). Roughly one minor every ~6–8 weeks, plus patches. |
| Where to get it | crates.io (`egui`, `eframe`, `epaint`, `egui-wgpu`, `egui-winit`, `egui_extras`). Source: `github.com/emilk/egui`. |
| MSRV | **Rust 1.95** at 0.36.1 (per a 29 Aug 2026 audit of the 0.36.1 tag; MSRV was 1.88 at 0.33.0, 1.85 at 0.32.0). MSRV for 0.37.0 specifically is **unverified as of 2026-11-15** — expect ≥ 1.95. |
| Breaking changes | Frequent and expected: still **0.x**, README states "interfaces are still in flux. New releases will have breaking changes." E.g. 0.34 replaced `App::update` with `fn logic` + `fn ui`; 0.37 removed `default_fonts` as a default feature and deprecated `Id::new` / `AsId`. |

Sources: https://github.com/emilk/egui/blob/HEAD/crates/eframe/CHANGELOG.md (read 15 Nov 2026); https://docs.rs/crate/egui/latest ; https://rusttutorial.org/gui-apps/ (versions as of Oct 2026).

---

## 2. Build requirements

### 2.1 Tauri 2

| Platform | Requirements |
|---|---|
| **Linux (Debian/Ubuntu)** | `build-essential curl wget file libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev` (plus `patchelf`). Note: `libwebkit2gtk-4.0-dev` no longer exists on Ubuntu 24.04 — Tauri 2 targets **4.1**; a 4.0 reference means a stale v1 guide. Fedora: `webkit2gtk4.1-devel …`; Arch: `webkit2gtk-4.1 …`. This is the heaviest Linux requirement of the three stacks and the main WSL friction point: the webview dev libraries must be installed before `cargo test`/`cargo build` will even link. |
| **Windows** | MSVC C++ Build Tools + Windows SDK (MinGW/GNU targets are **not** supported — WebView2 bindings assume the MSVC ABI). **WebView2 Runtime** — preinstalled on Windows 10 1803+ and Windows 11; otherwise install the Evergreen Bootstrapper. VBScript optional feature only for MSI bundles. Rust MSVC toolchain via rustup. |
| **Common** | Rust ≥ 1.90 (MSRV), Node.js (≥ 20.19 in one 2026 guide; frontend toolchain) + a JS package manager, Tauri CLI 2.x (`@tauri-apps/cli` or `cargo tauri`). |

Sources: Tauri prerequisites as summarised in https://github.com/stuffbucket/skills/blob/HEAD/plugins/stuffbucket/skills/tauri-setup-prerequisites/SKILL.md and https://github.com/anwardebes/vsclaude/blob/HEAD/specs/BUILD_AND_DISTRIBUTION.md ; MSRV from https://v2.tauri.app/blog/tauri-2.12/.

### 2.2 GPUI

| Platform | Requirements |
|---|---|
| **Linux** | No winit, no system webview. In-house Wayland (`wayland-client` + `calloop`) and X11 (`xcb`) backends; GPU via Vulkan (blade in 0.2.2) or wgpu (main). Needs a working Vulkan driver / GPU stack. An exact `apt` package list for building current GPUI on Ubuntu is **unverified as of 2026-11-15** — the PoC must record the list it actually needed. |
| **Windows** | Raw Win32 via `windows-rs`, Direct3D 11 renderer. MSVC toolchain expected (as for any Rust Windows GUI work). Zed for Windows reached GA 15 Oct 2025, so the Windows backend is no longer "in progress" in Zed itself — but that maturity lives on Zed main, not in the 0.2.2 crate. |
| **Common** | Rust (Zed pins 1.95.0; community GPUI apps pin 1.96.1 — Oct 2026 sources). No Node.js, no webview libraries. First build compiles a graphics stack from source and is slow (reported Oct 2026). |

Sources: GPUI deep dive, https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/HEAD/report/03-gpui.md §§1–2, 5; https://baeseokjae.github.io/posts/native-app-for-coding-agents-rust-gpui/.

### 2.3 egui / eframe

| Platform | Requirements |
|---|---|
| **Linux (Debian/Ubuntu)** | `libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev` (eframe README list), plus for wgpu builds `libwayland-dev libvulkan-dev libgl-dev` and `pkg-config`, `cmake` in some projects. Wayland and X11 are cargo features (`wayland`, `x11`) — both on by default. No webkit, no GTK. |
| **Windows** | Nothing extra beyond the MSVC C++ tools; DX12/Vulkan drivers come with GPU drivers. Both `windows-msvc` and `windows-gnu` targets are CI-tested. |
| **Common** | Rust ≥ ~1.95. `cargo run` / `cargo build` only — no Node, no bundler, no codegen step. The lightest build story of the three. |

Sources: https://rusttutorial.org/gui-apps/ ("Platform prerequisites"); eframe 0.37.0 Cargo.toml features via https://docs.rs/crate/eframe/latest/source/Cargo.toml.orig ; https://deepwiki.com/peters/horizon/1.1-getting-started (eframe/wgpu Linux deps).

---

## 3. GPU / rendering and smoothness

| Stack | Rendering | Smoothness claim (and how to read it) |
|---|---|---|
| **Tauri 2** | **No GPU canvas of its own.** It embeds the OS webview: Edge WebView2 (Chromium) on Windows, WebKitGTK on Linux, WebKit on macOS. All drawing is the browser's DOM/CSS/compositor. | Smoothness = browser smoothness: generally 60 fps for ordinary editor UI, vsync'd, GPU-composited by Chromium/WebKit. There is **no 120 fps story** and no control over the compositor; large-DOM editors can jank on relayout, and Linux WebKitGTK is the weakest of the three engines (older WebKit, distro-dependent version). Tauri 3 alpha adds an optional CEF runtime (`tauri-runtime-cef`), which would make the engine uniform — but it is alpha, not a basis for Mica today. Startup: small binary (~1 MB installer in an Oct 2026 benchmark vs 106 MB for Electron), but first window still pays webview + JS startup. |
| **GPUI** | True GPU rendering, no browser. macOS: in-house **Metal** renderer (shaders precompiled by build.rs); Windows: in-house **Direct3D 11** (+DXGI, DirectComposition); Linux: **blade/Vulkan** in 0.2.2, **rebuilt on wgpu** on main (Feb 2026). Primitives (quads, shadows, paths, glyph runs from a cached atlas) are rasterized by shaders; layout (taffy) → prepaint (hitboxes, text shaping) → paint (batched GPU draw calls). | **120 fps is the design target, with an 8.33 ms whole-frame budget** (Zed's founding goal; Zed holds 120 Hz ProMotion in production). This is the only stack with a shipping proof of that claim. Caveat: the claim is Zed's, on macOS/Metal first; Linux/wgpu and Windows/D3D11 are less documented, and complex nested flexbox layouts can spike frame time (taffy runs synchronously on the UI thread). |
| **egui / eframe** | **wgpu is the default renderer since 0.34** (Mar 2026); glow (OpenGL) remains as fallback. `epaint` tessellates everything into textured triangles. Immediate mode: the whole UI is re-declared every frame. | egui's stated goal is responsiveness with a **60 Hz debug-build target** — treat that as an aspiration, not a measurement. In practice eframe is fast for tool/debug UIs; there is no 120 fps discipline or published frame-time budget. Repaints are demand-driven (no repaint when idle, except for animations/caret blink), which keeps CPU/RSS low, but a full-document re-layout every frame is exactly what a large markdown editor must avoid by virtualizing — and egui gives no virtualized text editor out of the box. |

Sources: GPUI — https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/HEAD/report/03-gpui.md §2; https://www.shuffleapp.co/blog/building-shuffle-rust-gpui ; https://gpui-starter.freeoxide.com/blog/electron-vs-tauri-vs-gpui/ . egui — https://github.com/emilk/egui/blob/HEAD/crates/eframe/CHANGELOG.md (0.34.0 entry); https://docs.rs/crate/egui/latest . Tauri — installer-size benchmark via https://dawloom.com/blog/tauri-or-electron-in-2026/ (checked 7 Nov 2026); CEF runtime via https://tech-insider.org/tauri-vs-electron-2026/ .

---

## 4. Live preview / rich-text editing: built-in vs build-from-scratch

### 4.1 Tauri + CodeMirror 6 — the editor already exists

**Built-in (CodeMirror 6):** a complete, virtualized text editor — immutable `EditorState`, transactions, multiple selections, undo/redo history, search, autocomplete, lint, bracket matching, Lezer incremental parsing (`@lezer/markdown`, `@codemirror/lang-markdown`), syntax highlighting, folding, and `basicSetup` (15+ features in one bundle). It handles million-line documents by rendering only the viewport.

**Live Preview is a well-trodden pattern, but you still write it (~600 LOC):** there is no official "live preview" package. The standard implementation — the one Obsidian itself uses — is a CodeMirror 6 `ViewPlugin`/`StateField` that walks the Lezer syntax tree and applies `Decoration`s:

- `Decoration.mark` — CSS classes for bold/italic/strikethrough/inline-code/headings (proportional sizes via CSS, no layout jump if line-height is fixed per heading class);
- `Decoration.replace` / `Decoration.widget` — hide syntax markers (`**`, `##`, `` ` ``, `>`) and replace checkboxes, images, horizontal rules, tables, math and code fences with rendered DOM widgets;
- **cursor-aware toggling** — when the cursor enters a decorated range, the widget is removed and raw Markdown reappears; when it leaves, it re-renders. This is the signature Obsidian behaviour and it is *not* a dual-pane preview — it is one surface with dynamically toggled decorations.

Known pitfalls documented by projects that did this in 2026: block-level replace decorations must go through a `StateField` (not a `ViewPlugin`) or vertical layout breaks; `EditorView.atomicRanges` is needed over replaced blocks; heading sizes must come from `Decoration.line` CSS classes so line height never jumps. Reference implementations to study (not depend on): SilverBullet's `client/codemirror/` (MIT) and ixora (Apache-2.0). The Reading view (full rendered HTML) is a separate, easy mode — the same Markdown→HTML pipeline (unified/remark/rehype, KaTeX, Mermaid) can feed both the block widgets and Reading view.

**What Mica still builds:** the decoration layer itself, wikilink/hashtag Lezer extensions, the file/notes model, and the Rust backend (file watching, search, IPC via Tauri commands). The *editing engine* is bought, not built.

Sources: https://deepwiki.com/fryholic/sticker-md/3.3.1-live-preview ; https://github.com/bejranonda/externalbrain/blob/HEAD/research/obsidian/architecture.md §3 ; https://github.com/nodummd/nodum/blob/HEAD/docs/research/DECISIONS.md §1 (verified against npm 12 Aug 2026).

### 4.2 GPUI — no editor; build it from scratch

**Built-in:** essentially nothing above the primitive layer. GPUI ships low-level elements, `uniform_list` virtualization, focus/keybinding/action systems, and a 746-line `input.rs` example showing what a text input takes. There is **no first-party text-input widget, no rich-text widget, no markdown support, no syntax highlighting, and no multi-line editor**.

**What Mica would build:** everything — a rope/text buffer, shaping per paragraph into positioned glyph runs (via CoreText/DirectWrite/cosmic-text), a shaped-line cache invalidated per edited paragraph, exact-height virtualization (estimated heights cause scrollbar jumps), selection/caret geometry (including BiDi, which is known-broken: see §6), undo/redo, clipboard, and the whole Live Preview decoration concept re-implemented as custom `Element`s. Zed's own editor proves it *can* be done brilliantly — but Zed's editor is a multi-year, funded-team effort, and its code is GPL (see §8). The permissive alternative, `gpui-component`, ships a code editor (tree-sitter highlighting, LSP via provider traits) under Apache-2.0 — code-oriented, not a Markdown live-preview editor; adapting it is a project in itself.

**Honest cost, as stated by a 2026 GPUI notes app:** "we build the rich-text editor ourselves (Zed's editor is monospace-code-specific). That's the project's long pole."

Sources: GPUI deep dive §§2, 5, https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/HEAD/report/03-gpui.md ; https://github.com/appsicle/daisynotes/blob/HEAD/PLAN.md §§3, 5.

### 4.3 egui / eframe — a real text widget, but not an editor

**Built-in:** `TextEdit` — single-line and multi-line, copy/paste, undo, emoji, IME hooks, double/triple-click selection (0.37 added double-click-and-drag and triple-click-and-drag selection). `RichText` / `LayoutJob` / `TextFormat` give per-span font, size, colour, underline/strikethrough, and **explicit line height** (`TextFormat::line_height`, `LayoutJob::extra_text_line_spacing` since 0.36). `epaint`'s text stack (HarfRust shaping since 0.34) handles shaping; 0.37 added a `system_fonts` feature (on by default) so any installed glyph / coloured emoji renders, and fixed missing Chinese glyphs from system fonts on Windows.

**What Mica would build:** the Markdown layer and the editor architecture. `TextEdit` edits one plain `String` — there is no syntax tree, no decoration/widget system, no per-line-type rendering, and no built-in virtualization for large documents. A Live Preview in egui means either (a) rendering styled text as a read-only `LayoutJob` and swapping to a raw `TextEdit` on focus (losing cursor-precise inline reveal), or (b) building a custom text-editing widget on `epaint`/`LayoutJob` with per-paragraph layout, click-to-cursor mapping, and IME handling — i.e. a small editor engine. Syntax highlighting would be hand-rolled on top of `LayoutJob` spans. This is feasible and is the normal egui way (everything is assembled from primitives), but it is the bulk of the Mica work, and the result will not match CodeMirror's editing maturity without sustained effort.

Sources: https://docs.rs/crate/egui/latest ; epaint changelog, https://github.com/emilk/egui/blob/main/crates/epaint/CHANGELOG.md (0.34–0.37 entries); https://github.com/emilk/egui/blob/HEAD/crates/eframe/CHANGELOG.md (0.37.0).

---

## 5. Japanese IME support

This is a first-class Mica requirement (the user writes in Japanese). The three stacks differ sharply: one inherits the browser's IME, two hand-roll it on winit/OS APIs.

### 5.1 Tauri + CodeMirror 6 — inherits the browser IME (best position)

IME handling comes from Chromium (Windows) / WebKitGTK (Linux) and CodeMirror's composition handling. CodeMirror 6 uses the **EditContext API on Chrome 126+** for native input handling, and its 2026 changelog is full of composition fixes — evidence of active maintenance, not of instability: view 6.43.12 (Sep 2026) fixed DOM updates for marks around an active composition and a Firefox composition-outside-line-container issue; 6.43.10 fixed highlighted spaces merging during composition; 6.43.1 worked around macOS-Chrome IME interruption (Tibetan) and improved widget reuse near composition; 6.43.14 fixed composing over a selection in a read-only editor. Known edge cases remain on mobile (Android GBoard autocorrect quirks — mostly closed/worked-around by 2026; one open issue is Boox-e-reader-specific, not stock GBoard). For desktop Japanese input in a webview this is the most battle-tested path: the same stack Obsidian, VS Code (web) and Chrome DevTools rely on. **Linux caveat:** the IME is WebKitGTK's, not Chromium's — its Japanese IME behaviour (IBus/Fcitx integration) is the least documented of the three engines and must be tested in the PoC on the actual WSL/Linux setup, including candidate-window placement. That is a PoC test item, not a known blocker.

Sources: https://codemirror.net/docs/changelog/ (6.43.x entries, read 15 Nov 2026); https://github.com/bentlybro/kern/blob/HEAD/docs/research/verifications.md (CM6 Android/GBoard verification).

### 5.2 GPUI — real support, but only if you implement `EntityInputHandler`

IME exists at the platform layer — macOS implements `NSTextInputClient`, Wayland uses the text-input protocol, Windows handles `WM_IME` events — **but only for widgets that implement `EntityInputHandler`** (marked-text ranges, etc.). The official `input.rs` example is 746 lines, which is a fair measure of the work for a single-line input; a multi-line editor needs more. Known rough edge: **#56149, IME candidate-window position on Windows** (open). RTL text input/rendering support (#31102) remains open, and caret/selection geometry is known to break inside reordered BiDi runs on macOS — Japanese itself is LTR, so this is a secondary concern, but it signals the text system's maturity limits. A newer data point: `tauri-plugin-gpui` (which embeds GPUI in Tauri windows) documents per-OS IME behaviour in detail — on Linux it routes keys through `GtkIMMulticontext` (IBus/Fcitx/XIM, in-place composition, candidate window at the caret), on Windows via IMM32 at the caret, on macOS candidates appear at the window's top-left corner (unsupported). That plugin is not GPUI itself, but it shows what correct IME plumbing costs.

Sources: GPUI deep dive §3, https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/HEAD/report/03-gpui.md ; https://crates.io/crates/tauri-plugin-gpui (Input methods section).

### 5.3 egui — works, with open CJK bugs you must test against

IME arrives via **winit** (`ImeEvent::Preedit/Commit/DeleteSurrounding`) and `TextEdit` tracks `ime_enabled` / `ime_cursor_range`, reporting the candidate-window rect to the platform. There has been real progress: 0.34.0 (Mar 2026) "Much improved IME" (#7967); 0.36.0 (Aug 2026) improved mobile keyboard/IME support; 0.36.2 added `TextEdit::event_filter` (per-language IME key handling — e.g. whether Enter confirms a composition differs between Chinese, Japanese and Korean users).

But the bug tracker still carries reproducible CJK problems, and they cluster exactly on Mica's platforms:

- **#7908 (open): Backspace deletes characters in `TextEdit` during IME composition (CJK)** — the Backspace is double-consumed by the IME and by TextEdit. Reported on egui/eframe 0.30, reporter states it "persists even in the latest versions". Root-cause discussion points at winit not marking the confirming key as consumed on Windows (macOS does this, which is why macOS is unaffected).
- **#7485 (open, Aug 2025): incorrect typing with IBusWayland enabled** — on Fedora 42 / KDE Plasma 6.4.4 / Wayland, only one character can be typed before refocusing; disabling IBusWayland fixes it. The reporter notes it does not occur on Windows (or Wine). This is a Linux/Wayland IME interaction, directly relevant to a WSL-first Linux target.
- **History:** Linux IME was disabled entirely in 0.29.1 (#5188) to fix Backspace/arrow keys being captured via XIM, then re-enabled in 0.31.0 (#5198). The pattern — IME fixes regressing other keys — has repeated.

**PoC implication:** Japanese IME in egui cannot be assumed from the changelog; the PoC must include a scripted IME test (type Japanese, convert with Space, confirm with Enter, Backspace mid-composition, candidate-window position) on **Windows (MS IME / Google Japanese Input) and Linux/Wayland (IBus/Fcitx5)**, and record pass/fail per case.

Sources: https://github.com/emilk/egui/issues/7908 ; https://github.com/emilk/egui/issues/7485 ; https://github.com/emilk/egui/blob/HEAD/crates/eframe/CHANGELOG.md (0.29.1, 0.31.0, 0.34.0, 0.36.0, 0.36.2 entries); https://deepwiki.com/membrane-io/egui/3.5-text-input-and-editing (IME state tracking).

---

## 6. Proportional fonts, variable line height, custom layout

| Stack | How easy |
|---|---|
| **Tauri + CodeMirror 6** | **Trivial — it is CSS.** Proportional fonts, per-heading sizes, variable line height, custom line spacing, web fonts, `font-feature-settings`, ruby/furigana via HTML — all standard web tech. CodeMirror 6 handles mixed font sizes per line (its view code has explicit fixes for mixed-font-size lines, e.g. 6.41.0 `posAtCoords` in mixed-font-size lines) and variable line heights; block widgets can be arbitrary DOM. Layout of the app shell is HTML/CSS (flexbox/grid). This is the lowest-risk answer to Mica's typography goals. |
| **GPUI** | **Powerful, hand-rolled.** Layout is **taffy** (flexbox/grid, shared with Dioxus/Bevy/Servo) with a Tailwind-flavoured builder API (`div().flex().flex_col().gap_2()`). Text is shaped per-paragraph by the platform shaper (CoreText / DirectWrite / cosmic-text 0.14–0.19 + swash), so proportional fonts and per-paragraph sizes are natural — *if* you write the paragraph layout yourself. There is no CSS cascade and no rich-text layout primitive: variable line height inside one editor surface means custom `Element` layout code with a shaped-line cache. Full control, full cost. |
| **egui** | **Possible per-span, awkward per-document.** `LayoutJob`/`TextFormat` support proportional families, per-span size/colour/line-height (`TextFormat::line_height`, `extra_text_line_spacing`), and variable fonts (0.34 font-variations API). `FontDefinitions` / `Context::set_fonts` install custom faces; 0.37's `system_fonts` provider resolves system faces at runtime. But: layout is immediate-mode (the classic paradox — to centre a dialog you must know its size before laying it out), there is no CSS, and per-line variable heights inside a single `TextEdit` are not a supported concept — `TextEdit` is uniform text. Custom layout means custom widgets and manual measurement (`measure_height` patterns, invisible child-`Ui` passes). CJK vertical-centring quirks exist (epaint adds a font's whole `lineGap` below the baseline; faces with large gaps, e.g. Hiragino Sans GB at 0.5 em, render visibly high unless corrected). |

Sources: CodeMirror changelog (6.41.0, 6.40.0 `moveVertically` in wrapped lines with large line height), https://codemirror.net/docs/changelog/ ; GPUI taffy — https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/HEAD/report/00-ecosystem-map.md §1.4; egui — https://docs.rs/crate/egui/latest , epaint changelog (0.36.0 `extra_text_line_spacing`, 0.37.0 `TextFormat::valign` fix), https://github.com/miaokela/tinyterm-egui/blob/HEAD/skills/tinyterm-egui-dev/references/theme.md §3 (CJK lineGap note).

---

## 7. Maturity, documentation, community, breaking-change risk

| | Tauri 2 + CM6 | GPUI | egui / eframe |
|---|---|---|---|
| **Maturity** | Highest. Tauri 2.0 stable Oct 2024, 2.x in active release; CM6 is the editor inside Obsidian, Replit, Chrome DevTools. Production-proven on all three OSes. | Proven *as Zed*, unproven *as a framework for others*. Zed 1.0 Apr 2026, 1.28.1 Nov 2026, ~91k stars. But external use is a side-effect of Zed's development; in Feb 2026 Zed leadership said GPUI work not tied to Zed's use case is deferred. | Mature as a library, young as an app platform. egui is sponsored by Rerun, powers Rerun's viewer and many tools/games; eframe is its official app shell. Still 0.x by design. |
| **Documentation** | Best. Tauri has full docs (v2.tauri.app), guides, plugin docs, and a large tutorial/blog corpus. CodeMirror has complete API docs + changelog + examples. | Weakest. No book, no cookbook, no changelog; "the documentation that does exist is Zed itself." API docs exist on docs.rs for 0.2.2 (stale). Community starters (gpui-starter, gpui-component docs) partially fill the gap. | Good for a 0.x library: docs.rs for every crate, a demo app (`egui_demo_app`), many examples. Tutorials rot quickly because the API moves each minor (e.g. `App::update` → `logic`/`ui` in 0.34). |
| **Community / examples** | Largest of the three: Tauri ~105k stars (Oct 2026), 200+ community plugins (May 2026), Discord/forum, CrabNebula backing, Tauri Foundation under the Commons Conservancy. CM6: the default embeddable editor on the web. | Small and concentrated: ~100 reverse deps of `gpui` on crates.io (Jul 2026), ~45 apps / 20 libraries on awesome-gpui; the serious widget ecosystem is one third party (Longbridge). Zed's $32M Series B (Aug 2025, >$42M total) funds Zed, not GPUI-for-others. | Large for Rust GUI: egui is the most-used pure-Rust GUI; `egui_extras`, `egui_dock`, `egui_tiles`, `egui_plot`, `egui_dnd` form a real ecosystem. Funded via Rerun. |
| **Breaking-change risk** | Low for the PoC horizon. Tauri 2.x is semver-stable within 2.x (minor MSRV bumps are the main churn); CM6 packages are 6.x with additive, patch-driven releases. The risk is Tauri 3 (alpha) tempting a future migration. | **Highest.** Pre-1.0, no stability policy ("there will often be breaking changes between versions"), no release cadence for external users, and the ground moves under you (crate split, renderer swap, AccessKit) without a version bump. Mitigation is only pinning. | Medium-high, but manageable. Expect a breaking change every minor (~6–8 weeks); the changelog is thorough and migration notes exist. Pin `=0.37.0` in the PoC and budget upgrade time. |
| **Accessibility** | Browser a11y tree for free (screen readers, keyboard nav, IME inherited from webview). | Zero in any released version; AccessKit merged on main May 2026, unreleased, and Zed itself currently opts out (`Application::inaccessible()`) after nightly panics. | AccessKit is a default eframe feature; screen-reader support exists on native and web (opt-in flag on web). Better than GPUI, worse than a webview. |

Sources: maturity/docs — https://www.shuffleapp.co/blog/building-shuffle-rust-gpui ("no book, no cookbook"); https://goldstrikearch.github.io/rust-gui-desktop-ecosystem-state/ (AccessKit/taffy convergence, Jul–Aug 2026); Tauri stars/plugins — https://www.masturbyte.com/tauri-2-desktop.html (May 2026), https://www.opensourcedrop.com/tools/tauri-apps/tauri (Oct 2026); Zed funding — GPUI deep dive §6.

---

## 8. Licenses — including the GPL warning

All license statements below were checked against the projects' own repos/docs or SPDX metadata in third-party license inventories; see Sources.

| Component | License | Notes |
|---|---|---|
| **Tauri 2** (incl. `tauri`, `tauri-build`, `tauri-utils`, `tauri-runtime-wry`, plugins) | **MIT OR Apache-2.0** | Permissive, dual-licensed. The windowing crate `tao` is **Apache-2.0 only** (no MIT option) — still permissive; it only adds Apache's notice/patent-termination obligations. |
| **Wry** (webview layer) | MIT OR Apache-2.0 | Same as Tauri. |
| **CodeMirror 6** (`@codemirror/*`, `@lezer/*`) | **MIT** | Every package. No copyleft anywhere in the CM6 dependency closure as inventoried by downstream MIT apps (208 MIT JS packages in one audited Tauri+CM6 app's bundle). |
| **GPUI** (`gpui`, `gpui_platform`, `gpui_macos`, `gpui_linux`, `gpui_windows`, `gpui_wgpu`, `gpui_web`, `gpui_macros`, `util`, `collections`, `sum_tree`, …) | **Apache-2.0** | Permissive. The framework itself is safe to use in a proprietary or MIT-licensed Mica. |
| **Zed editor crates** (`editor`, `text`, `rope`, `language`, `lsp`, `project`, `multi_buffer`, `workspace`, `theme`, `ui`, `settings`, `fs`, `worktree`, `zed`, … — ~198 crates) | **GPL-3.0-or-later** | ⚠️ **THE CRITICAL WARNING.** These are copyleft. In Rust, static linking creates one combined work, so a *distributed* Mica binary that links any GPL Zed crate (e.g. `zed::editor`, `zed::text`, `zed::rope`) is a GPL work and must be distributed under GPL terms with source available. Using or privately compiling GPL software is unrestricted — obligations attach on **distribution** of the binary — but Mica intends to distribute, so this bites. |
| Zed collaboration server | GPL-3.0-or-later (was AGPL-3.0 when open-sourced Jan 2024; moved to GPL ~May 2026) | Not relevant to Mica unless it ever reused server code. Not AGPL any more, per a 13 Oct 2026 fact-check — treat the AGPL label in older articles as stale. |
| **egui / eframe / epaint** | **MIT OR Apache-2.0** | Permissive, dual-licensed — the standard Rust choice. |
| `gpui-component` (Longbridge) | Apache-2.0 | Permissive. The safe widget/editor source for GPUI work. |
| `gpui-pre` / `gpui-unofficial` republishes | Apache-2.0 (as republished) | They republish Zed's Apache-2.0 GPUI crates; the license follows the crates, but verify the republish's own notice and pin the exact upstream commit. |

### The GPL rule for Mica — read this before writing any GPUI PoC code

1. **Do NOT copy, vendor, or depend on Zed's editor crates** (`editor`, `text`, `rope`, `language`, `multi_buffer`, `workspace`, `ui`, `ui_input`, …) in Mica, and do not copy their code into Mica's repo — even "just a small piece" (e.g. the rope or the selection logic). That would make Mica GPL-3.0-or-later.
2. **Using GPUI itself is safe** — it is Apache-2.0, a different crate set from the editor, in the same monorepo. Keep the boundary explicit: depend only on `gpui*` crates (or their Apache-2.0 republishes) and on `gpui-component`.
3. **Design-only reference is safe.** Reading Zed's editor to understand *how* an editor is architected (shaped-line cache, paragraph invalidation, virtualization with exact heights, the `Element`/`EntityInputHandler` patterns) and then writing your own implementation is learning from a design, not copying code. Do not paste code, do not transliterate function-by-function, and do not keep Zed source in the Mica repo. When in doubt, write it from the concept, not from the file.
4. **If Mica ever wanted Zed-editor code, the only clean options would be** to license Mica itself as GPL-3.0-or-later, or to keep the GPL code in a separate process behind an IPC boundary (which is a different architecture, not a loophole to assume). Neither is the plan; the plan is Apache/MIT throughout.

Sources: https://github.com/jonx/macaros/blob/HEAD/docs/features/zed-editor/README.md (licensing boundary table: 28 Apache crates vs 198 GPL crates, GPL-on-distribution analysis); https://www.promptquorum.com/power-local-llm/zed-review (license verified against the repo's own LICENSE-GPL file, 12 Sep 2026); https://cloudparts.dev/technologies/zed/ (AGPL→GPL server move, facts checked 13 Oct 2026); GPUI deep dive §6, https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/HEAD/report/03-gpui.md ; Tauri/CodeMirror licenses — https://docs.typeward.com/reference/license/ and https://github.com/aryrabelo/tapa/blob/HEAD/THIRD-PARTY-LICENSES.md (SPDX inventory of a Tauri+CM6 app); egui — https://github.com/confeden/nova-prism/blob/HEAD/docs/research/06-gui-framework.md §1.3 (MIT/Apache-2.0).

---

## 9. Alternatives — one paragraph each (brief; out of PoC scope)

- **Slint.** A declarative UI framework (`.slint` markup + Rust/C++/JS logic) with a live-preview designer, good docs, and consistent cross-platform rendering (femtovg/wgpu/Skia backends, parley text). Version 1.17 in the July 2026 ecosystem survey (with first-class `DragArea`/`DropArea`); current November version **unverified as of 2026-11-15**. The catch is licensing: **GPLv3, or a royalty-free licence that requires disclosing Slint use, or a paid commercial licence** — a real constraint for Mica, and the reason it is not a PoC candidate. No built-in Markdown editor either.
- **Dioxus.** React-style components in Rust; desktop defaults to a webview (like Tauri), so it inherits Tauri's IME/font advantages, but with a smaller community and faster-breaking API. Its native renderer (**Blitz**, 0.7.9 in the July 2026 survey, Vello/parley-based) is still in development and would face the same "build the editor yourself" problem as GPUI/egui. Interesting if Mica ever wanted full-stack Rust; not a better editor bet than Tauri+CM6 today.
- **Wails (Go).** The Go analogue of Tauri: system webview + Go backend. Version 3 was the current line in 2026 surveys; exact November 2026 version **unverified as of 2026-11-15**. Viable if Mica were a Go project; it is a Rust project, so Wails adds a language without adding an editor.
- **Flutter (Dart).** A complete, polished, GPU (Skia/Impeller) framework with its own text/IME stack and excellent tooling — but Dart, not Rust; a large runtime; and its text editing is designed for app UI, not for building a CodeMirror-class Markdown editor. Rejected for Mica on language fit, not on quality.
- **Others seen in 2026 surveys (iced, Xilem, Floem, Freya, Vizia):** all real, all still 0.x/experimental for editor-class apps, all with the same "no editor, build it yourself" gap and smaller communities than egui. Iced is the closest to egui in spirit (wgpu, cosmic-text) and worth a glance if egui's IME proves unusable — but it does not change the ranking.

Sources: https://gpui-starter.freeoxide.com/blog/rust-gui-frameworks-comparison-2026/ (Slint licensing, Dioxus, Xilem, Vizia, Jun 2026); https://goldstrikearch.github.io/rust-gui-desktop-ecosystem-state/ (Slint 1.17, Dioxus Blitz 0.7.9, Jul–Aug 2026); https://github.com/janwirth/ecosystem_review/blob/HEAD/application-types/desktop.md (Wails v3, snapshot 14 Jun 2026).

---

## 10. Evaluation axes and scoring rubric for the PoC

Score each axis **1–5** after running the PoC (not from this report alone). Score what you *measured*, and write the evidence (command, file, screenshot, timing) next to the number.

| # | Axis | What to measure in the PoC |
|---|---|---|
| 1 | **Smoothness** | Scroll a 5,000-line document; type fast; resize the window. Record fps / frame-time feel, jank on scroll, and whether the app holds 60 fps (120 fps where claimed). |
| 2 | **Live-preview ease** | Time/LOC to get: bold/italic/headings rendering inline, syntax hidden off-cursor, checkbox click-to-toggle, code-fence rendering. Note what was built-in vs hand-built. |
| 3 | **Japanese IME** | Scripted test on Windows *and* Linux: type kana, convert with Space, confirm with Enter, Backspace mid-composition, candidate-window position, IME on/off switching. Pass/fail per case. |
| 4 | **Proportional font / layout** | Mixed proportional + monospace in one document; heading at 1.5× size with stable line height; variable line spacing; custom sidebar/panel layout. Note workarounds needed. |
| 5 | **Visual polish** | How close to an Obsidian-class look can the PoC get in the timebox: dark theme, selection/caret rendering, hover states, window chrome, HiDPI. |
| 6 | **Build / startup** | Clean-build time, incremental-build time, binary/installer size, cold-start to first editable window, Linux (WSL) build friction (extra apt packages, failures). |
| 7 | **Maturity / docs** | Could the PoC be built from docs alone, or did it need source-diving / issues / Discord? Count blockers and their resolution time. |
| 8 | **License risk** | 1 = would force a licence change or legal review; 5 = permissive throughout with a clean dependency closure. Record the actual `cargo tree` / `npm ls` licence inventory. |

### Rubric (per axis)

| Score | Meaning |
|---|---|
| **5 — Excellent** | Meets the Mica bar out of the box; no workarounds; evidence recorded. |
| **4 — Good** | Works well with minor, documented workarounds. |
| **3 — Adequate** | Works, but with visible compromises, extra code, or rough edges the user would notice. |
| **2 — Poor** | Barely works, needs a substantial hand-built subsystem, or has reproducible bugs on a target platform. |
| **1 — Blocker** | Fails the axis, or would force a licence/architecture change. |

### Preliminary scores (from research only — to be replaced by PoC measurements)

| Axis | Tauri + CM6 | GPUI | egui |
|---|---|---|---|
| 1 Smoothness | 4 | 5 | 4 |
| 2 Live-preview ease | 5 | 1 | 2 |
| 3 Japanese IME | 4* | 2 | 2 |
| 4 Proportional font / layout | 5 | 4 | 3 |
| 5 Visual polish | 5 | 4 | 3 |
| 6 Build / startup | 3 | 3 | 5 |
| 7 Maturity / docs | 5 | 2 | 4 |
| 8 License risk | 5 | 4† | 5 |
| **Total /40 (preliminary)** | **36** | **25** | **28** |

\* Tauri IME is 4, not 5, only because the Linux/WebKitGTK IME path is untested in this research — the PoC must confirm it.
† GPUI is 4, not 5, because the framework is Apache-2.0 but the adjacent GPL editor crates are a live foot-gun: one wrong dependency makes Mica GPL. The risk is procedural, not legal, if the boundary in §8 is respected.

**Reading the table:** Tauri wins on the axes that are *about being an editor* (live preview, fonts, polish, docs); egui wins on build/startup and is the credible pure-Rust second; GPUI wins only on raw smoothness, and pays for it on every axis that involves actually having an editor.

---

## 11. PoC recommendations (for the coordinator)

1. **Tauri + CodeMirror 6 (first):** pin `tauri = "=2.12.5"`, `@codemirror/view = "=6.43.15"`, `@codemirror/state = "=6.7.6"`; build the Live Preview decoration layer (~600 LOC target) with cursor-aware reveal; run the scripted Japanese IME test on Windows and on Linux/WebKitGTK; measure scroll fps on a 5,000-line file.
2. **egui (second):** pin `eframe = "=0.37.0"`; build a `LayoutJob`-based read view + `TextEdit` source view and attempt one inline styled paragraph; run the *same* scripted IME test (esp. #7908 Backspace-during-composition and the IBus/Wayland case) — this test alone may decide egui's fate.
3. **GPUI (third, timeboxed):** pin an exact source — either official `gpui = "=0.2.2"` (stale, honest baseline) or `gpui-pre = "=0.3.12"` (current, third-party) — record the choice; build one window with a shaped multi-line text view and a working `EntityInputHandler` single-line input; **do not** import or copy anything from `crates/editor`, `crates/text`, `crates/rope`, `crates/ui` or any other GPL crate; timebox strictly, because the deliverable is a *cost measurement*, not an editor.
4. **Record for every PoC:** exact versions, the Linux `apt` list actually needed, clean/incremental build times, cold-start time, and the IME test sheet. Those four artefacts are the comparison — not opinions.

---

## 12. Sources (checked 2026-11-15)

- Tauri releases: https://tauri.app/release/core/
- Tauri 2.12 announcement (26 Sep 2026): https://v2.tauri.app/blog/tauri-2.12/
- Tauri 2.12.5 notes / Tauri-vs-Electron 2026: https://dev-news.anonyfox.com/tauri/ ; https://dawloom.com/blog/tauri-or-electron-in-2026/
- Tauri prerequisites: https://github.com/stuffbucket/skills/blob/HEAD/plugins/stuffbucket/skills/tauri-setup-prerequisites/SKILL.md ; https://github.com/anwardebes/vsclaude/blob/HEAD/specs/BUILD_AND_DISTRIBUTION.md
- CodeMirror changelog (official): https://codemirror.net/docs/changelog/
- CodeMirror Live Preview pattern: https://deepwiki.com/fryholic/sticker-md/3.3.1-live-preview ; https://github.com/bejranonda/externalbrain/blob/HEAD/research/obsidian/architecture.md ; https://github.com/nodummd/nodum/blob/HEAD/docs/research/DECISIONS.md
- CodeMirror IME verification: https://github.com/bentlybro/kern/blob/HEAD/docs/research/verifications.md
- GPUI deep dive (Jul 2026): https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/HEAD/report/03-gpui.md
- GPUI ecosystem map / AccessKit / taffy: https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/HEAD/report/00-ecosystem-map.md ; https://goldstrikearch.github.io/rust-gui-desktop-ecosystem-state/
- GPUI in practice (Oct 2026): https://baeseokjae.github.io/posts/native-app-for-coding-agents-rust-gpui/ ; https://www.shuffleapp.co/blog/building-shuffle-rust-gpui ; https://github.com/appsicle/daisynotes/blob/HEAD/PLAN.md
- GPUI crates: https://index.githits.dev/packages/crates/gpui-pre/ ; https://crates.io/crates/gpui-unofficial/range/%5E1.14.2 ; https://crates.io/crates/tauri-plugin-gpui
- Zed releases / licensing: https://dev-news.anonyfox.com/zed/ ; https://github.com/jonx/macaros/blob/HEAD/docs/features/zed-editor/README.md ; https://www.promptquorum.com/power-local-llm/zed-review ; https://cloudparts.dev/technologies/zed/
- egui/eframe changelog (official): https://github.com/emilk/egui/blob/HEAD/crates/eframe/CHANGELOG.md
- epaint changelog: https://github.com/emilk/egui/blob/main/crates/epaint/CHANGELOG.md
- egui docs / versions: https://docs.rs/crate/egui/latest ; https://rusttutorial.org/gui-apps/
- egui IME issues: https://github.com/emilk/egui/issues/7908 ; https://github.com/emilk/egui/issues/7485 ; https://deepwiki.com/membrane-io/egui/3.5-text-input-and-editing
- egui CJK typography note: https://github.com/miaokela/tinyterm-egui/blob/HEAD/skills/tinyterm-egui-dev/references/theme.md
- Licenses (Tauri/CodeMirror inventory): https://docs.typeward.com/reference/license/ ; https://github.com/aryrabelo/tapa/blob/HEAD/THIRD-PARTY-LICENSES.md ; https://github.com/confeden/nova-prism/blob/HEAD/docs/research/06-gui-framework.md
- Alternatives: https://gpui-starter.freeoxide.com/blog/rust-gui-frameworks-comparison-2026/ ; https://github.com/janwirth/ecosystem_review/blob/HEAD/application-types/desktop.md

---

## 日本語要約 (Short Japanese summary)

- **Tauri 2 (2.12.5, 2026-10-21) + CodeMirror 6 (`@codemirror/view` 6.43.15) が第一候補。** ObsidianのLive Previewと同じ仕組み（Lezer + Decoration）で作れて、日本語IME・プロポーショナルフォント・可変行高はブラウザ/CSSに任せられる。弱点はWebView依存とLinuxの `webkit2gtk-4.1-dev` など。
- **egui/eframe 0.37.0 (2026-10-15) が純Rustの次点。** ビルドが最も軽く `TextEdit` もあるが、Markdownエディタは自作が必要。日本語IMEは未解決の不具合あり（#7908 変換中のBackspace誤削除、#7485 IBus/Waylandで1文字しか打てない）。PoCで実機テスト必須。
- **GPUIは最速だが最も危険。** Zed (1.28.1) が120fpsで実証済みだが、公式crates.ioは **0.2.2 (2025-10-22) のまま約13か月更新なし**。最新版はgit依存か第三者再配布（`gpui-pre` 0.3.12等）。エディタ・ウィジェットは自作。**Zedの editor crate はGPL-3.0のため、コピー・依存するとMicaもGPLになる。GPUI本体はApache-2.0で安全。設計だけ参考にし、コードはコピーしないこと。**
- 暫定順位: **1位 Tauri+CodeMirror 6、2位 egui、3位 GPUI**。PoCではIMEテスト（Windows/Linux）、5000行スクロール、ビルド/起動時間を必ず計測すること。
