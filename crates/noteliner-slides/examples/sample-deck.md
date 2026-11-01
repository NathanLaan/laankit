---
presentation:
  theme: dark
  aspect: "16:9"
  slideLevel: 2
  footer: "LaanKit native slides · {{page}} / {{total}}"
  slideNumbers: true
---
# Native Slides

Every slide drawn by egui, at any size

<!-- notes
Welcome everyone. This deck exercises every layout.
-->

## Agenda

- Why a native presenter
- **Layouts** inferred from the blocks
- Speaker notes and *directives*
- `code`, tables and pictures

## Two columns

The **split** layout puts two blocks side by side.

- Left is a paragraph
- Right is a list

## Four blocks make a grid

First block: a short paragraph.

Second block: another one.

> A quote in the grid

- One
- Two

## A quote slide

> Simplicity is prerequisite for reliability.

<!-- notes: Attribute this to Dijkstra. -->

## Numbers

| Quarter | Revenue | Growth |
|---------|---------|--------|
| Q1      | 1.2 M   | 4 %    |
| Q2      | 1.5 M   | 25 %   |
| Q3      | 1.9 M   | 27 %   |

## Code

```rust
fn main() {
    println!("Hello from a slide");
}
```

---

![A generated landscape](assets/landscape.png)

## A picture with a title

![A generated landscape](assets/landscape.png)

## Custom background
<!-- slide: bg=#3b2a5c header="Directives: bg and header" -->

Directives set a **background** and a header for one slide.

## Hidden slide
<!-- slide: notes-only -->

This slide stays in the note but not on the deck.

## Video

https://youtu.be/dQw4w9WgXcQ
