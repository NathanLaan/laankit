# LaanKit: Mode, Theme, and Colour Plan

Status: P1 and P2 implemented (2026-11-20) with your answers to Q1 to Q10, not yet checked by eye in an app. P3 to P5 not started. Splits the one Theme setting into three that you choose separately in Settings: Mode (Light, Dark, or System), Theme (Classic or Modern), and Colour (the colours of the older themes, and Neutral). Classic is the look the apps have always had. Modern is a new look: a quiet title bar, rounded and inset selections, softer buttons and inputs, lighter pane headers, and popups with a shadow.

## Summary

- Mode and Colour are in every app now. Each app's Settings has a Mode list and a Colour list where the Theme list was.
- Theme is built in the shared widgets and offered in the gallery example only. No app offers Modern yet, because each app also draws screens of its own that still have the Classic shapes. P3 to P5 move those, one app at a time.
- The theme you had opens as one combination of the three in Classic, and is drawn the same to the pixel. See "Checked".
- Six colours each have a light and a dark set. Five sets are new: Midnight in light, Mulberry and Rasberryson in dark, and Neutral in both.
- The Modern look is ready for you to review in the gallery. See "To review".

## The three settings

| Setting | Choices | Decides |
|---|---|---|
| Mode | Light, Dark, System | Whether the light or the dark set of the colour is used. System follows the computer's setting, and changes with it while the app is running. |
| Theme | Classic, Modern | The shapes: corners, heights, how a selection and the title bar are drawn, and shadows. |
| Colour | Blue, Purple, Midnight, Mulberry, Rasberryson, Neutral | The family of colours. Each has a light set and a dark set. |

Each is its own group on the UI tab of Settings, in that order, above UI Scale. Each is a list, as the Theme list was. Changing one leaves the other two as they are, and takes effect at once. The Theme group is shown only in an app whose screens have been moved to the shape, which is none of them yet.

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
| Tree row | 26 px. The selection is the full width, with a 2 px stripe at the left. | 28 px. The selection is inset 6 px and rounded 6 px, with no stripe. |
| Tabs | Square, with a line between and an accent line over the open one | Rounded at the top, with a gap between and no lines |
| Corners of buttons and inputs | 4 px, or 6 px in dialogs and Settings | 8 px |
| Corners of menus | 4 px | 10 px |
| Corners of dialogs | 12 px | 14 px |
| Corners of the Command Palette | 10 px | 12 px |
| Primary button in a dialog | A tinted fill with an accent outline, filled when pointed at | Filled with the accent, with no outline |
| Input with the focus | A 1 px outline in the accent | A 2 px ring |
| Dialog header | A bar of the accent, with the title in capitals | The dialog's own colour, the title as written, and a line under it |
| Captions in Settings and dialogs | 11 px capitals | 12 px semibold, as written |
| Buttons and lists drawn by egui itself | egui's padding | Taller, with 12 px by 7 px of padding |
| Scroll bars | Floating, 10 px | Floating, 6 px |
| Shadow under dialogs, menus, and popups | egui's own under its menus, and none under dialogs | A soft shadow under each |

Modern uses the same colours as Classic for each colour and mode.

A screen reader hears a pane's title as the app passes it (`FILES`) in both shapes, so the UI tests find panes the same way.

### The system's mode

With Mode set to System, each app reads the computer's setting every frame and changes with it. egui passes on what the window system reports. Where it reports nothing, System uses Dark, and the Mode group in Settings says so on that computer. I have not seen it change on any desktop. See "Not done".

### Elsewhere

| Part | Now |
|---|---|
| NoteLiner's Command Palette | Set Mode: Light, Dark, and System, and Set Colour: one for each colour, in place of the three Set Theme commands. Set Theme: Classic and Modern join them in P3. |
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
| The Command Palette gets ten commands. | Nine: three for Mode and six for Colour. The two for Theme wait for P3, when NoteLiner offers Modern. |
| A UI test in each app's settings tests. | One test in the shared crate covers the three groups, since each app calls the same function. |

## Checked

- The seven older themes, at 100% and 150% scale: a screenshot of the gallery from the build before this work and one from the build after P2, taken one after the other and compared pixel by pixel. They match, apart from the gallery's spinner, which is animated and differs between any two runs.
- A pointer over the window changes a screenshot, so a pair that differed was taken again.
- This check was made with the gallery's old row of seven theme chips still in place. The gallery's three new rows were added after it.

## To review

Run the gallery, and use its Mode, Theme, and Colour rows:

```bash
cargo run -p laankit-egui --example gallery
```

Or take a screenshot of one combination:

```bash
WAYLAND_DISPLAY= LAANKIT_MODE=light LAANKIT_STYLE=modern LAANKIT_COLOUR=purple \
  LAANKIT_SCREENSHOT=/tmp/shot.png LAANKIT_SCREENSHOT_EXIT=1 \
  cargo run -p laankit-egui --example gallery
```

Things to look at in Modern, in a few colours and both modes: the title bar and its buttons, the toolbar's selected button, the pane headers and the lines between panes, Settings and its lists, the Command Palette, the About dialog, and a toast.

## Not done

- System mode has not been seen to follow a computer's setting on any desktop. It needs a check by hand on each: change the system's setting with an app open.
- The new sets have been looked at in gallery screenshots only, not in an app.
- The gallery has no selected tree row and no tab strip, so those two Modern shapes have not been seen. They show in NoteLiner in P3.
- No UI test is run a second time in Modern yet. That starts in P3, with the first app that offers it.

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
| P2: The shape, and the shared widgets | `Shape` with the Classic and Modern values. The widgets in `crates/laankit-egui` read it: title bar, caption buttons, toolbar, panes, tree, tabs, dialogs, Settings, Command Palette, toasts, About, and egui's own buttons, menus, and scroll bars. The gallery offers Classic and Modern. | Done. No app offers Modern. |
| P3: NoteLiner | NoteLiner's own drawing reads the shape: the Files tree and its rows, the Tags, Search, Backlinks, Outline, and Slides panes, the editor's toolbar, the tab bar, the status bar, the diagram toolbar and Properties panel, and each dialog. The Theme group is shown, and the palette gets Set Theme. | Not started. Done when every NoteLiner screen has been looked at in Modern, in light and dark, in at least Blue and Purple, and the tests pass in both shapes. |
| P4: The editor and rich text | `laankit-editor`, `laankit-richtext`, and `blockliner-egui`: the gutter, the completion popup, code blocks, and the diagram canvas's handles and menus. | Not started. |
| P5: The other apps | ThreadLiner, PageLiner, BlockLiner, MediaLiner, and PrintLiner, one at a time. Each shows the Theme group when its screens are done. | Not started. |

P3 to P5 each depend on P2, and not on each other, except that P4 finishes what P3 leaves in NoteLiner's editor.

The count of places outside the shared crate that set a corner size by hand, as a measure of what P3 to P5 hold:

| Crate or app | Places |
|---|---|
| `apps/noteliner` | 21 |
| `apps/pageliner` | 12 |
| `apps/printliner` | 7 |
| `crates/blockliner-egui` | 6 |
| `apps/medialiner` | 5 |
| `apps/threadliner` | 4 |
| `crates/laankit-editor`, `crates/laankit-richtext` | 1 each |

## Risks

| Risk | What limits it |
|---|---|
| The Modern look is not what you had in mind. | It is in the shared widgets and the gallery only. Nothing reaches an app until you have seen it. |
| What you saw before changes by accident. | The seven older themes are seven combinations in Classic, with the same colours and the same numbers, and their screenshots are compared after each phase. |
| A colour looks wrong in Modern. | The twelve combinations are in the gallery to review, and a colour set can be given a second value for Modern. |
| System mode reports nothing on a Linux desktop. | It uses Dark and says so. Light and Dark still work as fixed choices. |
| A screen is missed, and shows square corners in Modern. | An app shows the Theme group only when its screens are done, and each phase ends with a pass over every screen. A missed place looks out of date, and does not break anything. |
| An app's own layout assumes the Classic row or header height. | The app keeps reading its own constants until its phase, and does not offer Modern until then. |
| Taller buttons in Modern crowd a dense toolbar that egui draws. | Found in each app's phase, where that toolbar can set its own padding. |
