# List Formatting — Implementation Plan

## Overview

Three formatting actions on the editor's current selection or line:

1. **Ordered list** — each selected line becomes `1.`, `2.`, `3.` …
2. **Unordered list** — each selected line becomes `- item`
3. **Indent as sub-list** — nest the current line one level deeper inside its list, or, if the line is not in a list, turn it into a new unordered item

Actions 1 and 2 appear as buttons in the **editor toolbar** (the strip that already holds History and Preview, `Editor.svelte:513`) and as items in the **editor context menu** (`Editor.svelte:407`). Action 3 is a third toolbar button. All three are **always enabled**: with a selection they transform every line it touches, and with a bare caret they transform the caret's line. That keeps them useful without selecting first, and removes the disabled state entirely — matching the History and Preview buttons beside them.

Both 1 and 2 **toggle**: running the unordered action on lines that are already `- items` strips the markers back to plain text; running it on `1. items` converts them to `- items`. Matches VS Code, Obsidian, and Typora.

## Goals

1. Line-oriented list formatting that handles the real cases — existing indentation, blank lines inside a selection, already-nested items, mixed markers — not just the flat happy path.
2. Emit **precise CodeMirror changes**, so each action is a single undo step and the transformed lines stay selected afterwards.
3. Keep the transform logic pure and unit-testable with no editor, no DOM, and no Electron — the `lib/slideEdits.js` + `tests/integration/slide-edits.test.mjs` precedent.
4. No new save path: changes flow through the editor's existing autosave.

## Current State

`Editor.svelte` already contains everything this feature needs:

| Piece | Location | Role here |
|---|---|---|
| `toggleMarker(marker)` | `Editor.svelte:111` | The pattern to follow — a command that computes changes and dispatches with an explicit resulting selection |
| Editor keymap | `Editor.svelte:155` | Where `Mod-b` / `Mod-i` live; optional list shortcuts go here |
| `handleContextMenu` | `Editor.svelte:407` | Static `items` array; gains a conditional list group |
| `.editor-actions` | `Editor.svelte:513` | Right-hand button group holding History and Preview |
| `updateListener` | `Editor.svelte:188` | Already pushes `selectionLength` and `selectionRange` to `projectState` |
| `scheduleSave` | `Editor.svelte:~215` | 500ms debounce → `writeFile` → auto-commit |
| `lib/slideEdits.js` | — | House style for pure markdown transforms |

Because the buttons live **inside** `Editor.svelte`, they reach `editorView` directly. No prop threading through `App.svelte`, and no new editor-command channel on `projectState` (the `scrollToLine` request pattern) is required.

Every dispatch runs through the existing `updateListener`, which sets `projectState.editorContent` and calls `scheduleSave`. Formatting a list saves and auto-commits exactly like typing does. Nothing new is needed.

## New file: `src/renderer/lib/listEdits.js`

Pure functions over an array of line strings. They never see CodeMirror, offsets, or the DOM — the caller maps line numbers to document positions.

```js
// lines      string[]  the whole document, split on \n (0-based array)
// fromLine   number    1-based, inclusive
// toLine     number    1-based, inclusive
// Returns { lines: string[], changed: boolean } — `lines` is the replacement
// text for the span [fromLine, toLine] only, NOT the whole document.

export function toOrderedList(lines, fromLine, toLine)
export function toUnorderedList(lines, fromLine, toLine)

// line       number    1-based
// Returns { lines, changed } for the single line, or a no-op when the line
// cannot be indented (see rules below).
export function indentAsSubList(lines, line)

// Exported for the caller's button-state logic and for tests.
export function classifySpan(lines, fromLine, toLine)  // → 'ordered' | 'unordered' | 'mixed' | 'none'
```

Recognizers, matching CommonMark:

```js
const UNORDERED_RE = /^(\s*)([-*+])(\s+)(.*)$/;
const ORDERED_RE   = /^(\s*)(\d+)([.)])(\s+)(.*)$/;
```

### Transform rules

**Blank lines are never touched.** A blank line inside a selection stays blank — it does not become `- `. It also does not consume an ordinal.

**Leading indentation is preserved per line.** Converting `    Alpha` yields `    - Alpha`, not `- Alpha`. A selection spanning mixed indent levels keeps its shape, so an already-nested structure survives a marker change.

**Toggle is decided across the whole span.** If *every* non-blank line in the span is an unordered item, the unordered action strips; otherwise it converts. Same for ordered. "Every" rather than "any" means a mixed selection converts rather than strips, which is the useful direction.

**Stripping** removes the marker and the whitespace that follows it, keeping the line's leading indentation: `  - Alpha` → `  Alpha`.

**Ordered numbering restarts per indent level** within the span, so a nested block numbers independently of its parent:

```
1. Alpha
   1. Beta
   2. Gamma
2. Delta
```

Numbering **starts at 1** within the selection; it does not continue from an ordered list immediately above. This has no rendering consequence — CommonMark takes the list's start number from the *first* item and disregards the numbers on subsequent items, so a `1. 2. 3.` block converted directly beneath an existing `1. 2. 3.` list still renders 1–6. See Open Questions.

**Marker style** is `-` for unordered and `N.` for ordered. Existing `*`, `+`, or `N)` markers are recognized on input and normalized on output.

### Indent rules (button 3)

Given the caret's line:

| Line is | Behavior |
|---|---|
| Not a list item | Becomes `- text` at its existing indent — a new unordered list |
| A list item with a preceding sibling at the same indent, in the same list | Indented one level; an ordered item is renumbered to `1.` as it becomes the first item of the sub-list |
| A list item that is the **first** item of its list | No-op |
| Blank | No-op |

"Preceding sibling in the same list" means: scanning upward, a list item at the same indent is found before hitting a blank line followed by a non-list line. The first item of a list cannot be indented, because a sub-list with no parent item is not what the user meant and renders as a stray nested list.

**Indent width is the parent item's content column**, not a fixed two spaces:

```
- Alpha          content col 2  →  child indents 2
  - Beta

1. Alpha         content col 3  →  child indents 3
   1. Beta
```

A fixed two-space indent is a real bug for ordered lists — CommonMark requires a child to reach the parent's content column, so `1. Alpha` + two-space child is a lazy paragraph continuation, not a sub-list.

## Wiring in `Editor.svelte`

Three commands in the `toggleMarker` mould (`Editor.svelte:111`), each mapping line numbers to offsets and dispatching one change:

```js
function applyLineEdit(fn) {
  return (view) => {
    const sel = view.state.selection.main;
    const doc = view.state.doc;
    const fromLine = doc.lineAt(sel.from).number;
    const toLine   = doc.lineAt(sel.to).number;
    const lines = doc.toString().split('\n');

    const { lines: replacement, changed } = fn(lines, fromLine, toLine);
    if (!changed) return false;

    const from = doc.line(fromLine).from;
    const to   = doc.line(toLine).to;
    const insert = replacement.join('\n');

    view.dispatch({
      changes: { from, to, insert },
      selection: EditorSelection.range(from, from + insert.length),
      scrollIntoView: true,
      userEvent: 'input.format.list',
    });
    return true;
  };
}
```

One change spanning the whole line range gives one undo step and lets the resulting selection cover exactly the transformed lines — so the user can press the other list button immediately and it operates on the same block.

`EditorSelection` is already imported (`Editor.svelte:11`). The indent command is the same shape but spans a single line and parks a cursor rather than a range, offsetting the caret by the inserted indent so it does not jump to column 0.

**Only the main selection range is transformed.** Multiple cursors fall back to the primary one. Merging overlapping line spans across ranges is not worth the complexity here; `toggleMarker` has the same limitation in practice.

## UI

### Editor toolbar

```
┌─────────────────────────────────────────┐
│ MY-NOTE.MD          [1.][•][→] │ [⟲][👁] │
├─────────────────────────────────────────┤
│ 1  Alpha                                │
│ 2  Beta                                 │
└─────────────────────────────────────────┘
```

Three `.editor-btn` buttons in a group left of History/Preview, separated by a new `.editor-divider` (a 1px `var(--border)` rule with 4px margins — the editor toolbar has no divider element today; `ToolbarDivider` from `@laankit/desktop-ui` is styled for the app toolbar's 36px buttons, not the editor's 28px ones).

| Button | Icon | Title |
|---|---|---|
| Ordered list | `fa-list-ol` | Ordered List |
| Unordered list | `fa-list-ul` | Unordered List |
| Indent as sub-list | `fa-indent` | Indent as Sub-list |

No `disabled` state. With no file open the document is empty, every transform is a no-op on a blank line, and the command returns `false` without dispatching — so an always-enabled button is already harmless.

The two list buttons show an `active` state (the existing `.editor-btn.active` accent colour) when `classifySpan` reports the current span is already entirely that list type — so the toggle's direction is visible before it is pressed. The span is the selection when there is one, otherwise the caret's line.

Buttons call `preventDefault()` on `mousedown` so clicking one does not blur the editor — the selection highlight stays visible while the action runs.

`fa-list-ol` is already used in the app toolbar for the Outline pane (`Toolbar.svelte:38`). Different bar and different context, but if the reuse reads as confusing in practice, `fa-list-numeric` is the alternative.

### Context menu

`handleContextMenu` (`Editor.svelte:407`) builds a static `items` array. It gains a group after the Cut/Copy/Paste block:

```
Select All              Ctrl+A
─────────────────────────────
Cut / Copy / Paste
─────────────────────────────
Ordered List            Ctrl+Shift+7
Unordered List          Ctrl+Shift+8
─────────────────────────────
Save to HTML / PDF / Markdown
```

The items are unconditional, for the same reason the buttons are: with no selection they act on the caret's line. That also suits `ContextMenu.svelte`, which renders every item as an enabled button and has no disabled state.

### Keyboard shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl/Cmd+Shift+7` | Ordered list |
| `Ctrl/Cmd+Shift+8` | Unordered list |

These go in the existing `domEventHandlers.keydown` (`Editor.svelte:176`) and match on `event.code` (`Digit7` / `Digit8`) rather than `event.key`, because `Shift+7` produces `&` on a US layout and something else again elsewhere. The arrow shortcuts right above them already use `event.code` for exactly this reason. A `keymap.of([{ key: 'Mod-Shift-7' }])` binding would be layout-fragile.

`Tab` is taken by `indentWithTab` and stays that way — rebinding it would break normal indentation for the third button's sake, so the indent action has no shortcut.

Like `Mod-b` and `Mod-i`, these live in the editor rather than the command palette. Palette registration (`App.svelte:183`) would require an editor-command channel on `projectState`, since `editorView` is private to `Editor.svelte`. Out of scope; noted in Open Questions.

## Testing

**Unit — `tests/integration/list-edits.test.mjs`**, plain node with no deps, following `slide-edits.test.mjs`:

- flat plain lines → ordered, numbered 1..n
- flat plain lines → unordered
- ordered → unordered (marker swap, not strip)
- unordered → unordered (strip to plain)
- ordered → ordered (strip to plain)
- mixed markers in one selection → converts, does not strip
- blank line inside selection stays blank and consumes no ordinal
- leading indentation preserved on convert and on strip
- nested selection: numbering restarts per indent level
- `*` and `+` normalized to `-`; `N)` normalized to `N.`
- single-line selection
- `classifySpan` returns each of its four values
- indent: plain line → `- text` at its indent
- indent: second item nests under first, at the parent's content column
- indent: ordered child renumbers to `1.`
- indent: first item of a list is a no-op
- indent: parent is `1. Alpha` → child indents 3, not 2

**E2E — `tests/e2e/12-list-formatting.spec.js`**: select three lines, click each toolbar button, assert the editor text; press Ctrl+Z and assert one undo restores the pre-format text; place a bare caret on a line and assert the button formats just that line.

Numbered 12 because `11-slide-management.spec.js` already holds 11.

Add `"test:lists": "node --no-warnings tests/integration/list-edits.test.mjs"` to `package.json`, matching `test:slides`.

## Open Questions

1. **Continuing an adjacent list's numbering.** Numbering starts at 1 within the selection. Continuing from an ordered list directly above would look tidier in the source, but has no effect on rendered output (CommonMark ignores all but the first ordinal) and adds a scan-upward dependency to a pure function.
2. **Outdent.** Not requested. If the third button gets a `Shift`-modifier sibling later, `indentAsSubList` should grow a matching `outdent` so the pair stays symmetric.
3. **Palette registration.** Currently out of scope, because it needs an editor command channel. If the earlier Claude plan lands, it introduces a similar renderer→editor path that these commands could share.

## Phasing

Single phase — the whole feature is one `lib/listEdits.js`, one command wrapper, three buttons, two menu items, and the tests. Roughly:

1. `lib/listEdits.js` + `tests/integration/list-edits.test.mjs` (test-first; the rules above are the test list)
2. `applyLineEdit` wrapper and the three commands in `Editor.svelte`
3. Toolbar buttons, divider, active styles
4. Context-menu group
5. Keymap entries (`event.code` based)
6. E2E spec
