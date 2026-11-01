# noteliner-slides

NoteLiner's presentations, natively: the deck parser and slide edits, plus the egui widgets that show and manage a deck.

```bash
cargo run -p noteliner-slides --example deck -- crates/noteliner-slides/examples/sample-deck.md
cargo test -p noteliner-slides
```

The example opens the SLIDES pane and the deck preview over a note held in memory. P presents from the selected slide, Ctrl+, opens the presentation settings, right-click a slide for its menu. `LAANKIT_SCREENSHOT`, `LAANKIT_THEME`, `LAANKIT_SLIDE=<n>` and `LAANKIT_OPEN=present|settings` work for screenshots. The sample deck and its image are synthetic.

## Modules

| Module      | From the Svelte build                           | What it does                                                         |
|-------------|-------------------------------------------------|----------------------------------------------------------------------|
| `deck`      | `lib/slides.js`                                 | Note body → `Deck`: slides, blocks, notes, directives, layouts (no egui) |
| `edits`     | `lib/slideEdits.js`                             | Pure edits: insert, duplicate, delete, reorder, move, merge, split, retitle, layout, notes (no egui) |
| `render`    | `Slide.svelte`                                  | `SlideView`: one slide on its 1920 × 1080 (etc.) stage, scaled into any rect |
| `pane`      | `SlidesPane.svelte`, `DeckPreview.svelte`       | `SlidesPane` (thumbnails, jump, rename, drag, context menu), `DeckPreview` |
| `settings`  | `PresentationSettingsModal.svelte`              | `PresentationSettings`: the dialog over a draft `Presentation`       |
| `presenter` | (none yet: the Svelte plan calls for one)       | `Presenter`: full-screen presenting with keys and speaker notes       |
| `color`     | `lib/svgColors.js` (names)                      | CSS colours for `bg=`                                                |

`deck` and `edits` are tested against the JS originals: `tests/differential.rs` runs both on generated notes and edits (6,000 per test run, 120,000 checked once with other seeds via `SLIDES_DIFF_CASES` / `SLIDES_DIFF_SEED`) and requires identical JSON and Markdown. It skips itself when `node` isn't on the PATH, or when the JS originals aren't in the checkout: they were removed with the Electron app (commit `d72bf98`), so run it from an earlier commit.

## Integration contract (for `apps/noteliner/native`)

The host owns the note text, its frontmatter and all I/O. The widgets read the current Markdown and `Deck` every frame and return actions.

1. **Is the note a deck?** `normalize_presentation(frontmatter.get("presentation"))` is `Some(config)` for a deck (`presentation: true` means the defaults).
2. **Parse** with `parse_deck(body, Some(&presentation_json))` whenever the editor text changes. The body is the frontmatter-stripped text the editor shows (a leading YAML block is skipped anyway). Line numbers are 1-based editor lines.
3. **Active slide:** `slide_at_line(&deck, caret_line).map(|s| s.index)`.
4. **SLIDES pane:** keep a `SlidesPane`; each frame `pane.show(ui, &markdown, Some(&deck), active, &images)` returns:
   - `PaneAction::Jump { line }`: scroll the editor to `line`.
   - `PaneAction::Edit(EditResult { markdown, caret_line })`: replace the editor text (one undo step), save it, move the caret.
   - `PaneAction::OpenSettings`: open the settings dialog.
5. **Preview column** (when the note is a deck): keep a `DeckPreview`; `preview.show(ui, Some(&deck), active, &vars, &images)` returns a line to jump to. `vars` fills the header/footer placeholders the Svelte preview fills: `name`, `title`, `date` (`YYYY-MM-DD`), `time` (`HH:MM`), `datetime`; `page` and `total` are added per slide.
6. **Commands** (`App.svelte`'s Presentation section) build a `SlideEdit` and call `.apply(&markdown, &deck)`:

   | Command                       | Edit                                                        |
   |-------------------------------|-------------------------------------------------------------|
   | New Slide (Ctrl+Enter)        | `Insert { after: active or slide count, .. }`               |
   | Split Slide at Cursor (Ctrl+Shift+Enter) | `SplitAtLine(caret_line)`                        |
   | Move Slide Up/Down (Alt+↑/↓)  | `Move(active, Direction::Up/Down)`                          |
   | Mark as Speaker Notes (Ctrl+Shift+V) | `ToggleNotes { from_line, to_line }` (selection or caret line) |

   "Convert to Presentation" / "Convert to Note" / "New Presentation" write the frontmatter through `noteliner-core`'s `set_presentation` (`Presentation::default().to_json()` or `null`).
7. **Settings dialog:** `PresentationSettings::new(&deck.config)`; each frame `show(ctx, deck.slides.len())` returns `Open`, `Cancel` or `Save(presentation)`. On Save call `set_presentation(id, &p.to_json())`; on failure set `dialog.error` (e.g. "Set your git name and email in Project Settings first.") and keep it open. `dialog.saving` shows "Saving...".
8. **Presenting:** `Presenter::new(active.unwrap_or(1))`; each frame, after the rest of the UI, `presenter.show(ctx, &deck, &vars, &images)` returns `Changed(index)` (move the caret if you like) and `Exit` (drop it). Skip the app's own shortcut dispatch while presenting.
9. **Images:** implement `ImageSource` (or pass a closure) to map `![](src)` to a URI egui's loaders can load: attachments to `file://…` or `bytes://…`. Install the loaders once (`egui_extras::install_image_loaders`) with the `file` feature (or register bytes). `PassThrough` passes URLs only.
10. **Fonts:** `laankit_egui::fonts::install` must have run (bold, italic and mono faces, Font Awesome).

## Parity with the Svelte slides

| Feature                                                        | Status    | Notes |
|----------------------------------------------------------------|-----------|-------|
| Deck parsing (breaks, setext, fences, comments, directives, notes-only, title slide, layouts) | Done | Identical to `slides.js` on 126,000 generated cases |
| Slide edits (all of `slideEdits.js`)                           | Done      | Identical output and caret line |
| `estimateDuration`, `slideAtLine`, `substitute`                | Done      | |
| Stage sizes 16:9, 16:10, 4:3; dark and light slide themes      | Done      | |
| Layouts: hero, title-body, split, grid, full-bleed, quote, table | Done    | Sizes and gaps from `Slide.svelte`'s CSS |
| Header/footer bands with `{{page}}`, `{{total}}`, host vars; slide numbers | Done | A bare `header` flag prints `true`, as the JS does |
| `bg=` directive                                                | Done      | Hex, `rgb()`/`rgba()`, `transparent`, SVG names; gradients ignored |
| `class=` directive                                             | Done      | `theme-light`/`theme-dark` and `layout-<id>` override the slide's theme and layout (the only class names `Slide.svelte`'s CSS styled); others ignored |
| Inline Markdown: bold, italic, code, links, strikethrough      | Done      | comrak → `laankit-richtext` blocks |
| Lists, quotes, code blocks (wrapped), tables                   | Done      | Including column alignment (`:-:`, `--:`) from the cells' `align` |
| Task lists (`- [ ]` / `- [x]`)                                 | Done      | Drawn boxes, filled with a tick when checked, in the slide theme's colours |
| Images: contain at natural size; full-bleed cover              | Done      | 8 px corners as `img { border-radius: 8px }`; square on full-bleed slides; inline `<svg>` via laankit-richtext |
| Video: YouTube/remote link card                                | Done      | |
| Video: local files play inline                                 | Out of scope | Shown as the link card (decided 2026-10-18) |
| Math (`$…$`, `$$…$$`, ```` ```math ````)                         | Native addition | Typeset by the app's math renderer (NoteLiner: Typst), tinted in the slide theme's text colour; source as code without one (`marked` showed text) |
| `<mark>` highlight                                             | Done      | `background: var(--slide-accent); color: var(--slide-bg)`, as `Slide.svelte` |
| `<kbd>` key caps                                               | Native addition | Code background with a muted outline (the Svelte slides used the browser default) |
| SLIDES pane: thumbnails, active row, jump, notes icon, layout tag, empty states, notes-only count | Done | |
| SLIDES pane: rename (double-click or menu), drag to reorder, context menu (every item) | Done | |
| Deck preview: scaled slides, number rail, notes below, follows the caret, click to jump | Done | |
| Presentation Settings dialog                                   | Done      | Levels 4–6 from frontmatter are shown as "custom" |
| Full-screen presenter                                          | Native addition | Keys, click to advance, S for notes, counter |
| Speaker view in a second window                                | Native addition | `Presenter::with_speaker_view(true)`: current and next slide, notes (rendered, with math), timer (T / R), clock; keys work in both windows; "Swap displays" (X); see below |

## Speaker view

`Presenter::new(start).with_speaker_view(true)` adds a second native window (an egui *immediate* viewport, drawn from the same `Presenter` in the same frame, so both windows always show the same slide without shared state behind locks). It shows the current slide, the next slide (or "End of presentation"), the current slide's notes as rendered Markdown, an elapsed timer (starts with the presentation; T pauses or resumes, R resets) and the clock, with Previous/Next/Swap/End buttons. Every navigation key works in either window; Escape in either ends.

egui can't list monitors or tell which one a window is on, so the audience window goes full screen where it is and the speaker window opens beside it as a normal window. Drag it to the presenter's screen, or press **X** ("Swap displays") to move the full-screen audience window to the speaker window's monitor and the speaker window to the audience window's old place (needs window positions, which Wayland doesn't report: there it does nothing). Ending restores the main window's position and full-screen state; closing the speaker window keeps presenting. Without multi-viewport support (tests, or a backend without it) the speaker view is a panel over the audience view. Every viewport command sent is in `Presenter::commands_sent()`, which the tests check. Tests: `tests/speaker.rs` (headless) and the presenter unit tests.

## Tests

`cargo test -p noteliner-slides`:

- Unit tests for parsing, edits, colours and the renderer's geometry.
- `tests/differential.rs`: the JS comparison above.
- `tests/widgets.rs` (egui_kittest): pane click → jump, context-menu Move Up, drag to reorder, double-click and menu rename, deck-preview click, settings Save/Cancel, presenter keys/click/notes/Escape, and every sample slide rendered at 40, 104, 640 and 1920 px.
