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
} finally {
  fs.rmSync(bundlePath, { force: true });
}

if (failures > 0) {
  console.log(`\n${failures} check(s) failed.`);
  process.exit(1);
}
console.log("\nAll live-preview DOM checks passed.");
