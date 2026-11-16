# NoteLiner: Choosing the Display to Present On Plan

Status: implemented (2026-11-15) except the trial runs and the speaker window's placement, and not yet checked on two displays. See "Changes during implementation". Evaluates whether NoteLiner can let you pick which screen a presentation goes full screen on when the computer has more than one. The answer is yes: the libraries NoteLiner is built on can list the screens and can put a window full screen on a chosen one, on Linux (X11 and Wayland), Windows, and macOS. None of it has been run on two screens yet, so the plan starts with a short trial on a real two-screen setup before anything is built into the app.

## Summary

- It is possible. egui 0.37, which NoteLiner already uses, has a command that puts a window full screen on a screen you name by its number, and the window underneath can list the screens with their names and sizes.
- No new dependency is needed.
- The presenter's notes say "egui can't list monitors". That was true when it was written and is not true of the version in use now.
- You would pick the screen in three places: a Present On submenu when you start, a key while presenting that moves to the next screen, and a list in the speaker window.
- NoteLiner would remember the screen you chose by its name, and use it the next time that screen is connected.
- With one screen connected, nothing changes and nothing new is shown.
- This machine has one screen and runs Wayland, so nothing here could be tried on two screens. The first phase is a trial that settles the open points in "What is not known yet".

## Today

| Part | Where | Does |
|---|---|---|
| Going full screen | `Presenter::show` in `crates/noteliner-slides/src/presenter.rs` | Sends `ViewportCommand::Fullscreen(true)` to the main window. The window goes full screen on the screen it is already on. |
| Moving to another screen | `Presenter::swap_displays` | With the speaker window open, X swaps the two windows' positions. It leaves full screen, moves the main window to where the speaker window is, and goes full screen again. It needs window positions, which Wayland does not report, so on Wayland it does nothing. |
| Without the speaker window | | There is no way to move the presentation. You drag the NoteLiner window to the other screen first, then press F5. |
| Stopping | `Presenter::exit` | Leaves full screen and, after a swap, moves the window back. |

## What the libraries offer

All of this is in the versions NoteLiner already builds with (egui, egui-winit, and eframe 0.37.0, and winit 0.30.13). I read it in their source.

| Need | What provides it | Notes |
|---|---|---|
| List the screens | `eframe::Frame::winit_window()` gives the window, and `available_monitors()` on it gives each screen's name, size in pixels, position, and scale. | The app's `ui` function is handed the `Frame` on every frame and ignores it today. The window is absent in headless tests. |
| Know which screen the window is on | `current_monitor()` on the same window. | Works on X11, Windows, and macOS. On Wayland it reports the first screen the window touches. |
| Go full screen on a chosen screen | `ViewportCommand::SetMonitor(index)`. egui-winit turns it into borderless full screen on the screen at that place in the `available_monitors()` list. | The egui documentation says it works on Windows, macOS, and Linux (X11 and Wayland). An index past the end of the list is ignored and logged. |
| Open a new window on a chosen screen | `ViewportBuilder::with_monitor(index)`. | Could place the speaker window, but it makes that window full screen, which is not wanted. |
| Come back | `ViewportCommand::Fullscreen(false)`. | The window manager returns the window to the size and place it had. This is what happens today. |

Wayland matters here because a Wayland program cannot move its own window, which is why Swap displays does nothing there. Asking for full screen on a named screen is different: it is part of the Wayland protocol, so the request can be made. Whether a given desktop honours it is one of the things the trial checks.

## What is not known yet

| # | Question | How the trial answers it |
|---|---|---|
| U1 | Does `SetMonitor` put the window on the asked-for screen on GNOME and KDE under Wayland, and on X11? | Run the trial program on each with two screens. |
| U2 | Does it work when the window is already full screen on another screen, or must it leave full screen first? | Send it twice in a row with different numbers. |
| U3 | After `Fullscreen(false)`, does the window return to its old screen and size? | Compare the window's place before and after. |
| U4 | Are the screen names useful? X11 and Wayland give connector names such as `DP-1` and `HDMI-A-1`, and Windows gives names such as `\\.\DISPLAY1`. | Print them on each system. |
| U5 | Is the order of the list stable while the program runs, and after a screen is unplugged and plugged back in? | Print the list every second during the trial and unplug a screen. |
| U6 | Does a screen with a different scale (a 4K laptop beside a 1080p projector) draw the slide sharp and at the right size? | Present a slide with small text on each. |
| U7 | What happens when the chosen screen is unplugged while presenting? | Unplug it. The hoped-for result is that the system moves the window to the remaining screen. |

None of these blocks the feature. A "no" to U1 on one desktop means the choice is hidden or disabled there, and you are told why.

## Proposal

### Listing the screens

The app reads the screens from the window once a second while a presentation is open or running, and when a menu that shows them opens. Each screen becomes a small record: its place in the list, its name, its size in pixels, and whether the NoteLiner window is on it. `noteliner-slides` does not depend on winit, so the app builds this list and hands it to the presenter, in the way it already hands over the deck and the images.

A screen is shown by its place and size, with the name after it when there is one:

```
Display 1: 3840 x 2160 (DP-1)   this window
Display 2: 1920 x 1080 (HDMI-A-1)
```

### Choosing before you start

With two or more screens connected, the places that have Present gain a Present On submenu that lists the screens. Picking one starts presenting on it and remembers it.

| Place | Change |
|---|---|
| The command palette | Present On Display 1, Present On Display 2, and so on, under Presentation. They are listed only while two or more screens are connected. |
| Right-click on a presentation in the FILES pane | A Present On submenu below Present. |
| Presentation Settings | A Display list: "The screen this window is on" (the default), or one of the connected screens. |

Present (F5) and the toolbar button use the remembered screen. If you have never chosen, or the remembered screen is not connected, they use the screen the window is on, as today.

### Changing while presenting

| Way | Does |
|---|---|
| D | Moves the presentation to the next screen in the list, and wraps to the first. |
| The on-screen controls | With two or more screens, a display button beside Exit opens the list. |
| The speaker window | A Display list in its top bar. Swap displays (X) stays for X11, Windows, and macOS, and is hidden on Wayland, where it cannot work. |

Each of these sends `SetMonitor` for the chosen screen.

### The speaker window

A program cannot place a normal window on a chosen screen on Wayland, so the speaker window still opens where the system puts it, and you drag it if you need to. On X11, Windows, and macOS it opens on a screen other than the one the presentation is on, at that screen's position plus a margin.

### Remembering the choice

The chosen screen is saved in `ui-preferences.json` as its name and size, not its place in the list, because the place can change when screens are plugged in. It is a setting of this computer, not of the presentation, so it is not written into the note. When the saved screen is not connected, the choice is kept for next time and the current screen is used.

### Stopping

Stopping sends `Fullscreen(false)` as today. If the trial shows the window does not return to its old screen on its own (U3), the presenter also moves it back where the system allows that.

## Decisions for you

| # | Question | Suggested answer |
|---|---|---|
| Q1 | Is the choice remembered for the computer, or saved in each presentation? | For the computer. A presentation opened on another machine would name a screen that is not there. |
| Q2 | Does Present (F5) use the remembered screen, or always the screen the window is on unless you pick Present On? | The remembered screen. Picking once is then enough for a room you present in often. |
| Q3 | Is D the right key for the next screen? | Yes. It is free in the presenter, and X stays as Swap displays. |
| Q4 | Should the first Present on a computer with two screens ask which one? | No. It uses the window's screen, and the controls show how to move. |
| Q5 | Should the trial program be kept in the repository as an example? | Yes, as `crates/noteliner-slides/examples/displays.rs`. It is the quickest way to check a new desktop or a new egui version. |

Q1 to Q5 went ahead on the suggested answers.

## Changes during implementation

| Planned | Built |
|---|---|
| P0 runs before anything is built, and the plan stops if `SetMonitor` works nowhere. | The trial program is written (`cargo run -p noteliner-slides --example displays`), but it has not been run on two displays, and the feature was built without its answers. U1 to U7 are still open. With one display the feature shows nothing, so it is safe to ship unverified. |
| A Present On submenu in the FILES menu. | The displays are listed in the menu itself, below Present, as "Present on Display 1: 3840 x 2160 (DP-1)". |
| Palette entries for each display. | The palette's entries are fixed, so there is one for each of the first four displays, named by number only, plus Present on This Window's Display. |
| A Display list in Presentation Settings. | Not built. That dialog saves into the note, and the display is a setting of the computer. Present on This Window's Display in the palette is the way back to the default. |
| A display button on the controls and a Display list in the speaker window that open the list. | Both are a Next display button, which is the same thing with two displays. |
| The speaker window opens on another display. | Not built. It needs display positions in units that differ between systems, and a wrong guess puts the window off the screen. It waits for the trial. |
| Swap displays is hidden on Wayland. | It is hidden whenever the system does not report the window's position, which is the case on Wayland. |

## Out of scope

- Mirroring the presentation on two screens at once.
- Changing a screen's resolution or the system's display arrangement.
- Exclusive full screen with a chosen video mode. Borderless full screen is enough for slides.
- Remembering a different screen for each presentation.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P0: Trial | A small example program that lists the screens every second and, on a key, goes full screen on the next one and comes back. Run on two screens under GNOME Wayland, KDE Wayland, X11, and Windows. | U1 to U7 each have a written answer in this plan, per system. If `SetMonitor` works on none of them, the plan stops here. |
| P1: The list and the presenter | The app reads the screens and passes them to the presenter. The presenter takes a starting screen, sends `SetMonitor`, and handles D. The comment in `presenter.rs` that says egui cannot list monitors is corrected. | With two screens, D moves the presentation to the other one and back, and stopping returns the window. Presenter tests check the commands sent for a made-up list of screens. |
| P2: Choosing and remembering | The Present On entries in the palette and the FILES menu, the Display list in Presentation Settings and in the speaker window, the display button on the controls, and the saved choice. | Picking a screen starts there and is used by the next F5. With the screen unplugged, F5 uses the window's screen and the choice is still saved. With one screen, none of the new entries show. UI tests cover each with a made-up list. |
| P3: The speaker window and the docs | The speaker window opens on another screen where the system allows it. Swap displays is hidden on Wayland. The Help page and the READMEs are updated. | Checked by hand on two screens on each system from P0. |

P1 needs P0. P2 and P3 need P1 and do not depend on each other.

## Testing

- The presenter already records every window command it sends (`Presenter::commands_sent`), and its tests check that list. The new tests pass in a made-up list of two or three screens and check for `SetMonitor` with the right number.
- UI tests give the app a made-up list, since a headless test has no window and so no real screens.
- Two real screens cannot be tested automatically. P0 and P3 are checks by hand, and their results are written into this plan.

## Risks

| Risk | What limits it |
|---|---|
| A desktop ignores the request for a named screen. | P0 finds this before anything is built. On that desktop the choice is disabled with a line saying why, and dragging the window before F5 still works. |
| The list's order changes when a screen is plugged in, so a remembered number points at the wrong screen. | The choice is saved by name and size, and the number is looked up again each time it is used. |
| Two identical screens have the same name and size. | The name includes the connector on Linux and the device on Windows, so they differ. If they do not, the first match is used. |
| The feature cannot be tried on the development machine, which has one screen. | P0 and P3 need a second screen or a projector, and are marked as hand checks. |
| A later egui release changes `SetMonitor`. | The trial program stays in the repository and shows a break in a few seconds. |
