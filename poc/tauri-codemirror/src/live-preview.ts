/**
 * Live preview for Mica (Tauri + CodeMirror 6 PoC).
 *
 * This is the Obsidian-style live-preview layer: the buffer always holds
 * raw Markdown, and this module only changes how lines are *shown*.
 *
 * How it works (CodeMirror 6 decoration pattern, see
 * docs/research-2026-11.md §4.1):
 *
 * 1. The Lezer Markdown parser (`@lezer/markdown`, via
 *    `@codemirror/lang-markdown`) builds a syntax tree incrementally
 *    as the user types. We never parse Markdown ourselves here.
 *    The parser base is the GFM language (`markdownLanguage`: task
 *    lists, strikethrough, tables, autolinks). Note that `markdown()`
 *    on its own defaults to strict CommonMark, which parses `- [x]`
 *    as a shortcut link, not a task — so the base must be passed
 *    explicitly (see `livePreview` below).
 * 2. A `StateField` walks that tree on every document or selection
 *    change and computes a `RangeSet<Decoration>`:
 *    - `Decoration.mark` — CSS classes for bold / italic / strikethrough /
 *      inline code / links (proportional sizes come from CSS).
 *    - `Decoration.line` — a CSS class for the whole line, used for
 *      heading sizes and blockquote styling.
 *    - `Decoration.replace` — hides syntax markers (`##`, `**`, `` ` ``,
 *      `>`, link URLs, …) on every line *except* the cursor line, and
 *      replaces task markers (`- [ ]`) and horizontal rules with
 *      widgets.
 * 3. Row-level cursor rule (the PoC rule from the Notion child page):
 *    markers are shown on the cursor line and hidden everywhere else.
 *    The cursor line is the line containing the selection head.
 *
 * Japanese IME safety: CodeMirror's composition handling (EditContext /
 * contenteditable) owns the composing text, and composition always
 * happens at the cursor — which is exactly the line where this layer
 * applies *no* hiding or replacement. No decoration ever overlaps the
 * composition range, so recomputing decorations cannot flicker or
 * corrupt composing text. See `scripts/composition-roundtrip.mjs` for
 * the headless round-trip check and the README for the manual
 * Windows IME test item.
 *
 * What this layer covers:
 * - ATX headings (`#`–`######`) and setext headings (`===` / `---`)
 * - bold (`**…**`), italic (`*…*`), strikethrough (`~~…~~`)
 * - inline code (`` `…` ``) and fenced code blocks
 * - links (`[text](url)`) — URL hidden, text styled
 * - task list checkboxes (`- [ ]` / `- [x]`) as clickable widgets
 * - block quotes (`> …`), horizontal rules (`---`)
 *
 * Out of PoC scope (documented, not silently missing): tables, math,
 * images (shown as a styled link, not rendered inline), footnotes,
 * nested emphasis, wiki links (`[[…]]`), and multi-line constructs
 * beyond fenced code. See README.md.
 */

import { syntaxTree } from "@codemirror/language";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import {
  type EditorState,
  RangeSet,
  RangeSetBuilder,
  StateField,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  WidgetType,
  type DecorationSet,
} from "@codemirror/view";

/** CSS classes emitted for each heading level (1–6). */
const HEADING_LINE_CLASSES = [
  "",
  "cm-mica-heading-1",
  "cm-mica-heading-2",
  "cm-mica-heading-3",
  "cm-mica-heading-4",
  "cm-mica-heading-5",
  "cm-mica-heading-6",
] as const;

/** One decoration waiting to be added to the RangeSetBuilder. */
interface PendingDecoration {
  from: number;
  to: number;
  value: Decoration;
}

/** A checkbox widget replacing a `- [ ]` / `- [x]` marker. */
export class CheckboxWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    /** Document offset of the `[` of the task marker. */
    readonly markerFrom: number,
  ) {
    super();
  }

  eq(other: CheckboxWidget): boolean {
    return (
      other.checked === this.checked &&
      other.markerFrom === this.markerFrom
    );
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className =
      "cm-mica-checkbox" + (this.checked ? " checked" : "");
    span.textContent = this.checked ? "✓" : "";
    span.dataset.markerFrom = String(this.markerFrom);
    span.setAttribute("role", "checkbox");
    span.setAttribute("aria-checked", String(this.checked));
    span.title = this.checked
      ? "Click to mark as not done"
      : "Click to mark as done";
    return span;
  }
}

/** A horizontal-rule widget replacing a `---` line. */
export class HorizontalRuleWidget extends WidgetType {
  eq(other: HorizontalRuleWidget): boolean {
    return other instanceof HorizontalRuleWidget;
  }

  toDOM(): HTMLElement {
    const rule = document.createElement("hr");
    rule.className = "cm-mica-horizontal-rule";
    return rule;
  }
}

/**
 * Toggle the checkbox whose marker starts at `markerFrom`.
 * Returns `false` when there is no task marker at that offset.
 * The marker text (`[ ]` / `[x]`) is the only thing changed; the
 * rest of the line — and the file — keeps its raw Markdown.
 */
export function toggleTaskCheckboxAt(
  view: EditorView,
  markerFrom: number,
): boolean {
  const line = view.state.doc.lineAt(markerFrom);
  const offsetInLine = markerFrom - line.from;
  const match = /\[([ xX])\]/.exec(line.text.slice(offsetInLine));
  if (match === null || match.index !== 0) {
    return false;
  }
  const wasChecked = match[1] !== " ";
  const checkPosition = markerFrom + 1;
  view.dispatch({
    changes: {
      from: checkPosition,
      to: checkPosition + 1,
      insert: wasChecked ? " " : "x",
    },
  });
  return true;
}

/**
 * Compute the live-preview decorations for one editor state.
 *
 * Decorations are collected first and then sorted by position before
 * being added to the RangeSetBuilder, because the tree walk visits
 * parent nodes before their children and a parent's decoration can
 * start *after* a child's (for example the bold content mark starts
 * one character after the opening `**`, which the child marker
 * decoration covers).
 */
export function computeLivePreviewDecorations(
  state: EditorState,
): DecorationSet {
  const doc = state.doc;

  // Lines containing a selection head: markers stay visible there.
  const cursorLines = new Set<number>();
  for (const range of state.selection.ranges) {
    cursorLines.add(doc.lineAt(range.head).number);
  }
  const isCursorLine = (position: number): boolean =>
    cursorLines.has(doc.lineAt(position).number);

  const pending: PendingDecoration[] = [];
  // Line number -> CSS classes for that whole line (merged so each
  // line gets a single Decoration.line).
  const lineClasses = new Map<number, string[]>();

  const addLineClass = (lineNumber: number, className: string): void => {
    const classes = lineClasses.get(lineNumber) ?? [];
    if (!classes.includes(className)) {
      classes.push(className);
    }
    lineClasses.set(lineNumber, classes);
  };

  const hideRange = (from: number, to: number): void => {
    if (to > from) {
      pending.push({ from, to, value: Decoration.replace({}) });
    }
  };

  const markRange = (from: number, to: number, className: string): void => {
    if (to > from) {
      pending.push({
        from,
        to,
        value: Decoration.mark({ attributes: { class: className } }),
      });
    }
  };

  /** Count the run of `character` at `position` in the document. */
  const countRun = (position: number, character: string): number => {
    let count = 0;
    while (doc.sliceString(position + count, position + count + 1) === character) {
      count += 1;
    }
    return count;
  };

  const tree = syntaxTree(state);
  tree.iterate({
    enter(node): void {
      const name = node.type.name;
      const from = node.from;
      const to = node.to;
      const onCursorLine = isCursorLine(from);

      switch (name) {
        // --- Headings -------------------------------------------------
        case "ATXHeading1":
        case "ATXHeading2":
        case "ATXHeading3":
        case "ATXHeading4":
        case "ATXHeading5":
        case "ATXHeading6": {
          if (!onCursorLine) {
            const level = Number(name.slice("ATXHeading".length));
            addLineClass(
              doc.lineAt(from).number,
              HEADING_LINE_CLASSES[level] ?? "cm-mica-heading-6",
            );
          }
          break;
        }
        case "SetextHeading": {
          if (!onCursorLine) {
            const firstLine = doc.lineAt(from);
            // `===` underlines are level 1, `---` underlines level 2.
            const underline = doc.sliceString(to - 1, to);
            addLineClass(
              firstLine.number,
              underline === "="
                ? "cm-mica-heading-1"
                : "cm-mica-heading-2",
            );
            // Hide the underline line itself (it is pure syntax).
            const underlineLine = doc.lineAt(to - 1);
            if (underlineLine.number !== firstLine.number) {
              hideRange(underlineLine.from, underlineLine.to);
            }
          }
          break;
        }
        case "HeaderMark": {
          if (!onCursorLine) {
            // Hide the `#`s and the spaces after them, so the heading
            // text starts at the left edge of the body column.
            const line = doc.lineAt(from);
            let end = to;
            while (
              end < line.to &&
              doc.sliceString(end, end + 1) === " "
            ) {
              end += 1;
            }
            hideRange(from, end);
          }
          break;
        }

        // --- Emphasis -------------------------------------------------
        case "Emphasis": {
          if (!onCursorLine) {
            // Single `*` or `_` markers, one character each side.
            markRange(from + 1, to - 1, "cm-mica-emphasis");
          }
          break;
        }
        case "StrongEmphasis": {
          if (!onCursorLine) {
            // Double `**` markers, two characters each side.
            markRange(from + 2, to - 2, "cm-mica-strong");
          }
          break;
        }
        case "Strikethrough": {
          if (!onCursorLine) {
            // Double `~~` markers, two characters each side.
            markRange(from + 2, to - 2, "cm-mica-strikethrough");
          }
          break;
        }
        case "EmphasisMark":
        case "StrikethroughMark": {
          // The marker characters themselves (`*`, `**`, `~~`).
          if (!onCursorLine) {
            hideRange(from, to);
          }
          break;
        }

        // --- Code -----------------------------------------------------
        case "InlineCode": {
          if (!onCursorLine) {
            // Backtick runs can be longer than one character
            // (`` ``code`` ``), so count them instead of assuming.
            const ticks = Math.max(countRun(from, "`"), 1);
            markRange(from + ticks, to - ticks, "cm-mica-code");
          }
          break;
        }
        case "CodeMark": {
          // Backticks of inline code and the ``` fences.
          if (!onCursorLine) {
            hideRange(from, to);
          }
          break;
        }
        case "CodeInfo": {
          // The language name after an opening ``` fence.
          if (!onCursorLine) {
            hideRange(from, to);
          }
          break;
        }
        case "CodeText": {
          if (!onCursorLine) {
            markRange(from, to, "cm-mica-code-block");
          }
          break;
        }

        // --- Links and images -----------------------------------------
        case "Link":
        case "Image": {
          if (onCursorLine) {
            break;
          }
          // Work from the raw text: `[text](url)`.
          const text = doc.sliceString(from, to);
          const openBracket = text.indexOf("[");
          const closeParen = text.indexOf("](");
          if (openBracket >= 0 && closeParen > openBracket) {
            markRange(
              from + openBracket + 1,
              from + closeParen,
              "cm-mica-link",
            );
            // Hide the opening `[` (and `!` for images) …
            hideRange(from, from + openBracket + 1);
            // … and the `](url)` tail.
            hideRange(from + closeParen, to);
          }
          break;
        }

        // --- Lists ----------------------------------------------------
        case "ListMark": {
          // Bullet / number markers stay visible (they are content,
          // not hidden syntax) but are shown muted.
          markRange(from, to, "cm-mica-list-marker");
          break;
        }
        case "TaskMarker": {
          if (onCursorLine) {
            break;
          }
          const text = doc.sliceString(from, to);
          const checked = text.includes("x") || text.includes("X");
          pending.push({
            from,
            to,
            value: Decoration.replace({
              widget: new CheckboxWidget(checked, from),
            }),
          });
          break;
        }

        // --- Quotes and rules -----------------------------------------
        case "Blockquote": {
          if (!onCursorLine) {
            const firstLine = doc.lineAt(from).number;
            const lastLine = doc.lineAt(Math.max(to - 1, from)).number;
            for (let line = firstLine; line <= lastLine; line += 1) {
              addLineClass(line, "cm-mica-quote");
            }
          }
          break;
        }
        case "QuoteMark": {
          if (!onCursorLine) {
            hideRange(from, to);
          }
          break;
        }
        case "HorizontalRule": {
          if (onCursorLine) {
            break;
          }
          pending.push({
            from,
            to,
            value: Decoration.replace({
              widget: new HorizontalRuleWidget(),
            }),
          });
          break;
        }

        default:
          break;
      }
    },
  });

  // Merge the per-line classes into one line decoration per line.
  for (const [lineNumber, classes] of lineClasses) {
    const line = doc.line(lineNumber);
    pending.push({
      from: line.from,
      to: line.from,
      value: Decoration.line({
        attributes: { class: classes.join(" ") },
      }),
    });
  }

  // RangeSetBuilder requires ranges in document order.
  pending.sort((left, right) => left.from - right.from || left.to - right.to);
  const builder = new RangeSetBuilder<Decoration>();
  for (const decoration of pending) {
    builder.add(decoration.from, decoration.to, decoration.value);
  }
  return builder.finish();
}

/**
 * The decoration field. It recomputes only when the document or the
 * selection changes; cursor movement alone (without a selection
 * change) is covered because moving the cursor changes the selection.
 */
export const livePreviewField = StateField.define<DecorationSet>({
  create: (state) => computeLivePreviewDecorations(state),
  update: (decorations, transaction) =>
    transaction.docChanged || transaction.selection
      ? computeLivePreviewDecorations(transaction.state)
      : decorations,
  provide: (field) => EditorView.decorations.from(field),
});

/**
 * Keep the cursor out of replaced widgets (checkboxes, rules):
 * atomic ranges make left/right movement skip over them instead of
 * stopping on invisible characters (Notion child page, hard-parts
 * note on cursor movement across hidden markers).
 */
const livePreviewAtomicRanges = EditorView.atomicRanges.of((view) => {
  const decorations = view.state.field(livePreviewField, false);
  if (decorations === undefined) {
    return RangeSet.empty;
  }
  const builder = new RangeSetBuilder<Decoration>();
  const iterator = decorations.iter();
  while (iterator.value !== null) {
    if (iterator.value.spec.widget !== undefined) {
      // The value is never rendered; only the range matters.
      builder.add(iterator.from, iterator.to, Decoration.mark({}));
    }
    iterator.next();
  }
  return builder.finish();
});

/** The full live-preview extension: Markdown (GFM base) + decorations. */
export const livePreview = [
  // `markdown()` alone defaults to strict CommonMark; pass the GFM
  // language as the base so `- [ ]` parses as a Task, not a link.
  markdown({ base: markdownLanguage }),
  livePreviewField,
  livePreviewAtomicRanges,
];
