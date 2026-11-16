# NoteLiner: Finding Present Plan

Status: implemented (2026-11-15), not yet checked by eye. NoteLiner can already show a presentation full screen, but the only buttons that start it are two icons in the left toolbar, and they were not found. The plan moves the presentation buttons to their own group at the end of the left toolbar, below Search and Backlinks, and sets the rule that buttons for one file type always come below the buttons every file type has. It also adds Present to the right-click menus and to the SLIDES pane. It adds a full-screen key and on-screen controls to the presenter, so you can also leave it without knowing that Escape does that.

## Summary

- The left toolbar's presentation buttons (Slides, Present, and Present with Speaker View) move below Search and Backlinks, after a divider. The buttons above the divider are the same for every file type and never move.
- Present is added to the right-click menu of a presentation in the FILES pane and of a slide in the SLIDES pane, and the SLIDES pane's header gets a Present button.
- While presenting, moving the mouse shows a small bar at the bottom of the screen with Previous, Next, the slide number, and Exit. It hides again after three seconds.
- F5 and Alt+F5 keep working. Present from the First Slide is added to the command palette, with Shift+F5.
- The work is in three phases, and the first one alone fixes the report.

## Today

| Part | Where | Does |
|---|---|---|
| The presenter | `crates/noteliner-slides/src/presenter.rs` | Draws the presentation over the whole window, one slide at a time, and puts the window in full screen. It restores the window when you stop. |
| Speaker view | The same file | Opens a second window with the current slide, the next slide, your notes, a timer, and the clock. |
| Starting it | `Act::Present` and `Act::PresentWithSpeakerView` in `apps/noteliner/native/src/app.rs` | Starts at the slide the cursor is in. |
| Buttons | The left toolbar, in `app.rs` | Two icons, a play circle and a chalkboard, between Slides and Tags. They are drawn only while the open note is a presentation. They have no label, and the chalkboard is the same icon as the Slides button above it. |
| Keys | `app.rs` | F5 and Alt+F5. |
| Command palette | `apps/noteliner/native/src/commands.rs` | Present and Present with Speaker View, under Presentation. |
| Right-click menus | `views/tree.rs`, `crates/noteliner-slides/src/pane.rs` | Presentation Settings only. Neither has Present. Tabs have no right-click menu. |
| While presenting | `presenter.rs` | Keys and a click move between slides, and Escape stops. Nothing on the screen says so. |

A UI test now checks that the two toolbar buttons are drawn when a presentation is open (`ui_tests_present.rs`), and it passes. So the buttons are there, and the problem is that they are hard to find.

## Why the buttons were missed

- They sit in the middle of the left toolbar, between Outline and Tags. When you open a presentation, three buttons appear there and push Tags, Attachments, Search, and Backlinks down. When you leave it, they move back up.
- They have no label. You learn what they do only by resting the mouse on them.
- Present with Speaker View uses the same icon as Slides, so two different buttons look alike.
- Nothing near the presentation itself (the SLIDES pane, its right-click menu) offers Present.

## Proposal

### The left toolbar

The rule: a button that is offered for every file type is a standard button, and a button that is offered for one file type is a context button. Standard buttons come first and keep their place whatever is open. Context buttons come after them, below a divider, and the divider is drawn only when there is a context button to show.

| Group | Buttons | Shown |
|---|---|---|
| Standard | Files, Outline, Tags, Attachments, a divider, Search, Backlinks | Always, in this order. |
| Context | A divider, Slides, Present, Present with Speaker View | While the open file is a presentation. |
| Bottom | Show Log, Status Bar, a divider, Settings, About | Always, against the bottom edge. |

```
 Note or diagram      Presentation
 ┌────┐               ┌────┐
 │ ≡  │ Files         │ ≡  │ Files
 │ ☰  │ Outline       │ ☰  │ Outline
 │ 🏷  │ Tags          │ 🏷  │ Tags
 │ 📎 │ Attachments   │ 📎 │ Attachments
 │ ── │               │ ── │
 │ 🔍 │ Search        │ 🔍 │ Search
 │ 🔗 │ Backlinks     │ 🔗 │ Backlinks
 │    │               │ ── │
 │    │               │ ▦  │ Slides
 │    │               │ ▶  │ Present
 │    │               │ ▶+ │ Present with Speaker View
 │    │               │    │
 │ >_ │ Show Log      │ >_ │ Show Log
 │ …  │               │ …  │
 └────┘               └────┘
```

- With the system title bar, Home, Open Folder, New File, and Import stay at the top of the standard group, as today.
- Present with Speaker View gets its own icon, so it no longer looks like Slides. Slides keeps the chalkboard.
- The rule is written down in the comment above the toolbar code and in `apps/noteliner/native/README.md`, so a later button for one file type (a diagram button, say) goes in the context group.
- A diagram has no context buttons on this toolbar today. Its drawing tools are in the bar above the canvas.
- The context group's order is Slides first, since it is a pane like the buttons above it, then the two actions.

The toolbar does not scroll. In a short window with a large UI scale, the context group could reach the bottom group. The context group is drawn only in the space above the bottom group, and a button that does not fit is left out. F5 and the command palette still start presenting.

### Present from the First Slide

Present and Present with Speaker View start at the slide the cursor is in, as today. A third command, Present from the First Slide, starts at slide 1. It is in the command palette under Presentation, and its key is Shift+F5. It has no toolbar button.

### Present in the menus and the SLIDES pane

| Place | Change |
|---|---|
| Right-click on a presentation in the FILES pane | Present and Present with Speaker View, above Presentation Settings. Present opens the presentation first if it is not the open note. |
| Right-click on a slide in the SLIDES pane | Present from This Slide, at the top. |
| The SLIDES pane's header | A play button beside the close button. |

### On-screen controls while presenting

Moving the mouse shows a bar at the bottom centre of the slide. It fades after three seconds without movement, and the mouse pointer hides with it.

```
                    ┌────────────────────────────────┐
                    │  ‹    3 / 12    ›    │   ✕ Exit │
                    └────────────────────────────────┘
```

- Previous and Next do what the arrow keys do.
- Exit does what Escape does.
- A click on the bar does not also move to the next slide. A click anywhere else still does.
- The bar is not drawn in the speaker window, which has its own buttons.

### Full-screen key

F11 while presenting takes the window out of full screen and puts it back, without stopping. You can then present in a window, which is useful when you share one window in a video call. The presenter still restores the window's earlier state when you stop.

## Decisions

| # | Question | Decision |
|---|---|---|
| Q1 | Is there a Present button above the editor? | No. The left toolbar, the menus, and the SLIDES pane are the places. |
| Q2 | Does Present start at the cursor's slide or at slide 1? | The cursor's slide, as today. Shift+F5 starts at slide 1. |
| Q3 | Are the on-screen controls wanted? | Yes. They show only when the mouse moves. |
| Q4 | Is presenting in a window (F11) wanted? | Yes. |
| Q5 | Should a presentation open with the SLIDES pane shown the first time? | No. The pane's state stays as you left it. |

Q2 to Q5 were suggested answers that went ahead without a reply. Say so if one is wrong.

## Out of scope

- Choosing the display to present on. The presenter goes full screen on the display the window is on, and Swap displays (X) in the speaker view stays as the way to move it. See [plan-noteliner-present-display.md](plan-noteliner-present-display.md).
- A slide overview grid, a laser pointer, a blank-screen key, and drawing on slides.
- Slide transitions and animation.
- Exporting a presentation to PDF, HTML, or PowerPoint. See `apps/noteliner/docs/plans/plan-presentations.md`, Phases 3 and 4.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1: The left toolbar | Standard buttons first, then the context group below a divider. A new icon for Present with Speaker View. The rule in the code comment and the README. | Tags, Attachments, Search, and Backlinks are in the same place for a note, a diagram, and a presentation. Slides, Present, and Present with Speaker View show below Backlinks for a presentation only. UI tests cover the order for each file type. |
| P2: Menus and the SLIDES pane | Present from the First Slide with Shift+F5. Present in the FILES and slide right-click menus, and the play button in the SLIDES pane's header. | Each one starts presenting at the right slide. Present on a presentation that is not open opens it first. UI tests cover each place. |
| P3: The presenter | The on-screen controls, the hidden pointer, and F11. The Help pages and the key table in `presenter.rs` are updated. | The bar shows on mouse movement and hides after three seconds. Its buttons work, and a click on it does not change the slide twice. F11 leaves and returns to full screen without stopping. Checked by eye in both themes. |

P1 is the fix for the report and can ship on its own. P2 and P3 do not depend on each other or on P1.

## Testing

- UI tests in `ui_tests_present.rs` for each new way to start presenting, and for the starting slide of each.
- A UI test that the standard buttons are in the same order and at the same height for a note, a diagram, and a presentation, and that the context group follows Backlinks.
- Presenter tests in `crates/noteliner-slides/tests` for the controls: shown after a pointer move, hidden after three seconds, and a click on Next moving one slide.
- By eye: the left toolbar for each file type, the controls over a light and a dark slide theme, and a real run on two displays with the speaker view.

## Risks

| Risk | What limits it |
|---|---|
| Moving the left toolbar buttons moves something a user has learned. | They stay on the same toolbar, and F5, Alt+F5, and the command palette entries do not change. |
| A fourth context button later makes the toolbar too tall for a small window. | A button that does not fit is left out, and each one has another way in (a key or the command palette). |
| The on-screen bar shows during a talk when the mouse is bumped. | It is small, sits at the bottom, and hides after three seconds.  |
