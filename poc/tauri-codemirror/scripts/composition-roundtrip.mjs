/**
 * Headless Japanese IME round-trip check for the Tauri + CodeMirror 6 PoC.
 *
 * Run: `npm run test:ime` (needs `npm install` first).
 *
 * What it checks, without a display: that Japanese text typed through
 * CodeMirror's document model — the same `EditorState` transactions
 * the editor applies for composition input — round-trips without
 * corruption. It simulates the keystrokes an IME produces:
 *
 * 1. Type romaji that becomes hiragana during composition
 *    ("konnichiha" -> "こんにちは").
 * 2. Convert with Space and confirm with Enter
 *    ("こんにちは" -> "今日は").
 * 3. Backspace mid-composition deletes exactly one character.
 * 4. Mixed Japanese / Markdown markers survive edits at multi-byte
 *    boundaries (no U+FFFD replacement characters, offsets stay in
 *    characters, not bytes).
 *
 * What it does NOT check (recorded as a manual test item in the
 * README): the real WebKitGTK / WebView2 candidate window, its
 * position, or flicker during live composition. Those need a human
 * on Windows (and on Linux with IBus/Fcitx). The decoration layer is
 * designed so composition is safe — see src/live-preview.ts — but
 * only a live IME session proves it.
 */

import { EditorState } from "@codemirror/state";

let failures = 0;

function check(name, actual, expected) {
  if (actual === expected) {
    console.log(`ok   ${name}`);
  } else {
    failures += 1;
    console.log(`FAIL ${name}\n  expected: ${JSON.stringify(expected)}\n  actual:   ${JSON.stringify(actual)}`);
  }
}

function stateWith(doc) {
  return EditorState.create({ doc });
}

/** Apply an insertion as the editor would for committed IME text. */
function insert(state, from, to, text) {
  return state.update({ changes: { from, to, insert: text } }).state;
}

/** Apply a deletion of `length` characters ending at `to`. */
function deleteBackward(state, to, length) {
  return state.update({ changes: { from: to - length, to, insert: "" } }).state;
}

// 1. Composition: romaji becomes hiragana, then more text is typed.
let state = stateWith("# メモ\n\n");
state = insert(state, 6, 6, "こ");
state = insert(state, 7, 7, "んにちは");
check("composition hiragana accumulates", state.doc.toString(), "# メモ\n\nこんにちは");

// 2. Conversion + confirmation replaces the composing range.
state = insert(state, 6, 11, "今日は");
check("conversion replaces composing range", state.doc.toString(), "# メモ\n\n今日は");
check("no replacement characters after conversion", state.doc.toString().includes("�"), false);

// 3. Backspace mid-composition deletes exactly one character.
state = insert(state, 9, 9, "です");
state = deleteBackward(state, 11, 1);
check("backspace mid-composition deletes one char", state.doc.toString(), "# メモ\n\n今日はで");

// 4. Editing at multi-byte boundaries: markers and Japanese text mix.
state = stateWith("これは**太字**と`コード`です。\n");
state = insert(state, 3, 3, "、テスト");
check("insert inside Japanese sentence", state.doc.toString(), "これは、テスト**太字**と`コード`です。\n");
state = insert(state, 0, 0, "## 見出し\n\n");
check("heading insert keeps Japanese intact", state.doc.sliceString(0, 6), "## 見出し");
check("document length is in characters, not bytes", state.doc.length, "## 見出し\n\nこれは、テスト**太字**と`コード`です。\n".length);
check("no replacement characters in mixed doc", state.doc.toString().includes("�"), false);

// 5. A long Japanese paragraph survives many small edits.
state = stateWith("");
const sentence = "日本語の入力が壊れないことを確認します。";
for (let i = 0; i < 20; i += 1) {
  state = insert(state, state.doc.length, state.doc.length, sentence + "\n");
}
check(
  "twenty Japanese paragraphs round-trip",
  state.doc.toString(),
  sentence.repeat(0) + (sentence + "\n").repeat(20),
);
check("line count after repeated inserts", state.doc.lines, 21);

if (failures > 0) {
  console.log(`\n${failures} check(s) failed.`);
  process.exit(1);
}
console.log("\nAll IME round-trip checks passed.");
