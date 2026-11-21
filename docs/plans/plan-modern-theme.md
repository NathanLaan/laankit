# LaanKit: Mode, Theme, and Colour Plan

Status: implemented (2026-11-20), all five phases, with your answers to Q1 to Q10 and the changes you asked for afterwards. Not yet checked by eye in a running app. Splits the one Theme setting into three that you choose separately in Settings: Mode (Light, Dark, or System), Theme (Classic or Modern), and Colour (the colours of the older themes, and Neutral). Classic is the look the apps have always had. Modern is a new look: a rounded window, a quiet title bar, rounded and inset selections, softer and larger buttons and inputs, lighter pane headers, and popups with a shadow.

## Summary

- Every app's Settings has a Mode list, a Theme list, and a Colour list where the one Theme list was.
- The theme you had opens as one combination of the three in Classic, and is drawn the same to the pixel. See "Checked".
- Six colours each have a light and a dark set. Five sets are new: Midnight in light, Mulberry and Rasberryson in dark, and Neutral in both.
- Modern is offered in all six apps. Each app's own drawing reads the shape, and each app's window has rounded corners in Modern.
- Every UI test passes in Classic and again with Modern forced.
- Nothing has been looked at in a running app by a person. See "Not done".

## The three settings

| Setting | Choices | Decides |
|---|---|---|
| Mode | Light, Dark, System | Whether the light or the dark set of the colour is used. System follows the computer's setting, and changes with it while the app is running. |
| Theme | Classic, Modern | The shapes: corners, heights, how a selection and the title bar are drawn, and shadows. |
| Colour | Blue, Purple, Midnight, Mulberry, Rasberryson, Neutral | The family of colours. Each has a light set and a dark set. |

Each is its own group on the UI tab of Settings, in that order, above UI Scale. Each is a list, as the Theme list was. Changing one leaves the other two as they are, and takes effect at once.

### The colours

| Colour | Dark set | Light set |
|---|---|---|
| Blue | The Dark theme | The Light theme |
| Purple | Dark-Purple | Light-Purple |
| Midnight | Midnight | New |
| Mulberry | New | Light-Mulberry |
| Rasberryson | New | Light-Rasberryson |
| Neutral | New | New |

| New set | How it is made |
|---|---|
| Mulberry, dark | The greys of Dark, with the accent lightened to `#e06aa8` so that it reads on a dark surface. |
| Rasberryson, dark | The same, with the accent `#ff6f8b`. |
| Midnight, light | Midnight is the Catppuccin Mocha palette, so its light set is Catppuccin Latte with the same roles: a pale blue-grey sidebar and page, and the accent `#1858e0`. |
| Neutral | Greyscale. Every colour in it is a grey, the accent included: `#d6d6d6` in dark, and `#3a3a3a` in light on a white page. Links, selections, and the focus ring are grey too. A unit test holds it to greys. |

A unit test holds the new sets to these contrasts, on the page and on the sidebar:

| Text | At least |
|---|---|
| Primary, secondary, and tertiary text | 4.5 to 1 |
| Muted text, and the accent | 3 to 1 |
| Text on the accent | 4.5 to 1 |

Muted text is held to 3 to 1 and not 4.5, because that is where the seven older sets have it (about 3.4 to 1), and the new sets are meant to sit beside them. Neutral's muted text is above 4.5 in both modes.

### Your older theme carries over

| Theme before | Mode | Theme | Colour |
|---|---|---|---|
| Midnight | Dark | Classic | Midnight |
| Dark | Dark | Classic | Blue |
| Dark-Purple | Dark | Classic | Purple |
| Light | Light | Classic | Blue |
| Light-Purple | Light | Classic | Purple |
| Light-Mulberry | Light | Classic | Mulberry |
| Light-Rasberryson | Light | Classic | Rasberryson |

A new install starts as Dark, Classic, Midnight, as before.

The saved choice is now `{ theme, mode, style, colour, scale }`. The old `theme` id is still read when the other three are missing, and is still written, as the nearest of the seven. An older build reading the same settings shows that theme. ThreadLiner's synced settings hold the same nearest id, so two computers agree on light or dark and, where it can be said in the older ids, on the colour.

### Classic and Modern

A record, `Shape`, holds what Theme decides. `ThemeState::apply` hands the colours and the shape to the widgets. A shared widget reads the shape where it had a number.

| Part | Classic | Modern |
|---|---|---|
| Title bar | A bar of the accent colour | The colour of the sidebar, with a 1 px line under it. Its buttons get a rounded fill when pointed at. |
| Toolbar's selected button | A tinted fill with an accent outline | The tinted fill alone, with the icon in the accent colour |
| Toolbar | Its own darker colour, with a line beside it | The colour of the sidebar, with no line |
| Pane header | 44 px, `FILES`, 11 px, a line under it | 32 px, `Files`, 12 px semibold, no line |
| Between panes | A 4 px bar | A 1 px line in the same 4 px band, which fills in when pointed at or dragged |
| Tree row | 26 px. The selection is the full width, with a 2 px stripe at the left. | 28 px. The selection is inset 6 px and rounded 8 px, with no stripe. |
| Tabs | Square, with a line between and an accent line over the open one | Rounded at the top, with a gap between and no lines |
| Corners of the window | Square | 12 px, where the app's window allows it. See "The window's corners". |
| Corners of buttons and inputs | 4 px, or 6 px in dialogs and Settings | 10 px |
| Corners of menus | 4 px | 12 px |
| Corners of dialogs | 12 px | 18 px |
| Corners of the Command Palette | 10 px | 14 px |
| Primary button in a dialog | A tinted fill with an accent outline, filled when pointed at | Filled with the accent, with no outline |
| Input with the focus | A 1 px outline in the accent | A 2 px ring |
| Dialog header | A bar of the accent, with the title in capitals | The dialog's own colour, the title as written, and a line under it |
| Captions in Settings and dialogs | 11 px capitals | 12 px semibold, as written |
| Headings an app draws itself (`PROPERTIES`, `LIBRARY`, a section in Help) | Capitals | `Properties`, `Library` |
| Corners an app draws itself, 4 px and up | As they were | The next rounder step. Corners of 1 to 3 px are text highlights and small marks, and stay. |
| Size of the controls | As they were | 10% larger: dialog buttons, text inputs, the lists and checkbox rows in Settings, and egui's own buttons and checkboxes. Their text stays the size the UI scale gives it. The toolbar's buttons stay 36 px. See "Changes during implementation". |
| Buttons and lists drawn by egui itself | egui's padding | Taller, with about 13 px by 8 px of padding |
| Scroll bars | Floating, 10 px | Floating, 6 px |
| Shadow under dialogs, menus, and popups | egui's own under its menus, and none under dialogs | A soft shadow under each |

Modern uses the same colours as Classic for each colour and mode.

A screen reader hears a pane's title as the app passes it (`FILES`) in both shapes, so the UI tests find panes the same way.

### The window's corners

In Modern the window itself has rounded corners, of 12 px. The apps draw their own title bar on a window with no frame from the system, so the corners are the app's to draw.

| Part | How |
|---|---|
| The window | Made see-through where nothing is drawn (`window::undecorated_rounded`). This is chosen when the window is made, so every app's window is now made this way, in Classic too. |
| The corners | After the app has drawn its frame, `window::window_edges` clears the pixels outside the curve at each corner, a row at a time, and draws the window's hairline along the curve. No panel has to round its own corner. |
| The clear colour | Nothing in Modern, and the panel colour in Classic (`window::clear_color`). |
| Maximized or full screen | Square. |
| macOS | Left to the system, which rounds its windows itself. |
| With the system's own title bar (Custom Window Titlebar off) | Left to the system. |

A screenshot of each of the six apps in Modern has all four corner pixels fully see-through, and in Classic has them filled.

A see-through window needs a compositor. Every current desktop has one. On an X11 session without one, the corners would show black.

### The system's mode

With Mode set to System, each app reads the computer's setting every frame and changes with it. egui passes on what the window system reports. Where it reports nothing, System uses Dark, and the Mode group in Settings says so on that computer. I have not seen it change on any desktop. See "Not done".

### Elsewhere

| Part | Now |
|---|---|
| NoteLiner's Command Palette | Set Mode: Light, Dark, and System, Set Theme: Classic and Modern, and Set Colour: one for each colour, in place of the three older Set Theme commands. |
| ThreadLiner | Offers Blue and Midnight, the colours of the three themes it offered. |
| A new presentation in NoteLiner | Its slide theme starts as the nearest of the seven older themes to the mode and colour in use. The slide themes themselves are unchanged. |
| Environment variables | `LAANKIT_MODE`, `LAANKIT_STYLE`, and `LAANKIT_COLOUR` are new. `LAANKIT_THEME` still takes the seven older ids, and sets the mode and colour. |
| The gallery example | Has a row each for Mode, Theme, and Colour, and its Settings shows all three groups. |

## Your answers

| # | Question | Answer |
|---|---|---|
| Q1 | Make the three missing sets? | Yes. |
| Q2 | The names of the colours | Blue, Purple, Midnight, Mulberry, Rasberryson, and Neutral. |
| Q3 | Are the values in the shape table the look you have in mind? | Yes. |
| Q4 | Should a new install start in System mode, or in Modern? | Neither. |
| Q5 | A command in the palette for each choice? | Yes. |
| Q6 | Color or Colour? | Colour. |
| Q7 | Add the neutral greys as a colour? | Yes. |
| Q8 | Switches in place of checkboxes in Modern? | No. |
| Q9 | Fades on hover and selection? | No. |
| Q10 | Which app gets Modern first? | NoteLiner. |

## Changes during implementation

| Planned | Built |
|---|---|
| Three choices in a row for Mode, two for Theme, and swatches for Colour. | Each is a list, like the Theme list it replaces and the UI Scale list below it. |
| `LAANKIT_COLOR`. | `LAANKIT_COLOUR`, to match the label. |
| Classic corners are 4 px, and Classic has no shadows. | Dialogs and Settings already used 6 px, the Command Palette 10 px, and the palette and toasts already had a shadow. Classic keeps each of those. Modern takes the next rounder step from whatever Classic had. |
| Scroll bars float in Modern only. | egui's scroll bars already float. Modern makes them thinner. |
| The button outline is removed in Modern. | It is removed on the buttons the shared widgets draw. Buttons drawn by egui itself share one outline setting with inputs, so they keep it. In most colours that outline is the same colour as the button and cannot be seen. |
| A colour set can carry a second value for Modern. | Not needed yet, and not built. Purple's accent is as dim in Modern as it is in Classic. |
| The Command Palette gets ten commands. | Eleven, with Neutral: three for Mode, two for Theme, and six for Colour. |
| Controls 10% larger includes the toolbar. | The toolbar's buttons were made larger, and then put back. The toolbar does not scroll, and at NoteLiner's default window height the larger buttons pushed Present with Speaker View off it. |
| Each app rounds the panels that reach its window's corners. | One shared routine cuts the corners out of whatever the app drew. NoteLiner alone has eight panels that can reach a corner, depending on what is open. |
| Every corner an app draws is moved to the shape. | Corners of 4 px and up are. Corners of 1 to 3 px are highlights behind text and small marks, and would look wrong any rounder. Rendered content is left alone: notes, slides, diagrams on the canvas, articles, and books. |
| A UI test in each app's settings tests. | One test in the shared crate covers the three groups, since each app calls the same function. |

## Checked

- The seven older themes in the gallery, at 100% and 150% scale: a screenshot from the build before this work and one from the build after P2, taken one after the other and compared pixel by pixel. They match, apart from the gallery's spinner, which is animated.
- Each of the six apps in Classic, in Midnight and in Light: a screenshot from the last commit before P3 and one from the build after P5, compared pixel by pixel. All twelve pairs are the same. NoteLiner was opened on a demo project with a note selected, and the other five as they open with nothing set up, so this covers each app's frame and first screen, and not every screen.
- All 857 tests in the workspace pass in Classic, and pass again with `LAANKIT_STYLE=modern`.
- A screenshot of each app in Modern was looked at: NoteLiner on a demo project, and the other five as they open with nothing set up.
- A pointer over the window changes a screenshot, so a pair that differed was taken again.

## To review

In any app, open Settings and choose Modern in the Theme list. The window's corners need Custom Window Titlebar on, which takes a restart.

The gallery shows every shared widget, with a row each for Mode, Theme, and Colour:

```bash
cargo run -p laankit-egui --example gallery
```

A screenshot of one combination, in any app:

```bash
WAYLAND_DISPLAY= LAANKIT_MODE=light LAANKIT_STYLE=modern LAANKIT_COLOUR=purple \
  LAANKIT_SCREENSHOT=/tmp/shot.png LAANKIT_SCREENSHOT_EXIT=1 \
  cargo run -p noteliner-native
```

## Not done

- Nothing has been used in a running app. Screenshots show one frame of one state. Menus, dropdowns while open, drag and drop, the Command Palette, most dialogs, and a project with real content have not been seen in Modern.
- The rounded corners have been seen in screenshots of the app's own drawing only. I could not see what the desktop shows behind them. Look at them on each system you test on, and on Windows in particular, where a see-through window with no frame is the least tried.
- Every app's window is now see-through in Classic too, since that is chosen when the window is made. It should make no difference to what you see. It has not been checked on Windows or macOS.
- System mode has not been seen to follow a computer's setting on any desktop. Change the system's setting with an app open, on each.
- ThreadLiner, PageLiner, BlockLiner, MediaLiner, and PrintLiner were seen in Modern only as they open with nothing set up. Their lists, readers, canvases, and dashboards with content in them have not been seen.
- Row heights and header heights that an app sets for itself are unchanged in Modern, apart from NoteLiner's file tree. A pane an app draws itself can be a few pixels shorter or taller than a shared one beside it.
- Settings is three groups longer than it was. In BlockLiner at 800 px tall, the last settings on its page are now below the fold, and one test was given a taller window for that.

## Out of scope

- A new layout: moving, adding, or removing toolbars, panes, or menus.
- New fonts or a new icon set.
- Slide themes in NoteLiner, and the reading themes in PageLiner.
- Themes you make yourself.
- A different mode, theme, or colour for each app. Each app saves its own choice.
- The old Svelte builds. They read the old `theme` id, which is still written.

## Phases

| Phase | Scope | State |
|---|---|---|
| P1: Mode and Colour | `Mode` and `Colour` in `crates/laankit-egui/src/theme.rs`, the five new sets, the saved form, System mode, the groups in Settings, NoteLiner's commands, ThreadLiner's colours, the environment variables, and every app. | Done. |
| P2: The shape, and the shared widgets | `Shape` with the Classic and Modern values. The widgets in `crates/laankit-egui` read it: title bar, caption buttons, toolbar, panes, tree, tabs, dialogs, Settings, Command Palette, toasts, About, and egui's own buttons, menus, and scroll bars. | Done. |
| P3: NoteLiner | The see-through window and its corners. NoteLiner's own drawing reads the shape: the Files tree and its rows, the Tags, Search, and Backlinks panes, the attachments and Properties panels, the diagram toolbar, Help, the tag editor, and the dialogs. The Theme group is shown, and the palette has Set Theme. | Done. Seen in one screenshot of the demo project. |
| P4: The editor and the diagram crate | `laankit-editor`: the toolbar's buttons. `blockliner-egui`: the Properties panel and the export and colour dialogs. The editor's text, the rich text view, and the diagram canvas are content, and are unchanged. | Done. |
| P5: The other apps | ThreadLiner, PageLiner, BlockLiner, MediaLiner, and PrintLiner: the see-through window, the Theme group, and each app's own corners, row fills, and headings. | Done. Each seen in one screenshot, with nothing set up. |

## Risks

| Risk | What limits it |
|---|---|
| The Modern look is not what you had in mind. | It is in the shared widgets and the gallery only. Nothing reaches an app until you have seen it. |
| What you saw before changes by accident. | The seven older themes are seven combinations in Classic, with the same colours and the same numbers, and their screenshots are compared after each phase. |
| A colour looks wrong in Modern. | The twelve combinations are in the gallery to review, and a colour set can be given a second value for Modern. |
| System mode reports nothing on a Linux desktop. | It uses Dark and says so. Light and Dark still work as fixed choices. |
| A screen is missed, and shows square corners or a full-width selection in Modern. | The conversion was made by rule across each app's drawing code, so what it missed is drawing that does not follow the usual forms. A missed place looks out of date, and does not break anything. It is found by using each app in Modern. |
| An app's own layout assumes the Classic row or header height. | Each app keeps its own heights, except NoteLiner's file tree. The tests pass in Modern, which covers the layouts the tests reach. |
| Taller buttons in Modern crowd a dense row of controls that egui draws. | Not seen in the screenshots taken. It would show when using each app in Modern. |
