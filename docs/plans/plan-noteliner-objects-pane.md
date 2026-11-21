# NoteLiner: Objects Pane for Diagrams Plan

Status: for review (2026-11-21). Adds a list of every shape, line, and text object in the open diagram to the sidebar. A click on a row selects the object on the canvas. The plan also reviews whether the Outline pane should become the one place for this, for a note's headings, and for a presentation's slides. Nothing is built yet.

## Summary

- With a diagram open, the sidebar gets a list of its objects in three groups: Shapes, Text, and Lines.
- A click on a row selects that object on the canvas and brings it into view. Selecting on the canvas marks the row.
- Recommendation on re-use: yes for diagrams, no for slides. The Outline pane shows headings in a note and objects in a diagram, and its title changes to match. The Slides pane stays a pane of its own. See the review below and Q1.
- The list itself is built in `blockliner-egui`, so BlockLiner can show the same list later. That is not part of this plan.

## Today

| Part | Where | Does |
|---|---|---|
| The Outline pane | `outline` in `apps/noteliner/native/src/views/sidebar.rs` | Lists the `#` to `####` headings of the open note. A click moves the caret to the heading. |
| The Outline pane with a diagram open | The same | Says "No headings". The pane has no use in a diagram. |
| The Slides pane | `SlidesPane` in `views/sidebar.rs`, `deck.rs` | A thumbnail of each slide, with Present in its header. It exists only while a presentation is open. |
| The panes' buttons | The toolbar in `app.rs` | Outline (Ctrl+Shift+O) is always there. Slides (Ctrl+Shift+G) is there only for a presentation. |
| A diagram's objects | `Diagram` in `crates/blockliner-core/src/model.rs` | `shapes` in stacking order from back to front, and `connectors` in the order they were made. A text object and a note are shapes of the kinds `Text` and `Note`. |
| The selection | `Canvas` in `crates/blockliner-egui/src/canvas/mod.rs` | Shape ids and line numbers. `select_only` sets it. |
| The view | `canvas/view.rs` | Pan and zoom. Nothing moves the view to an object yet. |

## Review: one pane for outlines, objects, and slides

The three lists answer the same question, which is "what is in this document". The question is whether they should share one pane.

| | Headings in a note | Objects in a diagram | Slides in a presentation |
|---|---|---|---|
| Useful today in the Outline pane | Yes | No, it is always empty | Yes, a presentation has headings too |
| A row is | A line of text | An icon and a line of text | A picture, about 90 px tall |
| A click | Moves the caret | Selects on the canvas | Moves the caret to the slide |
| Height it wants | Short | Short to medium | Tall |
| Controls in the header | None | None | Present |
| Wanted at the same time as headings | n/a | No. A diagram has none. | Yes. Headings are how you move inside a long slide and across sections. |

| Option | What you get | What it costs |
|---|---|---|
| A. A separate Objects pane, as Slides is | Nothing existing changes. | A seventh pane, a third toolbar button and a new shortcut, and an Outline pane that stays empty beside it in every diagram. |
| B. Outline shows headings or objects. Slides stays separate. (Recommended) | The empty pane gets a use. One button and one shortcut mean "the structure of this document" in a note and in a diagram. No new pane, setting, or saved height. | The pane's title changes when you move between a note's tab and a diagram's tab. |
| C. One pane for all three | The fewest panes. | In a presentation you lose either the headings or the thumbnails, or need a switch in the header to pick one. The pane's best height jumps between kinds. Present moves or goes. Everyone who has both panes open today loses that. |

B is the recommendation. Objects and headings never compete, since a diagram has no headings, so nothing is lost. Slides and headings do compete, since a presentation has both and both are in use, so merging them takes something away.

## Proposal

### The pane

| Item | With a note or a presentation open | With a diagram open |
|---|---|---|
| Title | Outline | Objects |
| Toolbar button's tooltip | Outline (Ctrl+Shift+O) | Objects (Ctrl+Shift+O) |
| Command Palette | Toggle Outline | The same command, listed also as Toggle Objects |
| Content | Headings, as today | The list below |
| Whether it is shown, its place, and its height | One setting for both, as today. The key in the layout stays `outline`, so nothing saved changes. | |

### The list

```
Objects
v Shapes (4)
    [] Start
    <> Approved?
    [] Ship it
    [] Rectangle
v Text (1)
    T  Draft, do not send
v Lines (3)
    -> Start to Approved?
    -> Approved? to Ship it    yes
    -- Line
```

| Group | Holds | Order |
|---|---|---|
| Shapes | Every shape that is not a text object, notes included. See Q3. | Front to back, so the top row is the shape drawn over the others. |
| Text | Shapes of the kind `Text`. | Front to back. |
| Lines | Every connector: arrows, plain lines, and free lines. | The order they were made. |

| Part of a row | Shows |
|---|---|
| Icon | The shape's own glyph from `blockliner-egui`'s `glyphs.rs`, a T for text, and an arrow or a plain line for a line. |
| Name of a shape or a text object | The first line of its label. When it has no label, its kind, such as Rectangle or Diamond, in the muted colour. |
| Name of a line | "Start to Approved?", from the labels of the shapes at its ends. An end that is not on a shape is left out, and a line with neither is "Line" or "Arrow". |
| Label of a line | At the right of the row, in the muted colour, when the line has one. |
| A shape NoteLiner cannot edit | A shape declared inside a `subgraph` or another statement kept as written. It is listed in the muted colour, with a tooltip that says why. |

- A group's heading has its count and can be folded. A group with nothing in it is not drawn.
- A diagram with no objects shows "No objects".
- Statements that NoteLiner keeps as written and does not draw are not objects, and are not listed.
- A name too long for the pane is cut short with an ellipsis, and the whole name is in the tooltip.

### Selecting

| Action | Result |
|---|---|
| Click a row | Selects that object alone on the canvas. If it is not on screen, the view moves to centre it, at the same zoom. |
| Ctrl+click a row | Adds it to the selection, or takes it out. |
| Shift+click a row | Selects from the last row clicked to this one, within the group. |
| Select on the canvas | The rows are marked, and the pane scrolls to the first of them if none is in view. |
| Click the empty part of the pane | Nothing. The selection is left alone. |

- Selecting from the pane is the same selection as on the canvas. The toolbar, the Properties panel, Delete, and the keyboard shortcuts act on it as they do today.
- Selecting is not an edit. It adds no step to Undo and does not mark the diagram as changed.
- With several tabs, the pane shows the diagram in the tab you are looking at.

### How it is built

| Part | Where | Change |
|---|---|---|
| The rows | New `objects.rs` in `crates/blockliner-egui` | `objects(&Diagram) -> Vec<ObjectRow>`: the group, the icon, the name, the label, and what to select. A `show` function draws them and returns a click. It knows nothing about NoteLiner. |
| Selecting and moving the view | `Canvas` and `DiagramEditor` | A `select` that takes shapes and lines and goes through the editor, so the toolbar's state follows. A new `reveal` on the view that pans to a rectangle only when it is off screen. |
| The pane | `views/sidebar.rs` | `SidebarProps` gains the open diagram's rows and selection. The `outline` pane draws headings or objects by the kind of the open file, and its title is chosen the same way. |
| The app | `app.rs` | Passes the diagram to the sidebar, and applies the pane's click to `DiagramDoc`. |

## Questions

| # | Question | My recommendation |
|---|---|---|
| Q1 | A, B, or C in the review above? | B. Outline shows objects in a diagram, and Slides stays as it is. |
| Q2 | Should the pane's title change to Objects in a diagram, or stay Outline? | Change. "Outline" over a list of shapes reads as a mistake, and you called it Objects. |
| Q3 | Three groups, or one list in stacking order? | Three groups, as you listed them. Lines have no place in the stacking order, so one list would have to invent one. |
| Q4 | Is a note a shape or a text object? | A shape. It has a border and a fill, and is drawn with the shape tools. |
| Q5 | Should a click move the view to an object that is off screen? | Yes, and only then. An object already on screen should not make the canvas jump. |
| Q6 | Should a double-click on a row start editing the object's label? | Not in the first version. A double-click on the canvas already does it, and after a click from the pane the object is on screen. |
| Q7 | Should dragging a row change the stacking order? | No. Bring to Front and Send to Back already do it. It can follow once the list is in use. |
| Q8 | Should rows have a right-click menu, with Delete, Duplicate, and the order commands? | No, for the first version. Those commands already act on the selection, which the click sets. |
| Q9 | Should BlockLiner get the same pane? | Later, as its own plan. The list is built in the shared crate so that it costs little. BlockLiner has no sidebar to put it in today. |
| Q10 | Should there be a box to filter the list by name? | No. Add it if diagrams grow past what you can scan. |

## Out of scope

- Renaming, deleting, reordering, hiding, or locking an object from the pane.
- Groups, layers, and nesting. A diagram has none.
- Objects of a diagram embedded in a note or shown on a slide.
- Any change to the Slides pane.
- BlockLiner the app.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1 | `objects.rs` with the rows and the list, the pane's content and title by kind of file, the tooltip and the command's second name, and a click that selects one object. | With a diagram open the pane is titled Objects and lists every shape, text object, and line under the right group with the right name. A click selects the object. A note still shows its headings, and nothing in the saved layout changes. |
| P2 | Ctrl+click and Shift+click, rows marked from the canvas's selection and scrolled into view, and `reveal`. | The pane and the canvas always agree on the selection. A click on a row for an object that is off screen brings it to the centre, and one for an object on screen moves nothing. |
| P3 | The README, and a check at 500 objects. | The list scrolls without a stall at 500 objects, drawing only the rows in view. |

## Testing

- Unit tests on `objects`: the three groups, front to back within Shapes and Text, a name from the first line of a label, a kind for an empty label, a line's name from its ends, a free line, and a shape that cannot be edited.
- A unit test on `reveal`: a rectangle on screen leaves the pan alone, and one off screen is centred at the same zoom.
- A UI test in `ui_tests_diagram.rs`: open a diagram, find the pane by "Objects", and find a row for each object by its name.
- A UI test: click a row and check the canvas's selection, then Ctrl+click a second row and check both.
- A UI test: select on the canvas and check the row is marked.
- A UI test: with a note open the pane is titled Outline and lists its headings, and switching to a diagram's tab changes both.
- A UI test that a click on a row leaves the diagram unchanged on disk and adds no Undo step.
- By eye: a diagram with every kind of shape, in Classic and Modern, in a light and a dark colour, with the sidebar at its narrowest.

## Risks

| Risk | What limits it |
|---|---|
| The title changing between tabs is surprising. | The button and the shortcut stay in one place, and the tooltip names what the pane shows now. Q2 lets you keep one title. |
| The pane was closed because it is empty in diagrams, so the new list is never seen. | The toolbar button is there in a diagram with its new tooltip. The README says where the list is. |
| Many shapes share a name, such as five with the label "Step". | The rows are in stacking order, the icon gives the kind, and a click shows which one it is. |
| A line's name is long, from two long labels. | It is cut short, with the whole name in the tooltip. |
| A large diagram makes the list slow. | Only the rows in view are drawn. P3 checks 500. |
| The pane and the canvas disagree after an undo, a paste, or a reload from disk. | The rows are built from the diagram each frame, and the marks from the canvas's own selection. Nothing is kept in the pane but which groups are folded. |
