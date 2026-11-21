# NoteLiner: Diagrams in Presentations Plan

Status: for review (2026-11-15). Evaluates whether a BlockLiner diagram in a NoteLiner project can be shown on a slide, and plans it. It is possible, and the parts already exist: NoteLiner can turn a diagram into an image, and slides can already show that kind of image. A slide would refer to the diagram file, so the slide shows the diagram as it is now, and an edit to the diagram shows on the slide without exporting anything.

## Summary

- It is possible, with no new dependency. What is missing is the step that connects the two parts, and a way to insert the reference.
- You embed a diagram with the Markdown image syntax and the diagram's path: `![System map](Work/map.mmd)`.
- The slide shows the live diagram. When you edit and save the diagram, every slide that shows it updates within a few seconds.
- An Insert Diagram command lists the project's diagrams and writes the line for you. You can also drag a diagram from the FILES pane into the editor.
- A diagram is drawn in BlockLiner's light colours. On a dark slide it sits on a light card, so its lines and labels stay readable.
- A diagram that cannot be found or read shows a placeholder that says why.
- The work is in four phases. The first shows a diagram on a slide and is useful on its own.

## Today

| Part | Where | Does |
|---|---|---|
| Diagram files | `.mmd` files in the project, read by `blockliner_core::read` | A diagram is Mermaid text with BlockLiner's layout in comments. NoteLiner opens it on the canvas in a tab. |
| Diagram to image | `blockliner_egui::export::svg` | Draws a diagram as SVG, the same as the canvas at 100%, in light colours on a solid background. Export Diagram uses it for SVG and PNG files. |
| Slide images | `SlideImages` in `apps/noteliner/native/src/deck.rs` | Turns the address in `![alt](address)` into something egui can draw. It accepts web addresses, `data:` addresses, and files in `_attachments/` beside the note. Anything else draws a placeholder with the alt text. |
| SVG on slides | `laankit_richtext::svg` (`InlineSvgLoader`), called from `image_uri` in `crates/noteliner-slides/src/render.rs` | An SVG given as a `data:` address is turned into pixels on a worker thread, with the bundled fonts, and drawn. This is how inline `<svg>` in a slide works today. |
| Slide layout | `crates/noteliner-slides/src/render.rs` | Places an image by the slide's layout (beside the text in Split, filling the slide in Full Bleed, and so on) and scales it to fit. |
| Showing a diagram on a slide today | | Export the diagram to PNG, attach the PNG to the presentation, and repeat after each change to the diagram. |

## Why it is possible

The chain from a diagram file to pixels on a slide exists except for one link.

| Step | State |
|---|---|
| 1. Find the address `Work/map.mmd` in a slide. | Exists. The slide parser already hands every image address to `SlideImages`. |
| 2. Recognize that the address is a diagram in the project and read the file. | Missing. `SlideImages` refuses any path outside `_attachments/`. |
| 3. Parse the file into a diagram. | Exists: `blockliner_core::read`. |
| 4. Draw the diagram as SVG. | Exists: `blockliner_egui::export::svg`, with `blockliner_core::export::scope` for the whole diagram. |
| 5. Hand the SVG to the slide as a `data:` address. | Missing, and a few lines. |
| 6. Turn the SVG into pixels at the size the slide needs and draw it. | Exists: `InlineSvgLoader`. |

NoteLiner already depends on `blockliner-core` and `blockliner-egui` for its diagram editor, so nothing is added to the build.

Two things were checked in the source and not by running them: that the SVG from step 4 is accepted by the loader in step 6, and that the loader's bundled fonts include the one the diagram's labels use. Both crates use `usvg` with their own font lists. The first phase starts by proving this with one diagram on one slide. If the fonts differ, step 5 can hand over a PNG from `blockliner_egui::export::png`, which is known to match the canvas.

## Proposal

### How you write it

A diagram is embedded with the image syntax, with the diagram's path in the project:

```markdown
## How it fits together

![System map](Work/map.mmd)
```

- The path is from the folder the presentation is in, and `../` goes up a folder. This is how links between notes in different folders already work (`link_target` in `apps/noteliner/native/src/app.rs`), and embeds use the same code.
- A path that starts with `/` is from the top of the project. This is new, and it lets a presentation that moves keep pointing at the same diagram.
- The text in the square brackets is the alt text. It is shown when the diagram cannot be drawn, and read by screen readers.
- Everything that works for an image on a slide works for a diagram: the slide layouts, and a slide with only a diagram.
- Other Markdown tools show a broken image for this line, since they cannot draw a `.mmd` file. Nothing else in the note changes.

### What the slide shows

- The whole diagram, scaled to fit the space the layout gives it, without the grid.
- BlockLiner's light colours, as in an exported diagram. On a light slide theme the diagram's background is left out, so it sits on the slide. On a dark slide theme the diagram is drawn on a light card with rounded corners, because dark lines and labels would not show on a dark slide.
- The same picture in the SLIDES pane's thumbnails, the slide preview, the presenter, and the speaker view. They all draw through the same code.

### Keeping it current

The picture is made from the diagram file and kept until the file changes. NoteLiner already checks the project's files for changes every three seconds while its window has the focus, and a diagram you edit in a tab is saved half a second after you stop. So a slide follows its diagram within a few seconds, and at once when you switch from the diagram's tab to the presentation's.

While presenting, the picture is not remade on every frame. It is remade only when the file's time changes.

### When it cannot be drawn

| Case | The slide shows |
|---|---|
| The path does not exist. | A placeholder: "Diagram not found: Work/map.mmd". |
| The file is not Mermaid that BlockLiner can read. | A placeholder: "Could not read diagram: Work/map.mmd". |
| The diagram is empty. | A placeholder: "Empty diagram: Work/map.mmd". |
| The path leaves the project folder. | The alt text, as for any address NoteLiner does not load. |

### Inserting a diagram

| Way | Does |
|---|---|
| Insert Diagram... in the command palette, under Presentation | Opens a list of the project's diagrams with a filter box. Picking one writes the line at the cursor, with the diagram's name as the alt text and the path worked out from the presentation's folder. |
| Insert Diagram... in a slide's right-click menu in the SLIDES pane | The same, at the end of that slide. |
| Dragging a diagram from the FILES pane into the editor | Writes the line where you drop it. |
| New Diagram for This Slide... in the command palette | Makes a new diagram beside the presentation, inserts it, and opens it in a tab. |

Ctrl+click on the line in the editor opens the diagram in a tab, as it does for a link.

### Renaming and moving

Renaming or moving a diagram, or moving the presentation, changes the path between them. NoteLiner would then rewrite the `![...](...)` lines that point at the diagram, in every presentation, as part of the rename or move. Without this the slide shows "Diagram not found" until you fix the path by hand.

NoteLiner does not rewrite links on a rename today, for notes or for attachments. This plan adds it for diagram embeds only.

## Decisions for you

| # | Question | Suggested answer |
|---|---|---|
| Q1 | Is the image syntax with a path the right way to write an embed, or should it be by name, as `![[System map]]`? | The image syntax. It is standard Markdown and it is what slides already read. A name would need a new rule in the slide parser and breaks when two diagrams share a name. |
| Q2 | On a dark slide, is a light card behind the diagram acceptable? | Yes, to start. Drawing diagrams in dark colours is a larger change to BlockLiner's export and can follow. |
| Q3 | Should an embedded diagram also show in the preview of an ordinary note? | Yes, in a later phase. The preview loads its images through similar code, so it is a small addition, and a diagram in a note is as useful as one on a slide. |
| Q4 | Should NoteLiner rewrite the embed lines when a diagram is renamed or moved? | Yes. It is the difference between a feature that keeps working and one that breaks when you tidy your folders. |
| Q5 | Should part of a diagram be embeddable, for example one group of shapes? | No. Split a large diagram into smaller files. |
| Q6 | Should the PDF export of a note include embedded diagrams? | Yes, with Q3. The exporter accepts SVG images, so the diagram goes in as a sharp vector image. |

## Out of scope

- Editing a diagram on the slide. You edit it in its tab.
- Embedding a diagram that is outside the project folder.
- Showing a ` ```mermaid ` code block as a diagram. That is separate work, and it would draw Mermaid's own layouts, not BlockLiner's.
- Animating a diagram, or revealing it a part at a time.
- Exporting a presentation to PDF, HTML, or PowerPoint. That is Phases 3 and 4 of `apps/noteliner/docs/plans/plan-presentations.md`. When it is built, it reads diagrams through the same code.
- Drawing diagrams in dark colours (see Q2).

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1: A diagram on a slide | Prove steps 4 to 6 with one diagram, and fall back to PNG if the fonts differ. `SlideImages` resolves a `.mmd` path, reads and draws the diagram, and keeps the picture until the file's time changes. The placeholders. The light card on dark themes. | `![Map](map.mmd)` shows the diagram in the SLIDES pane, the slide preview, and the presenter, with labels in the right font. Editing the diagram in a tab updates the slide. A missing or unreadable diagram shows its placeholder. Tests cover path resolution, the cache, and each placeholder. |
| P2: Inserting | Insert Diagram in the palette and the SLIDES menu, the picker, dragging from FILES, New Diagram for This Slide, and Ctrl+click to open. | Each way writes a line that P1 draws, with a correct relative path from a presentation in a sub-folder. UI tests cover each. |
| P3: Renaming and moving | Rewrite embed lines when a diagram or a presentation is renamed or moved, including a move of a folder that holds either. | After each kind of rename and move, the slide still shows the diagram, and an open presentation's editor shows the new path without a conflict dialog. Core tests cover each case. |
| P4: Notes and PDF | Embedded diagrams in an ordinary note's preview and in the PDF and HTML export of a note. The Help pages and READMEs. | A note with an embed shows the diagram in the preview and in the exported PDF. |

P1 is useful on its own. P2 and P3 need P1 and do not depend on each other. P4 is the answer to Q3 and Q6 and can be dropped.

## Testing

- Unit tests for resolving a path from a presentation's folder: beside it, in a sub-folder, with `../`, from the top with `/`, with spaces and percent-encoding, and a path that tries to leave the project.
- Unit tests for the cache: the picture is made once, made again when the file's time changes, and dropped for a file that is gone.
- UI tests in `ui_tests_present.rs`: a presentation with an embed draws an image for it, and an edit to the diagram changes the image's address.
- A render test in `crates/noteliner-slides/tests`: a slide with an SVG diagram paints at thumbnail, preview, and full-screen sizes.
- By eye: label fonts against the canvas, a wide diagram and a tall one in each slide layout, light and dark slide themes, and a large diagram (a few hundred shapes) for the time the first draw takes.

## Risks

| Risk | What limits it |
|---|---|
| The slide's SVG loader and BlockLiner's export use different fonts, so labels look wrong. | P1 checks this first, and PNG from BlockLiner's own export is the fallback. |
| A large diagram takes long to draw, and the presenter stalls on its slide. | The picture is made on a worker thread, as inline SVG is today, and kept until the file changes. The slide shows the placeholder until it is ready. |
| A full-screen 4K slide needs a large picture. | The loader makes the picture at the size asked for, and SVG scales without loss. With the PNG fallback, the scale is chosen from the slide's size. |
| A diagram on a light card looks out of place on a dark slide. | Q2. Dark colours for diagrams can follow as its own change. |
| Rewriting lines on a rename changes a note the user has open with unsaved edits. | The rewrite goes through the editor for an open note, as tag changes do, so it does not raise the changed-on-disk dialog. |
| Other tools show a broken image for the line. | The line is ordinary Markdown and harms nothing. This is the cost of a live reference over an exported picture. |
