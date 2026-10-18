---
title: Rich Note
tags: [spike, export]
created: 2026-10-17
---

Every construct NoteLiner's export has to handle. Typst-special characters
in plain text: # * _ ` $ [ ] < > @ = - + / ~ \ and "quotes", 'apostrophes',
an ellipsis... a -- dash, a URL-looking http://example.com/a_b*c, an
email-like a@b.c, and <label>-like text. Non-ASCII: café, naïve, 日本語,
Ελληνικά, emoji 😀.

## Inline formatting

Some *emphasis*, **strong**, ***both***, ~~strikethrough~~, `inline code with
#hash and *stars*`, a [link](https://example.com/path?q=1&r=2 "title"), an
autolink https://typst.app, and a hard line break here\
right here. Raw <b>inline HTML</b> and a<br>break.

Wikilinks: [[Project Plan]] and [[Real Name|a labelled link]].

Footnotes: first reference[^note], a second note[^two], and the first
again[^note].

[^note]: The footnote text, with **formatting** and `code`.
[^two]: Another footnote.

## Lists

- Bullet one
- Bullet two with a longer line that wraps across the page width so we can
  see how continuation lines align with the bullet text
  - Nested bullet
    - Third level
      1. Ordered inside bullets
      2. Second
- Bullet three

3. Ordered list starting at 3
4. Four

   With a second paragraph (loose item).

5. Five

Tasks:

- [ ] An open task
- [x] A finished task
- [ ] A task with `code` and a [link](https://example.com)
  - [x] A nested finished task

## Code

```rust
/// Doc comment
fn main() {
    let s = "string with \"escapes\" and \\ backslash";
    println!("{s} #{}", 42); // comment
}
```

```js
const f = (a, b) => a ?? b; // `template ${x}`
```

```
plain fence, no language: $ # [ ] * _
    indented line
```

## Block quote

> A quotation that spans
> several lines.
>
> > A nested quotation.

## Table

| Left | Centre | Right | Notes with `code` |
|:-----|:------:|------:|-------------------|
| a    | b      | 1.00  | **bold** cell     |
| long cell content that should wrap inside the column | c | 22.50 | [link](https://example.com) |
| x | | 333 | |

## Images

An attachment PNG:

![Gradient](./_attachments/att-gradient.png)

A wide PNG (must scale to the page width):

![Wide](_attachments/att-wide.png)

An SVG with a space in its name:

![Diagram](_attachments/My%20Diagram.svg)

A missing image ![Missing](_attachments/nope.png) and a remote one
![Remote](https://example.com/x.png).

---

<div class="raw-block">An HTML block's <em>text</em> survives.</div>

### Heading three

#### Heading four

Final paragraph.
