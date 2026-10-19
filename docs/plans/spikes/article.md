# Spike 2 — Article HTML, rendered natively

> **Archived Phase 0 record.** The `apps/spikes/*` crates this describes were removed on
> 2026-10-18 after being promoted into `crates/` and the native apps; their code is in git
> history (commit `9922753`). Commands below refer to those removed crates.

**Result: pass.** 64 of 64 real articles (100%) were readable without the
"Open in browser" fallback; the plan's bar is 90%. **Go for Phase 2
(ThreadLiner).**

## Method

- `spike-richtext` (shared with spike 4):
  1. Sanitizes with `ammonia`, configured with ThreadLiner's DOMPurify
     allow-list (`ContentViewer.svelte`: the same tags and attributes,
     `data-*` attributes, and the `<noscript><img>` unwrap).
  2. Parses with `scraper` (html5ever) into a `Vec<Block>`: paragraphs,
     headings, lists (nested, `start`), blockquotes, code, images (with
     lazy-load `data-src` and `srcset` handling), figures and captions,
     rules and tables. Inline spans carry bold, italic, underline, strike,
     code, small and a link.
  3. Lays the blocks out with egui `LayoutJob`s.
- **Rendering details:**
  - Links are clickable, with a pointer cursor and a URL tooltip.
  - Text is selectable.
  - Remote images load through egui_extras' HTTP loader; `data:` images
    are decoded locally. Image `title` text shows as a tooltip (xkcd).
- **Viewport culling.** Every block is measured once per width; after
  that, only blocks near the viewport are laid out.
- **Fonts.** egui's default fonts have no bold or italic, so the spike
  loads system faces (DejaVu/Liberation, Segoe UI/Georgia, Arial/Georgia).
  Phase 1 should bundle OFL fonts instead.
- **Corpus.** 64 entries from 16 feeds, fetched with
  `threadliner_core::feed`:
  - Rust Blog, Simon Willison, Julia Evans, LWN, Ars Technica
  - The Verge, Smashing Magazine, GitHub Blog, Martin Fowler, xkcd
  - Daring Fireball, Dan Luu, Kottke, Changelog (podcast)
  - Cloudflare, Raspberry Pi

  The YouTube feed contributed nothing: its entries carry no content
  field, so ThreadLiner shows nothing for them today either. The corpus is
  cached outside the repo.
- **Readability rule.** An article is readable if:
  - at least 90% of the source's visible text survives (non-whitespace
    characters), and
  - it isn't embed-only: an iframe, video or audio with fewer than 200
    characters of text and no image.

  That rule mostly catches lost text. It can't judge layout, so 14
  articles were also screenshotted and checked by eye: release notes with
  code, image-heavy posts, tables, quotes, xkcd, a podcast and an
  `svg`/`script` post.

```bash
SPIKE_ARTICLE_DIR=<scratch> cargo run --profile fast-release -p spike-article -- fetch
SPIKE_ARTICLE_DIR=<scratch> cargo run --profile fast-release -p spike-article -- report
# A screenshot of entry 46, scrolled to its first table:
MARINA_ARTICLE=46 MARINA_FIND=table MARINA_SCREENSHOT=shot.png MARINA_SCREENSHOT_EXIT=1 \
  cargo run --profile fast-release -p spike-article
```

## Numbers

Measured with `--profile fast-release` on Linux, in a 1000×800 headless egui
context, tessellation included.

| Measure | p50 | p95 | max |
|---|---|---|---|
| Sanitize + convert | 0.24 ms | 1.71 ms | 4.62 ms |
| First frame (measures every block) | 1.79 ms | 14.95 ms | 27.31 ms |
| Steady frame (culled) | 0.13 ms | 0.28 ms | 0.51 ms |
| Frame after jumping to the middle | 0.11 ms | 1.54 ms | 1.96 ms |

- **Text kept:** 100% for all 64 articles.
- **Dropped elements across the corpus:** 6 `svg`, 2 `script`, 1
  `iframe`. Inline SVG diagrams are lost, which is also true in
  ThreadLiner today, since DOMPurify strips them.
- **Over budget:** the first frame of the largest article (Dan Luu, 208
  blocks) is 27 ms. That's a one-off when the article opens.

## Gaps (to handle in Phase 2)

- **Slow first frame.** Long articles take 15–30 ms to open. Fix: measure
  in slices over a few frames, or measure off-screen while the list is
  still showing.
- **Tables.**
  - `colspan` is padded with empty cells, so cells line up but headers
    don't visually span columns.
  - `rowspan` is ignored.
  - Cells wrap at an even share of the width rather than by content.
- **CSS isn't applied to articles** (DOMPurify strips `style` too).
  Floats, side-by-side images and figure widths become a single column of
  centred images.
- **Inline SVG, MathML, iframes and video** aren't rendered. Text inside
  them is kept where the sanitizer keeps it (iframe fallback text). Video
  and podcast embeds need a "media" card that links out.
- **Images.**
  - No placeholder size while loading, so content can shift slightly when
    an image arrives above the viewport.
  - No `srcset` density choice: the first candidate is used.
- **Selection.** Text can be selected one block at a time (an egui
  `Label` limit). Selecting across paragraphs needs a custom selection
  layer.
- **Fonts.** No glyph fallback beyond egui's emoji font; `★` was missing
  in the entry list. Phase 1 needs a fallback chain (Noto, or system fonts
  via `system_fonts`).
- **Screenshots on Wayland.** `MARINA_SCREENSHOT` never fires on GNOME
  Wayland, because the unfocused window gets no frames. Run with
  `env -u WAYLAND_DISPLAY` (XWayland) for screenshots. CI on X11/Xvfb is
  unaffected.

## Go / no-go

**Go.** Conversion and layout are fast, nearly every real article reads
well, and nothing in the corpus needed a webview. The gaps above are
Phase 2 work, not blockers.
