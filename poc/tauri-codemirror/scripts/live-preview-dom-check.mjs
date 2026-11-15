/**
 * Headless live-preview DOM check for the Tauri + CodeMirror 6 PoC.
 *
 * Run: `npm run test:dom` (needs `npm install` first).
 *
 * Renders the real editor (`EditorView` + the `livePreview` extension
 * from src/live-preview.ts) in jsdom and asserts on the resulting DOM.
 * This catches decoration bugs that a pure `EditorState` check cannot
 * — for example, this check caught that `markdown()` defaults to
 * strict CommonMark, where `- [x]` parses as a shortcut link instead
 * of a task (the fix: `markdown({ base: markdownLanguage })`).
 *
 * The flow replicates the app exactly: the view is created with an
 * empty document (as in src/main.ts) and the file content is swapped
 * in with the same dispatch `focusTab` uses. Assertions:
 *
 * - 2 checkbox widgets (one per task line in sample-notes/welcome.md)
 * - 1 horizontal-rule widget
 * - the `## Live preview` heading line carries its heading class
 *   and the `##` marker text is not in the DOM
 * - the cursor line (line 1) still shows its raw `#` marker
 * - multi-line constructs reaching the cursor line (a link and a bold
 *   span split over two lines with the cursor on the second line, a
 *   fenced code block with the cursor on a middle line) are shown raw
 *   in full and leave the cursor line undecorated; each has a control
 *   run with the cursor elsewhere proving the markers do get hidden
 * - auto-save race (src/auto-save.ts): an edit made while a save is
 *   in flight keeps the tab dirty, so the newer text is saved next;
 *   a failed write keeps it dirty too (pure node, no DOM needed — it
 *   lives here so it runs with the existing headless checks)
 *
 * jsdom is not a browser: layout is all zeros, so this checks DOM
 * structure, not pixels. The Xvfb screenshot in the README is the
 * visual counterpart.
 */

import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";

const projectRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const bundlePath = path.join(projectRoot, ".tmp-live-preview-bundle.mjs");
const autoSaveBundlePath = path.join(projectRoot, ".tmp-auto-save-bundle.mjs");

let failures = 0;
function check(name, actual, expected) {
  if (actual === expected) {
    console.log(`ok   ${name}`);
  } else {
    failures += 1;
    console.log(
      `FAIL ${name}\n  expected: ${JSON.stringify(expected)}\n  actual:   ${JSON.stringify(actual)}`,
    );
  }
}

try {
  // Bundle the TypeScript decoration layer (esbuild ships with Vite).
  execFileSync(
    path.join(projectRoot, "node_modules", ".bin", "esbuild"),
    [
      path.join(projectRoot, "src", "live-preview.ts"),
      "--bundle",
      "--format=esm",
      `--outfile=${bundlePath}`,
      "--log-level=error",
      "--external:@codemirror/*",
      "--external:@lezer/*",
    ],
    { stdio: "inherit" },
  );

  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    pretendToBeVisual: true,
  });
  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  Object.defineProperty(globalThis, "navigator", {
    value: dom.window.navigator,
  });
  globalThis.MutationObserver = dom.window.MutationObserver;
  globalThis.ResizeObserver = dom.window.ResizeObserver;
  globalThis.requestAnimationFrame = (callback) => setTimeout(callback, 0);
  globalThis.cancelAnimationFrame = (id) => clearTimeout(id);
  globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);

  const livePreviewModule = await import(bundlePath);
  const { EditorState } = await import("@codemirror/state");
  const { EditorView, lineNumbers } = await import("@codemirror/view");
  const { defaultKeymap, history, historyKeymap } = await import(
    "@codemirror/commands"
  );
  const { keymap: editorKeymap } = await import("@codemirror/view");

  const sampleDoc = fs.readFileSync(
    path.join(projectRoot, "..", "..", "sample-notes", "welcome.md"),
    "utf8",
  );
  const host = document.createElement("div");
  document.body.appendChild(host);
  const view = new EditorView({
    parent: host,
    state: EditorState.create({
      doc: "",
      extensions: [
        lineNumbers(),
        history(),
        editorKeymap.of([...defaultKeymap, ...historyKeymap]),
        EditorView.lineWrapping,
        ...livePreviewModule.livePreview,
      ],
    }),
  });
  // Same dispatch as focusTab in src/main.ts.
  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: sampleDoc },
    selection: { anchor: 0 },
  });

  const checkboxes = view.dom.querySelectorAll(".cm-mica-checkbox");
  check("checkbox widgets rendered", checkboxes.length, 2);
  check(
    "checked box marked checked",
    view.dom.querySelectorAll(".cm-mica-checkbox.checked").length,
    1,
  );
  check(
    "horizontal-rule widget rendered",
    view.dom.querySelectorAll(".cm-mica-horizontal-rule").length,
    1,
  );
  const headingLine = view.dom.querySelector(".cm-mica-heading-2");
  check("heading line has heading class", headingLine !== null, true);
  check(
    "heading marker hidden off-cursor",
    headingLine?.textContent?.includes("##") ?? true,
    false,
  );
  check(
    "heading text kept",
    headingLine?.textContent?.includes("Live preview") ?? false,
    true,
  );
  const firstLine = view.dom.querySelector(".cm-line");
  check(
    "cursor line shows its raw marker",
    firstLine?.textContent?.startsWith("# Welcome to Mica") ?? false,
    true,
  );
  check(
    "bold text marked",
    view.dom.querySelectorAll(".cm-mica-strong").length > 0,
    true,
  );
  check(
    "inline code marked",
    view.dom.querySelectorAll(".cm-mica-code").length > 0,
    true,
  );
  view.destroy();

  // --- Multi-line constructs reaching the cursor line ----------------
  // A construct that starts above the cursor line but reaches it must
  // be shown raw in full, and the cursor line must carry no decoration
  // from the live-preview layer (that is the composition-safety rule).
  // Each case has a control run with the cursor on line 1, proving
  // the same markers *are* hidden when the cursor is elsewhere.

  /** Render `doc` with the cursor at the start of line `cursorLine`. */
  function renderAt(doc, cursorLine) {
    const caseHost = document.createElement("div");
    document.body.appendChild(caseHost);
    const caseView = new EditorView({
      parent: caseHost,
      state: EditorState.create({
        doc,
        extensions: [...livePreviewModule.livePreview],
      }),
    });
    caseView.dispatch({
      selection: { anchor: caseView.state.doc.line(cursorLine).from },
    });
    return caseView;
  }
  /** Rendered text of 1-based line `number` (all lines fit in jsdom). */
  function lineText(caseView, number) {
    return caseView.dom.querySelectorAll(".cm-line")[number - 1]?.textContent;
  }
  /** True when line `number` carries no live-preview decoration. */
  function lineUndecorated(caseView, number) {
    const lineElement = caseView.dom.querySelectorAll(".cm-line")[number - 1];
    if (lineElement === undefined) {
      return false;
    }
    const decorated = (node) =>
      [...node.classList].some((name) => name.startsWith("cm-mica"));
    return (
      !decorated(lineElement) &&
      [...lineElement.querySelectorAll("*")].every((node) => !decorated(node)) &&
      lineElement.textContent === caseView.state.doc.line(number).text
    );
  }

  // Link split over two lines, cursor on the second line.
  const linkDoc = "Intro line\n\nSee [split\nlink](http://x) here.\n\nEnd.";
  let caseView = renderAt(linkDoc, 4);
  check(
    "split link: URL tail visible on cursor line",
    lineText(caseView, 4)?.includes("](http://x)") ?? false,
    true,
  );
  check(
    "split link: opening [ visible on line above",
    lineText(caseView, 3)?.includes("[split") ?? false,
    true,
  );
  check("split link: cursor line undecorated", lineUndecorated(caseView, 4), true);
  caseView.destroy();
  caseView = renderAt(linkDoc, 1);
  check(
    "split link control: URL hidden off-cursor",
    lineText(caseView, 4)?.includes("](http") ?? true,
    false,
  );
  caseView.destroy();

  // Bold split over two lines, cursor on the second line.
  const boldDoc = "Intro line\n\nSome **bold\ntext** here.\n\nEnd.";
  caseView = renderAt(boldDoc, 4);
  check(
    "split bold: closing ** visible on cursor line",
    lineText(caseView, 4)?.includes("**") ?? false,
    true,
  );
  check("split bold: cursor line undecorated", lineUndecorated(caseView, 4), true);
  caseView.destroy();
  caseView = renderAt(boldDoc, 1);
  check(
    "split bold control: ** hidden off-cursor",
    lineText(caseView, 4)?.includes("**") ?? true,
    false,
  );
  caseView.destroy();

  // Fenced code block, cursor on a middle line.
  const fenceDoc =
    "Intro line\n\n```js\nconst a = 1;\nconst b = 2;\n```\n\nEnd.";
  caseView = renderAt(fenceDoc, 4);
  check("fenced code: opening fence visible", lineText(caseView, 3), "```js");
  check("fenced code: closing fence visible", lineText(caseView, 6), "```");
  check("fenced code: cursor line undecorated", lineUndecorated(caseView, 4), true);
  caseView.destroy();
  caseView = renderAt(fenceDoc, 1);
  check("fenced code control: fence hidden off-cursor", lineText(caseView, 3), "");
  check(
    "fenced code control: code styled off-cursor",
    caseView.dom.querySelectorAll(".cm-mica-code-block").length > 0,
    true,
  );
  caseView.destroy();

  // --- Auto-save race (src/auto-save.ts) -----------------------------
  // An edit lands while the write is in flight: the tab must stay
  // dirty so the restarted timer saves the newer text.
  execFileSync(
    path.join(projectRoot, "node_modules", ".bin", "esbuild"),
    [
      path.join(projectRoot, "src", "auto-save.ts"),
      "--bundle",
      "--format=esm",
      `--outfile=${autoSaveBundlePath}`,
      "--log-level=error",
    ],
    { stdio: "inherit" },
  );
  const { saveTabContent } = await import(autoSaveBundlePath);
  const written = [];
  const tab = { path: "/notes/a.md", content: "first", dirty: true };
  const outcome = await saveTabContent(tab, async (filePath, content) => {
    written.push(content);
    // The user types while the backend write is still running.
    tab.content = "first + newer";
    tab.dirty = true;
  });
  check(
    "auto-save: edit during save reported as changed",
    outcome.kind,
    "changed",
  );
  check("auto-save: edit during save keeps tab dirty", tab.dirty, true);
  const second = await saveTabContent(tab, async (filePath, content) => {
    written.push(content);
  });
  check("auto-save: follow-up save writes newer text", written.join(" | "), "first | first + newer");
  check(
    "auto-save: follow-up save leaves tab clean",
    `${second.kind}/${tab.dirty}`,
    "saved/false",
  );
  const failing = { path: "/notes/b.md", content: "x", dirty: true };
  const failed = await saveTabContent(failing, async () => {
    throw new Error("disk full");
  });
  check(
    "auto-save: failed write keeps tab dirty",
    `${failed.kind}/${failing.dirty}`,
    "failed/true",
  );
} finally {
  fs.rmSync(bundlePath, { force: true });
  fs.rmSync(autoSaveBundlePath, { force: true });
}

if (failures > 0) {
  console.log(`\n${failures} check(s) failed.`);
  process.exit(1);
}
console.log("\nAll live-preview DOM checks passed.");
