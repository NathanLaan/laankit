# Plan Summary and Recommendation

Status: For review

This summary compares the three refactor plans drafted in this directory
and recommends a path forward.

## The three plans

| Plan | What it does | Scope |
|---|---|---|
| [`plan-refactor-refresh-ui.md`](./plan-refactor-refresh-ui.md) | Copy NoteLiner's UI patterns into ThreadLiner. Two apps, two codebases, same visual language. | Tactical |
| [`plan-refactor-common-foundation.md`](./plan-refactor-common-foundation.md) | Extract a shared `@laankit/desktop-ui` library. Two apps, one chrome library. | Structural |
| [`plan-refactor-modular-app.md`](./plan-refactor-modular-app.md) | Collapse both apps into one host with installable `noteliner` / `threadliner` modules. | Strategic |

Each plan is strictly larger than the previous one and depends on the
previous one's outcome.

## Side-by-side

| Dimension | Refresh UI | Common Foundation | Modular App |
|---|---|---|---|
| Effort | ~2 weeks | ~3–4 weeks (after Refresh) | ~10–13 weeks (after Foundation) |
| Risk | Low | Medium | High |
| User-visible win | ThreadLiner looks like a sibling of NoteLiner | Same as Refresh, but cheaper to keep aligned over time | One installer, one window, cross-module features |
| Reversibility | High — purely cosmetic | Medium — library is an internal dependency | Low — data migration tied to new shell |
| New product surface | None | None | Module manager, workspace switcher, cross-module UX |
| Maintenance burden after | Two near-identical chrome implementations to keep in sync by hand | One library + two consumers; library churn is amplified to two apps | One host + N modules; module contract is the load-bearing piece |
| Branding | NoteLiner / ThreadLiner | NoteLiner / ThreadLiner | Unified shell (TBD), modules marketed as features |

## Decision matrix

Pick by what you care about most.

- **"I want ThreadLiner to look modern, this quarter."** → Refresh UI.
- **"I want both apps to stop drifting from each other."** → Foundation.
- **"I want one product, and I'm willing to invest a quarter."** → Modular.

## Recommended approach

**Adopt all three plans, in sequence.** They are designed to compose. Each
one delivers value on its own and de-risks the next.

### Stage 1 — Ship `plan-refactor-refresh-ui.md` first (2 weeks)

Reasons:

1. **Highest immediate user-visible win for the smallest cost.** ThreadLiner
   today is functional but visually behind NoteLiner. The refresh closes
   that gap.
2. **It's a prerequisite for everything else.** The Svelte 5 upgrade and
   the variable-rename are unavoidable if either of the larger plans
   happens. Doing them in isolation (under a deliberately bounded scope)
   keeps the risk small and the diff readable.
3. **It validates the visual language.** If the NoteLiner UI patterns
   feel right inside ThreadLiner, that's evidence the library plan will
   pay off. If they don't, we learn that *before* extracting a library
   that bakes them in.

Do this even if the rest of the plan never happens. ThreadLiner benefits
either way.

### Stage 2 — Land `plan-refactor-common-foundation.md` second (3–4 weeks)

Conditional on the refresh going well, with a re-evaluation gate before
starting:

- Are the two codebases now genuinely similar?
- Are we maintaining both apps actively enough to feel the cost of
  divergence?
- Do we have a clean Svelte 5 + Vite + Electron toolchain in both?

If yes to all three, the library carve-out is the right move. It pays
back the second time you have to change a button radius and the third
time you fix a theme bug.

If we're not actively maintaining one of the apps, skip this stage — a
shared library has overhead that only makes sense when both consumers
move. A dormant ThreadLiner plus an active NoteLiner doesn't need a
library; it needs ThreadLiner to be archived or revived.

### Stage 3 — Evaluate `plan-refactor-modular-app.md` last, maybe never

The modular app is the *strategic* play. It is the right answer if and
only if:

- Both apps are getting active development and we want them to share
  more than chrome (sync engine, command palette, cross-module
  navigation).
- The user base is one that wants both — i.e., note-takers who also
  follow feeds. If the audiences don't overlap, the unified app gives
  each user a feature they don't want.
- We're willing to spend ~3 months and accept the reversibility risk.

If those conditions hold, do it. If not, the foundation library already
delivers the bulk of the "shared product" benefits at a fraction of the
cost.

## Practical next steps

1. **Read the three plans.** Send back any concerns, ideally with the
   word "blocker" so they're easy to surface.
2. **Decide on Svelte 4 vs Svelte 5 for ThreadLiner.** (See `plan-refactor-refresh-ui.md` §8.) The plans assume Svelte 5.
3. **Decide on sync model.** (See `plan-refactor-refresh-ui.md` §3.) Option A (hybrid: keep auto-sync, add manual controls) is the recommendation.
4. **Sign off on Stage 1.** Plan-refresh-ui is ready to start once those two decisions are made.

## What a "no" looks like

If none of these is the right move, the alternative is: leave ThreadLiner as
it is and invest in NoteLiner. ThreadLiner is a niche RSS reader; NoteLiner
is the larger product. If you'd rather pour cycles into NoteLiner
features, that's a perfectly reasonable answer and these plans should be
shelved, not partially executed.

## Cross-references

- [plan-refactor-refresh-ui.md](./plan-refactor-refresh-ui.md)
- [plan-refactor-common-foundation.md](./plan-refactor-common-foundation.md)
- [plan-refactor-modular-app.md](./plan-refactor-modular-app.md)
