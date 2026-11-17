# NoteLiner: Presentation Themes Plan

Status: implemented (2026-11-17) on the decisions below, not yet checked by eye. See "Changes during implementation". A presentation had two slide themes of its own, Dark and Light, in colours that matched no NoteLiner theme. This plan replaces them with one slide theme for each NoteLiner theme, in that theme's colours, and lets you pick the theme when you make a presentation.

## Summary

- The slide themes Dark and Light are removed.
- There are seven slide themes, one for each NoteLiner theme, under the same names and ids.
- A slide theme takes its colours from the NoteLiner theme, so the two cannot drift apart.
- A presentation still names its own theme in its file. It does not follow the theme NoteLiner is in, so it looks the same on every machine.
- The New Item dialog gains a Theme list, shown when Presentation is chosen.
- An existing presentation keeps working. Its `dark` or `light` now means NoteLiner's Dark or Light, so its colours change a little.
- The work is one phase.

## Before

| Part | Where | Did |
|---|---|---|
| The slide themes | `SlideTheme::DARK` and `SlideTheme::LIGHT` in `crates/noteliner-slides/src/render.rs` | Six colours each: background, text, muted text, accent, rule, and code background. They were copied from the Svelte build's CSS. |
| A deck's theme | `theme:` in the note's `presentation:` block | `dark` or `light`. Any other name was drawn as dark. |
| One slide's theme | The `class=` directive | `theme-dark` or `theme-light` on a slide overrode the deck's theme. |
| Choosing a theme | Presentation Settings | A list of Dark and Light. |
| A new presentation | `default_presentation` in `apps/noteliner/native/src/backend.rs` | Always started as `dark`. |

## The themes

| Id in the file | Name |
|---|---|
| `midnight` | Midnight |
| `dark` | Dark |
| `darkPurple` | Dark-Purple |
| `light` | Light |
| `lightPurple` | Light-Purple |
| `lightMulberry` | Light-Mulberry |
| `lightRasberryson` | Light-Rasberryson |

The ids are the ones NoteLiner already stores for its own theme.

Each slide colour comes from one colour of the NoteLiner theme:

| Slide colour | Used for | NoteLiner colour |
|---|---|---|
| Background | The slide | `bg_base` |
| Text | Headings and body | `text_primary` |
| Muted text | A hero slide's body, the header and footer, and the slide number | `text_secondary` |
| Accent | Links, highlights, and a checked box | `accent` |
| Rule | The lines of the header, the footer, and tables | `border` |
| Code background | Code and key caps | `code_bg` |

Muted text uses `text_secondary` and not `text_muted`. NoteLiner's muted text has a contrast of about 3.3 to 3.9 against the background, which is too faint for the body of a title slide. `text_secondary` is about 6.8 to 7.9, close to the 6 of the themes removed.

## Proposal

### Naming a theme

- A deck's `theme:` is one of the seven ids.
- The name is read without regard to case or hyphens, so `darkPurple`, `dark-purple`, and `Dark-Purple` are the same theme.
- A deck with no `theme:` is Dark, as before. So is a deck that names a theme NoteLiner does not have.
- A slide's `class=` takes `theme-` and any of the seven, such as `theme-light-mulberry`.

### Existing presentations

A deck that says `dark` or `light` needs no change. It is now drawn in NoteLiner's Dark or Light:

| Colour | Old Dark | New Dark | Old Light | New Light |
|---|---|---|---|---|
| Background | `#14181d` | `#1a1a1a` | `#ffffff` | `#f5f5f5` |
| Text | `#f2f5f8` | `#e0e0e0` | `#16191d` | `#1a1a1a` |
| Accent | `#4da3ff` | `#64a0ff` | `#0b62c4` | `#3278dc` |

### Presentation Settings

The Theme list shows the seven themes.

### The New Item dialog

- With Presentation chosen, a Theme list shows below Template.
- It starts as the NoteLiner theme you are using.
- When you pick a presentation template, the list gains a first choice, "(From the template)", and moves to it. The template's own theme is kept unless you pick another.
- Note, Diagram, Folder, and Duplicate show no Theme list.

### What does not change

- Convert to Presentation still starts a note as Dark. You change it in Presentation Settings, which opens right after.
- A presentation made through the MCP server is as before.

## Decisions

| # | Question | Decision |
|---|---|---|
| Q1 | Do the old Dark and Light stay beside the new themes? | No. They are removed and replaced. |
| Q2 | Is there a "Match app" theme that follows NoteLiner's theme? | No. |
| Q3 | Can you pick the theme when you make a presentation? | Yes, in a list on the New Item dialog, shown for Presentation. |

## Changes during implementation

| Planned | Built |
|---|---|
| Muted text from NoteLiner's muted text. | From `text_secondary`, for contrast. See "The themes". |
| Not decided: the theme the New Item list starts on. | The NoteLiner theme in use. This is my choice, and it is one line to change. |
| Not decided: a template that is already a presentation. | "(From the template)" keeps its theme. Picking a theme keeps the template's other settings and changes only the theme. |
| Not decided: how a theme is spelled in `class=`. | Any case, with or without hyphens. |

## Known limits

- Dark-Purple's accent has a contrast of 2.9 against its background, as it has in NoteLiner. A link on a Dark-Purple slide is hard to read. I left it, since the request was to match.
- In the dark themes the code background and the rule are the same colour, as they are in NoteLiner.
- The old plan `apps/noteliner/docs/plans/plan-presentations.md` still describes Dark and Light.

## Out of scope

- A theme that follows NoteLiner's theme (Q2).
- Themes of your own, and changing single colours of a theme.
- Fonts for slides.
- Changing the NoteLiner themes themselves.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1: Themes | The seven slide themes from the NoteLiner palettes, the lookup by name, `class=`, the Presentation Settings list, the Theme list on New Item, the Help pages, and the READMEs. | Every theme draws a slide in its NoteLiner colours. A new presentation has the theme picked. A deck that says `dark` or `light` opens. Tests cover each. |

## Testing

- Unit tests in `noteliner-slides`: each theme has its NoteLiner colours, a theme is found by id and by name, an unknown name is Dark, and `class=` takes a hyphenated name.
- The widget test paints tasks, a table, and marks in all seven themes.
- A unit test on the New Item form: the theme with and without a template.
- A UI test in `ui_tests_folders.rs`: the Theme list shows only for Presentation, a presentation left alone takes NoteLiner's theme, and a picked theme is written to the file.
- Not tested: a new presentation from a template with a theme picked.
- By eye: a slide in each of the seven themes, and the Theme list on New Item.

## Risks

| Risk | What limits it |
|---|---|
| An existing deck looks different. | Only the shades of Dark and Light change. The table above lists them. |
| Colours tuned for 14 px text read poorly on a slide. | Text and muted text were checked for contrast. The one weak colour is listed under "Known limits". |
| A NoteLiner theme is changed later and decks change with it. | That is what matching means. A deck's file names the theme, not the colours. |
