//! LaTeX-style math (the `$…$` and `$$…$$` of Markdown notes) → Typst math.
//!
//! Notes write math the way every Markdown tool does, in LaTeX syntax; Typst
//! has its own math syntax. [`latex_to_typst`] converts a documented subset
//! and refuses everything else ([`Unsupported`]), so callers fall back to the
//! source text instead of rendering something wrong. It needs neither Typst
//! nor egui: the PDF export emits its output as native Typst math, and the
//! app's preview and slides render it to images through
//! [`Exporter::math_svg`](crate::Exporter::math_svg).
//!
//! **Supported** (anything else is [`Unsupported`]):
//!
//! | LaTeX                                                        | Typst                         |
//! |--------------------------------------------------------------|-------------------------------|
//! | letters, digits, `+ - = < > ! ' * : . ? \|`, Unicode symbols | the same (letters spaced so `ab` isn't read as one name) |
//! | `x^2`, `x_{i+1}`, `x_i^2`                                    | `x^(2)`, `x_(i + 1)`          |
//! | `\frac{a}{b}`, `\dfrac`, `\tfrac`, `\cfrac`, `\binom{n}{k}`  | `frac(a, b)`, `binom(n, k)`   |
//! | `\sqrt{x}`, `\sqrt[n]{x}`                                    | `sqrt(x)`, `root(n, x)`       |
//! | Greek (`\alpha` … `\Omega`, `\varepsilon`, `\varphi`, …)     | `alpha` … `Omega`, `epsilon.alt`, … |
//! | relations and operators (`\le`, `\neq`, `\approx`, `\in`, `\subseteq`, `\cdot`, `\times`, `\pm`, `\to`, `\Rightarrow`, …) | `<=`, `!=`, `approx`, `in`, `subset.eq`, `dot.op`, `times`, `plus.minus`, `->`, `=>`, … |
//! | `\sum`, `\prod`, `\int`, `\iint`, `\oint`, `\bigcup`, `\lim`, `\max`, … with `_` / `^` limits | `sum`, `product`, `integral`, … |
//! | `\sin`, `\log`, `\exp`, … and `\operatorname{name}`          | `sin`, `log`, `op("name")`    |
//! | `\left( … \right)` with `( [ \{ \| \langle \lfloor \lceil .` | `lr(( … ))`                   |
//! | `pmatrix`, `bmatrix`, `Bmatrix`, `vmatrix`, `Vmatrix`, `matrix`, `cases`, `aligned`, `align(*)`, `gathered`, `split` | `mat(delim: "(", a, b; c, d)`, `cases(…)`, `&` / `\` |
//! | `\text{…}`, `\mathrm`, `\mathbf`, `\mathit`, `\mathsf`, `\mathtt`, `\mathbb`, `\mathcal`, `\mathfrak`, `\boldsymbol` | `"…"`, `upright(…)`, `bold(…)`, … `bb(…)`, `cal(…)`, `frak(…)` |
//! | accents `\hat`, `\bar`, `\tilde`, `\vec`, `\dot`, `\ddot`, `\overline`, `\underline`, `\widehat`, `\widetilde`, `\overbrace`, `\underbrace` | `hat(…)`, `macron(…)`, … |
//! | `\overset{a}{b}`, `\underset{a}{b}`, `\stackrel{a}{b}`       | `limits(b)^(a)` / `limits(b)_(a)` |
//! | spacing `\, \: \; \quad \qquad \!`, `~`, line breaks `\\`, `&` | `thin`, `med`, `thick`, `quad`, `wide`; `\`; `&` |
//! | `\big(` … `\Bigg)` sizes, `\displaystyle`, `\textstyle`, `\limits` | dropped (Typst sizes these itself) |
//!
//! Not supported: `\color`, `\boxed`, `array` (column specs), `\newcommand`,
//! `\label`/`\tag`, `%` comments, and any other command.

use std::fmt;

/// Why a formula couldn't be converted (callers show its source instead).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported(pub String);

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported math: {}", self.0)
    }
}

impl std::error::Error for Unsupported {}

/// The longest formula converted; longer ones are refused (a guard for the
/// renderer, which compiles each formula).
pub const MAX_SOURCE_LEN: usize = 8 * 1024;
/// How deeply groups may nest.
const MAX_DEPTH: usize = 64;

/// Converts LaTeX math (without the `$` delimiters) to Typst math (also
/// without delimiters). The result is meant to sit inside `$…$` (inline) or
/// `$ … $` (display); see [`typst_equation`].
pub fn latex_to_typst(latex: &str) -> Result<String, Unsupported> {
    if latex.len() > MAX_SOURCE_LEN {
        return Err(Unsupported("formula too long".into()));
    }
    let tokens = tokenize(latex)?;
    let mut p = Parser {
        tokens,
        pos: 0,
        depth: 0,
    };
    let out = p.sequence(Stop::End)?;
    let out = out.trim().to_string();
    if out.is_empty() {
        return Err(Unsupported("empty formula".into()));
    }
    Ok(out)
}

/// A whole Typst equation: `$x$` inline, `$ x $` on its own line.
pub fn typst_equation(latex: &str, display: bool) -> Result<String, Unsupported> {
    let body = latex_to_typst(latex)?;
    Ok(if display {
        format!("$ {body} $")
    } else {
        format!("${body}$")
    })
}

/// Typst draws radical and accent bars as stroked straight lines
/// (`<path fill="none" stroke="…" stroke-width="w" … d="M 0 0h 21.16"/>`).
/// A path with no height has an empty bounding box, which SVG rasterizers
/// such as `resvg` skip, so the bar of a square root goes missing. This turns
/// each such horizontal or vertical line into a filled rectangle of the
/// stroke's width, with the same transform.
pub fn straight_strokes_as_rects(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find("<path ") {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find("/>") else {
            break;
        };
        let tag = &rest[start..start + len + 2];
        out.push_str(&line_as_rect(tag).unwrap_or_else(|| tag.to_string()));
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

fn line_as_rect(tag: &str) -> Option<String> {
    let attr = |name: &str| -> Option<&str> {
        let key = format!(" {name}=\"");
        let i = tag.find(&key)? + key.len();
        Some(&tag[i..i + tag[i..].find('"')?])
    };
    if attr("fill") != Some("none") {
        return None;
    }
    let stroke = attr("stroke")?;
    let width: f32 = attr("stroke-width")?.parse().ok()?;
    let d = attr("d")?.trim();
    let rest = d.strip_prefix("M 0 0")?.trim();
    let (w, h) = if let Some(n) = rest.strip_prefix('h') {
        let len: f32 = n.trim().parse().ok()?;
        ((len.min(0.0), -width / 2.0), (len.abs(), width))
    } else {
        let len: f32 = rest.strip_prefix('v')?.trim().parse().ok()?;
        ((-width / 2.0, len.min(0.0)), (width, len.abs()))
    };
    let transform = attr("transform")
        .map(|t| format!(" transform=\"{t}\""))
        .unwrap_or_default();
    Some(format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{stroke}\"{transform}/>",
        w.0, w.1, h.0, h.1
    ))
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// `\name` (letters) or `\` + one other character (`\,`, `\{`, `\\`).
    Cmd(String),
    Open,
    Close,
    Sup,
    Sub,
    Amp,
    /// A run of digits (with one decimal point).
    Number(String),
    /// Any other single character (letters included).
    Char(char),
    Space,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, Unsupported> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                i += 1;
                let Some(&next) = chars.get(i) else {
                    return Err(Unsupported("trailing backslash".into()));
                };
                if next.is_ascii_alphabetic() {
                    let start = i;
                    while i < chars.len() && chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let mut name: String = chars[start..i].iter().collect();
                    // `\operatorname*`, `\align*`-style stars belong to the command.
                    if chars.get(i) == Some(&'*') && name == "operatorname" {
                        name.push('*');
                        i += 1;
                    }
                    out.push(Tok::Cmd(name));
                } else {
                    out.push(Tok::Cmd(next.to_string()));
                    i += 1;
                }
            }
            '{' => {
                out.push(Tok::Open);
                i += 1;
            }
            '}' => {
                out.push(Tok::Close);
                i += 1;
            }
            '^' => {
                out.push(Tok::Sup);
                i += 1;
            }
            '_' => {
                out.push(Tok::Sub);
                i += 1;
            }
            '&' => {
                out.push(Tok::Amp);
                i += 1;
            }
            '%' => return Err(Unsupported("% comment".into())),
            '$' => return Err(Unsupported("nested $".into())),
            '#' => return Err(Unsupported("# parameter".into())),
            c if c.is_ascii_digit() => {
                let start = i;
                let mut dot = false;
                while i < chars.len() {
                    let d = chars[i];
                    if d.is_ascii_digit() {
                        i += 1;
                    } else if d == '.'
                        && !dot
                        && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit())
                    {
                        dot = true;
                        i += 1;
                    } else {
                        break;
                    }
                }
                out.push(Tok::Number(chars[start..i].iter().collect()));
            }
            c if c.is_whitespace() => {
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                out.push(Tok::Space);
            }
            c if c.is_control() => return Err(Unsupported("control character".into())),
            c => {
                out.push(Tok::Char(c));
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Where a [`Parser::sequence`] ends.
#[derive(Clone, Copy, PartialEq)]
enum Stop {
    End,
    /// The `}` of a group.
    Group,
    /// `]` (the index of `\sqrt[n]`).
    Bracket,
    /// `\right`.
    Right,
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(&Tok::Space) {
            self.pos += 1;
        }
    }

    fn enter(&mut self) -> Result<(), Unsupported> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Unsupported("nested too deeply".into()));
        }
        Ok(())
    }

    /// Items until `stop`, which is consumed (or the end of input for
    /// [`Stop::End`]). Each item is followed by a space, so adjacent letters
    /// stay separate identifiers in Typst.
    fn sequence(&mut self, stop: Stop) -> Result<String, Unsupported> {
        self.enter()?;
        let mut out = String::new();
        loop {
            match (self.peek(), stop) {
                (None, Stop::End) => break,
                (None, _) => return Err(Unsupported("unclosed group".into())),
                (Some(Tok::Close), Stop::Group) => {
                    self.pos += 1;
                    break;
                }
                (Some(Tok::Close), _) => return Err(Unsupported("unbalanced }".into())),
                (Some(Tok::Char(']')), Stop::Bracket) => {
                    self.pos += 1;
                    break;
                }
                (Some(Tok::Cmd(c)), Stop::Right) if c == "right" => {
                    self.pos += 1;
                    break;
                }
                _ => {}
            }
            let item = self.item()?;
            if !item.is_empty() {
                out.push_str(&item);
                out.push(' ');
            }
        }
        self.depth -= 1;
        Ok(out)
    }

    /// One item with any `^` / `_` attached.
    fn item(&mut self) -> Result<String, Unsupported> {
        let base = self.atom()?;
        if base.is_empty() {
            return Ok(base);
        }
        let mut out = base;
        // `f'` and `f''`: primes attach to their base (a space would detach them in Typst).
        while self.peek() == Some(&Tok::Char('\'')) {
            self.pos += 1;
            out.push('\'');
        }
        loop {
            self.skip_spaces_before_script();
            match self.peek() {
                Some(Tok::Sup) => {
                    self.pos += 1;
                    let arg = self.argument()?;
                    out = format!("{out}^({arg})");
                }
                Some(Tok::Sub) => {
                    self.pos += 1;
                    let arg = self.argument()?;
                    out = format!("{out}_({arg})");
                }
                Some(Tok::Cmd(c)) if c == "limits" || c == "nolimits" => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
        Ok(out)
    }

    /// Spaces between a base and its `^`/`_` don't separate them in LaTeX.
    fn skip_spaces_before_script(&mut self) {
        let mut p = self.pos;
        while self.tokens.get(p) == Some(&Tok::Space) {
            p += 1;
        }
        if matches!(self.tokens.get(p), Some(Tok::Sup | Tok::Sub)) {
            self.pos = p;
        }
    }

    /// A required argument: a `{group}`, or the next single token.
    fn argument(&mut self) -> Result<String, Unsupported> {
        self.skip_spaces();
        match self.peek() {
            Some(Tok::Open) => {
                self.pos += 1;
                Ok(self.sequence(Stop::Group)?.trim().to_string())
            }
            Some(Tok::Number(n)) => {
                // `x^23` is `x^{2}3` in LaTeX: a script takes one digit, and
                // the rest stays as the next token.
                let mut chars = n.chars();
                let first = chars.next().unwrap_or('0');
                let rest: String = chars.collect();
                if rest.is_empty() {
                    self.pos += 1;
                } else {
                    self.tokens[self.pos] = Tok::Number(rest);
                }
                Ok(first.to_string())
            }
            Some(Tok::Sup | Tok::Sub | Tok::Amp | Tok::Close) | None => {
                Err(Unsupported("missing argument".into()))
            }
            _ => self.atom(),
        }
    }

    /// One atom (no scripts).
    fn atom(&mut self) -> Result<String, Unsupported> {
        let Some(tok) = self.next() else {
            return Err(Unsupported("unexpected end".into()));
        };
        Ok(match tok {
            Tok::Space => String::new(),
            Tok::Open => format!("({})", self.sequence(Stop::Group)?.trim()),
            Tok::Close => return Err(Unsupported("unbalanced }".into())),
            Tok::Sup | Tok::Sub => return Err(Unsupported("script without a base".into())),
            Tok::Amp => "&".into(),
            Tok::Number(n) => n,
            Tok::Char(c) => char_atom(c)?,
            Tok::Cmd(name) => self.command(&name)?,
        })
    }

    fn command(&mut self, name: &str) -> Result<String, Unsupported> {
        if let Some(sym) = symbol(name) {
            return Ok(sym.to_string());
        }
        Ok(match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let a = self.argument()?;
                let b = self.argument()?;
                format!("frac({}, {})", arg(&a), arg(&b))
            }
            "binom" | "dbinom" | "tbinom" => {
                let a = self.argument()?;
                let b = self.argument()?;
                format!("binom({}, {})", arg(&a), arg(&b))
            }
            "sqrt" => {
                self.skip_spaces();
                if self.peek() == Some(&Tok::Char('[')) {
                    self.pos += 1;
                    let n = self.sequence(Stop::Bracket)?;
                    let x = self.argument()?;
                    format!("root({}, {})", arg(n.trim()), arg(&x))
                } else {
                    let x = self.argument()?;
                    format!("sqrt({})", arg(&x))
                }
            }
            "text" | "textrm" | "textnormal" | "mbox" | "textup" => {
                format!("\"{}\"", string_escape(&self.group_text()?))
            }
            "textbf" => format!("bold(\"{}\")", string_escape(&self.group_text()?)),
            "textit" => format!("italic(\"{}\")", string_escape(&self.group_text()?)),
            "texttt" => format!("mono(\"{}\")", string_escape(&self.group_text()?)),
            "operatorname" => format!("op(\"{}\")", string_escape(&self.group_text()?)),
            "operatorname*" => format!(
                "op(\"{}\", limits: #true)",
                string_escape(&self.group_text()?)
            ),
            "mathrm" | "rm" => self.wrap("upright")?,
            "mathbf" | "bf" | "boldsymbol" | "bm" => self.wrap("bold")?,
            "mathit" | "it" => self.wrap("italic")?,
            "mathsf" => self.wrap("sans")?,
            "mathtt" => self.wrap("mono")?,
            "mathbb" => self.wrap("bb")?,
            "mathcal" => self.wrap("cal")?,
            "mathfrak" => self.wrap("frak")?,
            "hat" | "widehat" => self.wrap("hat")?,
            "bar" => self.wrap("macron")?,
            "tilde" | "widetilde" => self.wrap("tilde")?,
            "vec" => self.wrap("arrow")?,
            "dot" => self.wrap("dot")?,
            "ddot" => self.wrap("dot.double")?,
            "acute" => self.wrap("acute")?,
            "grave" => self.wrap("grave")?,
            "breve" => self.wrap("breve")?,
            "check" => self.wrap("caron")?,
            "overline" => self.wrap("overline")?,
            "underline" => self.wrap("underline")?,
            "overbrace" | "underbrace" => {
                let x = self.argument()?;
                let script = if name == "overbrace" {
                    Tok::Sup
                } else {
                    Tok::Sub
                };
                self.skip_spaces_before_script();
                if self.peek() == Some(&script) {
                    self.pos += 1;
                    let t = self.argument()?;
                    format!("{name}({}, {})", arg(&x), arg(&t))
                } else {
                    format!("{name}({})", arg(&x))
                }
            }
            "overset" | "stackrel" | "underset" => {
                let over = self.argument()?;
                let base = self.argument()?;
                let s = if name == "underset" { "_" } else { "^" };
                format!("limits({}){s}({over})", arg(&base))
            }
            "left" => self.left_right()?,
            "right" => return Err(Unsupported("\\right without \\left".into())),
            "begin" => self.environment()?,
            "end" => return Err(Unsupported("\\end without \\begin".into())),
            "pmod" => format!("(mod {})", self.argument()?),
            "not" => {
                self.skip_spaces();
                match self.next() {
                    Some(Tok::Char('=')) => "!=".into(),
                    Some(Tok::Cmd(c)) if c == "in" => "in.not".into(),
                    Some(Tok::Cmd(c)) if c == "subset" => "subset.not".into(),
                    Some(Tok::Cmd(c)) if c == "equiv" => "equiv.not".into(),
                    _ => return Err(Unsupported("\\not".into())),
                }
            }
            // Sizes and styles Typst decides itself.
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl"
            | "biggr" | "Biggl" | "Biggr" | "displaystyle" | "textstyle" | "scriptstyle"
            | "limits" | "nolimits" | "!" => String::new(),
            other => return Err(Unsupported(format!("\\{other}"))),
        })
    }

    /// `name(argument)`.
    fn wrap(&mut self, func: &str) -> Result<String, Unsupported> {
        let x = self.argument()?;
        Ok(format!("{func}({})", arg(&x)))
    }

    /// The raw text of a `{…}` group (`\text`, environment names).
    fn group_text(&mut self) -> Result<String, Unsupported> {
        self.skip_spaces();
        if self.next() != Some(Tok::Open) {
            return Err(Unsupported("expected {".into()));
        }
        let mut depth = 0;
        let mut out = String::new();
        loop {
            match self.next() {
                None => return Err(Unsupported("unclosed {".into())),
                Some(Tok::Open) => {
                    depth += 1;
                    out.push('{');
                }
                Some(Tok::Close) if depth == 0 => break,
                Some(Tok::Close) => {
                    depth -= 1;
                    out.push('}');
                }
                Some(Tok::Space) => out.push(' '),
                Some(Tok::Number(n)) => out.push_str(&n),
                Some(Tok::Char(c)) => out.push(c),
                Some(Tok::Sup) => out.push('^'),
                Some(Tok::Sub) => out.push('_'),
                Some(Tok::Amp) => out.push('&'),
                Some(Tok::Cmd(c)) => match c.as_str() {
                    "{" | "}" | "$" | "%" | "&" | "_" | "#" => out.push_str(&c),
                    "\\" => out.push(' '),
                    " " | "," | ";" | ":" => out.push(' '),
                    _ => return Err(Unsupported(format!("\\{c} in text"))),
                },
            }
        }
        Ok(out)
    }

    /// `\left<d> … \right<d>` → `lr(… )`.
    fn left_right(&mut self) -> Result<String, Unsupported> {
        let open = self.delimiter()?;
        let body = self.sequence(Stop::Right)?;
        let close = self.delimiter()?;
        Ok(format!("lr({open} {} {close})", body.trim()))
    }

    fn delimiter(&mut self) -> Result<&'static str, Unsupported> {
        self.skip_spaces();
        Ok(match self.next() {
            Some(Tok::Char('(')) => "(",
            Some(Tok::Char(')')) => ")",
            Some(Tok::Char('[')) => "[",
            Some(Tok::Char(']')) => "]",
            Some(Tok::Char('|')) => "|",
            Some(Tok::Char('.')) => "",
            Some(Tok::Char('<')) => "chevron.l",
            Some(Tok::Char('>')) => "chevron.r",
            Some(Tok::Char('/')) => "slash",
            Some(Tok::Cmd(c)) => match c.as_str() {
                "{" | "lbrace" => "\\{",
                "}" | "rbrace" => "\\}",
                "|" | "Vert" | "lVert" | "rVert" => "‖",
                "vert" | "lvert" | "rvert" => "|",
                "langle" => "chevron.l",
                "rangle" => "chevron.r",
                "lfloor" => "floor.l",
                "rfloor" => "floor.r",
                "lceil" => "ceil.l",
                "rceil" => "ceil.r",
                "lbrack" => "[",
                "rbrack" => "]",
                _ => return Err(Unsupported(format!("delimiter \\{c}"))),
            },
            _ => return Err(Unsupported("delimiter".into())),
        })
    }

    /// `\begin{env} … \end{env}`.
    fn environment(&mut self) -> Result<String, Unsupported> {
        let name = self.group_text()?;
        let (env, kind) = match name.as_str() {
            "matrix" => ("matrix", Env::Matrix("#none")),
            "pmatrix" => ("pmatrix", Env::Matrix("\"(\"")),
            "bmatrix" => ("bmatrix", Env::Matrix("\"[\"")),
            "Bmatrix" => ("Bmatrix", Env::Matrix("\"{\"")),
            "vmatrix" => ("vmatrix", Env::Matrix("\"|\"")),
            "Vmatrix" => ("Vmatrix", Env::Matrix("\"‖\"")),
            "smallmatrix" => ("smallmatrix", Env::Matrix("#none")),
            "cases" => ("cases", Env::Cases),
            "aligned" => ("aligned", Env::Lines),
            "align" => ("align", Env::Lines),
            "align*" => ("align*", Env::Lines),
            "gathered" => ("gathered", Env::Lines),
            "gather" => ("gather", Env::Lines),
            "gather*" => ("gather*", Env::Lines),
            "split" => ("split", Env::Lines),
            "equation" => ("equation", Env::Lines),
            "equation*" => ("equation*", Env::Lines),
            other => return Err(Unsupported(format!("environment {other}"))),
        };
        let rows = self.rows(env)?;
        Ok(match kind {
            Env::Matrix(delim) => {
                let rows: Vec<String> = rows
                    .iter()
                    .map(|cells| cells.iter().map(|c| arg(c)).collect::<Vec<_>>().join(", "))
                    .collect();
                format!("mat(delim: {delim}, {})", rows.join("; "))
            }
            Env::Cases => {
                let rows: Vec<String> = rows
                    .iter()
                    .map(|cells| arg(cells.join(" & ").trim()))
                    .collect();
                format!("cases({})", rows.join(", "))
            }
            Env::Lines => rows
                .iter()
                .map(|cells| cells.join(" & ").trim().to_string())
                .collect::<Vec<_>>()
                .join(" \\ "),
        })
    }

    /// The cells of an environment's body, by row (`\\`) and column (`&`).
    fn rows(&mut self, env: &str) -> Result<Vec<Vec<String>>, Unsupported> {
        self.enter()?;
        let mut rows = vec![vec![String::new()]];
        loop {
            match self.peek() {
                None => return Err(Unsupported(format!("unclosed {env}"))),
                Some(Tok::Cmd(c)) if c == "end" => {
                    self.pos += 1;
                    let name = self.group_text()?;
                    if name != env {
                        return Err(Unsupported(format!("\\end{{{name}}} for {env}")));
                    }
                    break;
                }
                Some(Tok::Cmd(c)) if c == "\\" => {
                    self.pos += 1;
                    rows.push(vec![String::new()]);
                }
                Some(Tok::Amp) => {
                    self.pos += 1;
                    rows.last_mut().expect("a row").push(String::new());
                }
                Some(Tok::Close) => return Err(Unsupported("unbalanced }".into())),
                _ => {
                    let item = self.item()?;
                    if !item.is_empty() {
                        let cell = rows.last_mut().and_then(|r| r.last_mut()).expect("a cell");
                        cell.push_str(&item);
                        cell.push(' ');
                    }
                }
            }
        }
        self.depth -= 1;
        // A trailing `\\` leaves an empty last row.
        if rows
            .last()
            .is_some_and(|r| r.len() == 1 && r[0].trim().is_empty())
            && rows.len() > 1
        {
            rows.pop();
        }
        Ok(rows
            .into_iter()
            .map(|r| r.into_iter().map(|c| c.trim().to_string()).collect())
            .collect())
    }
}

enum Env {
    Matrix(&'static str),
    Cases,
    Lines,
}

/// A function argument: commas and semicolons inside it are escaped so
/// they don't split it. (Escaping them everywhere keeps the output simple:
/// `\,` is a literal comma in Typst math.)
fn arg(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        "\"\"".into()
    } else {
        s.to_string()
    }
}

fn string_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// A single character as Typst math.
fn char_atom(c: char) -> Result<String, Unsupported> {
    Ok(match c {
        // Typst gives these meaning (fraction, comment, code, strings,
        // function arguments, labels); LaTeX means the plain glyph.
        '/' => "slash".into(),
        ',' => "\\,".into(),
        ';' => "\\;".into(),
        '"' => "\\\"".into(),
        '@' => "\\@".into(),
        '[' => "\\[".into(),
        ']' => "\\]".into(),
        '(' => "\\(".into(),
        ')' => "\\)".into(),
        '~' => " ".into(),
        '`' => "\\`".into(),
        '<' => "lt".into(),
        '>' => "gt".into(),
        '|' => "bar.v".into(),
        '*' => "ast".into(),
        '-' => "minus".into(),
        '+' => "plus".into(),
        '=' => "eq".into(),
        '!' => "excl".into(),
        ':' => "colon".into(),
        '\'' => "prime".into(),
        '.' => ".".into(),
        '?' => "?".into(),
        c if c.is_alphabetic() => c.to_string(),
        c if c.is_ascii() => return Err(Unsupported(format!("character {c:?}"))),
        // Other Unicode (Greek, arrows, …) means itself.
        c => c.to_string(),
    })
}

/// LaTeX symbol commands that are one Typst symbol.
fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        // Greek, lower case.
        "alpha" => "alpha",
        "beta" => "beta",
        "gamma" => "gamma",
        "delta" => "delta",
        "epsilon" => "epsilon",
        "varepsilon" => "epsilon.alt",
        "zeta" => "zeta",
        "eta" => "eta",
        "theta" => "theta",
        "vartheta" => "theta.alt",
        "iota" => "iota",
        "kappa" => "kappa",
        "lambda" => "lambda",
        "mu" => "mu",
        "nu" => "nu",
        "xi" => "xi",
        "omicron" => "omicron",
        "pi" => "pi",
        "varpi" => "pi.alt",
        "rho" => "rho",
        "varrho" => "rho.alt",
        "sigma" => "sigma",
        "varsigma" => "sigma.alt",
        "tau" => "tau",
        "upsilon" => "upsilon",
        "phi" => "phi.alt",
        "varphi" => "phi",
        "chi" => "chi",
        "psi" => "psi",
        "omega" => "omega",
        // Greek, upper case.
        "Gamma" => "Gamma",
        "Delta" => "Delta",
        "Theta" => "Theta",
        "Lambda" => "Lambda",
        "Xi" => "Xi",
        "Pi" => "Pi",
        "Sigma" => "Sigma",
        "Upsilon" => "Upsilon",
        "Phi" => "Phi",
        "Psi" => "Psi",
        "Omega" => "Omega",
        // Relations.
        "le" | "leq" => "<=",
        "ge" | "geq" => ">=",
        "ne" | "neq" => "!=",
        "ll" => "<<",
        "gg" => ">>",
        "approx" => "approx",
        "equiv" => "equiv",
        "sim" => "tilde.op",
        "simeq" => "tilde.eq",
        "cong" => "tilde.equiv",
        "propto" => "prop",
        "in" => "in",
        "notin" => "in.not",
        "ni" => "in.rev",
        "subset" => "subset",
        "subseteq" => "subset.eq",
        "supset" => "supset",
        "supseteq" => "supset.eq",
        "perp" => "perp",
        "parallel" => "parallel",
        "mid" => "divides",
        "vdash" => "tack.r",
        "models" => "models",
        "prec" => "prec",
        "succ" => "succ",
        "preceq" => "prec.eq",
        "succeq" => "succ.eq",
        // Operators.
        "pm" => "plus.minus",
        "mp" => "minus.plus",
        "times" => "times",
        "div" => "div",
        "cdot" => "dot.op",
        "ast" => "ast",
        "star" => "star",
        "circ" => "compose",
        "bullet" => "bullet",
        "oplus" => "plus.o",
        "ominus" => "minus.o",
        "otimes" => "times.o",
        "odot" => "dot.o",
        "cup" => "union",
        "cap" => "inter",
        "setminus" => "without",
        "wedge" | "land" => "and",
        "vee" | "lor" => "or",
        "neg" | "lnot" => "not",
        // Arrows.
        "to" | "rightarrow" => "->",
        "gets" | "leftarrow" => "<-",
        "leftrightarrow" => "<->",
        "Rightarrow" | "implies" => "=>",
        "Leftarrow" => "arrow.l.double",
        "Leftrightarrow" | "iff" => "<=>",
        "mapsto" => "|->",
        "longrightarrow" => "-->",
        "longleftarrow" => "<--",
        "Longrightarrow" => "==>",
        "uparrow" => "arrow.t",
        "downarrow" => "arrow.b",
        "hookrightarrow" => "arrow.r.hook",
        // Big operators.
        "sum" => "sum",
        "prod" => "product",
        "coprod" => "product.co",
        "int" => "integral",
        "iint" => "integral.double",
        "iiint" => "integral.triple",
        "oint" => "integral.cont",
        "bigcup" => "union.big",
        "bigcap" => "inter.big",
        "bigoplus" => "plus.o.big",
        "bigotimes" => "times.o.big",
        "bigwedge" => "and.big",
        "bigvee" => "or.big",
        // Named operators.
        "lim" => "lim",
        "limsup" => "limsup",
        "liminf" => "liminf",
        "max" => "max",
        "min" => "min",
        "sup" => "sup",
        "inf" => "inf",
        "det" => "det",
        "gcd" => "gcd",
        "lcm" => "lcm",
        "log" => "log",
        "lg" => "lg",
        "ln" => "ln",
        "exp" => "exp",
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "cot" => "cot",
        "sec" => "sec",
        "csc" => "csc",
        "arcsin" => "arcsin",
        "arccos" => "arccos",
        "arctan" => "arctan",
        "sinh" => "sinh",
        "cosh" => "cosh",
        "tanh" => "tanh",
        "coth" => "coth",
        "arg" => "arg",
        "deg" => "deg",
        "dim" => "dim",
        "ker" => "ker",
        "hom" => "hom",
        "Pr" => "Pr",
        "mod" | "bmod" => "mod",
        // Letters and misc.
        "infty" => "infinity",
        "partial" => "partial",
        "nabla" => "nabla",
        "forall" => "forall",
        "exists" => "exists",
        "nexists" => "exists.not",
        "emptyset" | "varnothing" => "emptyset",
        "ell" => "ell",
        "hbar" => "planck",
        "aleph" => "aleph",
        "Re" => "Re",
        "Im" => "Im",
        "wp" => "pee",
        "angle" => "angle",
        "triangle" => "triangle.stroked.t",
        "top" => "top",
        "bot" => "bot",
        "prime" => "prime",
        "degree" => "degree",
        "dagger" => "dagger",
        "ldots" | "dots" | "dotsc" => "...",
        "cdots" | "dotsb" => "dots.c",
        "vdots" => "dots.v",
        "ddots" => "dots.down",
        "langle" => "chevron.l",
        "rangle" => "chevron.r",
        "lfloor" => "floor.l",
        "rfloor" => "floor.r",
        "lceil" => "ceil.l",
        "rceil" => "ceil.r",
        "vert" | "lvert" | "rvert" => "bar.v",
        "Vert" | "lVert" | "rVert" | "|" => "bar.v.double",
        // Escaped characters.
        "{" | "lbrace" => "\\{",
        "}" | "rbrace" => "\\}",
        "%" => "%",
        "&" => "\\&",
        "_" => "\\_",
        "$" => "\\$",
        "#" => "\\#",
        // Spacing and breaks.
        "," | "thinspace" => "thin",
        ":" | ">" | "medspace" => "med",
        ";" | "thickspace" => "thick",
        "quad" => "quad",
        "qquad" => "wide",
        " " => "space",
        "\\" | "newline" | "cr" => "\\",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> String {
        latex_to_typst(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn letters_stay_separate_and_scripts_group() {
        assert_eq!(t("ab"), "a b");
        assert_eq!(t("x^2"), "x^(2)");
        assert_eq!(t("x_{i+1}"), "x_(i plus 1)");
        assert_eq!(t("x_i^2"), "x_(i)^(2)");
        assert_eq!(t("e^{i\\pi}"), "e^(i pi)");
        assert_eq!(t("3.14"), "3.14");
    }

    #[test]
    fn fractions_roots_binomials() {
        assert_eq!(t("\\frac{a}{b}"), "frac(a, b)");
        assert_eq!(t("\\frac12"), "frac(1, 2)");
        assert_eq!(t("\\sqrt{x}"), "sqrt(x)");
        assert_eq!(t("\\sqrt[3]{x}"), "root(3, x)");
        assert_eq!(t("\\binom{n}{k}"), "binom(n, k)");
    }

    #[test]
    fn symbols_and_big_operators() {
        assert_eq!(t("\\alpha \\le \\beta"), "alpha <= beta");
        assert_eq!(t("\\sum_{i=1}^{n} i"), "sum_(i eq 1)^(n) i");
        assert_eq!(t("\\int_0^\\infty"), "integral_(0)^(infinity)");
        assert_eq!(t("\\lim_{x \\to 0}"), "lim_(x -> 0)");
        assert_eq!(t("a, b; c"), "a \\, b \\; c");
        assert_eq!(t("a/b"), "a slash b");
    }

    #[test]
    fn delimiters_text_and_styles() {
        assert_eq!(t("\\left( x \\right)"), "lr(( x ))");
        assert_eq!(t("\\left\\{ x \\right."), "lr(\\{ x )");
        assert_eq!(t("\\text{if } x"), "\"if \" x");
        assert_eq!(t("\\mathbb{R}"), "bb(R)");
        assert_eq!(t("\\mathcal{A}"), "cal(A)");
        assert_eq!(t("\\hat{x} + \\vec{v}"), "hat(x) plus arrow(v)");
        assert_eq!(t("\\operatorname{rank}"), "op(\"rank\")");
    }

    #[test]
    fn environments() {
        assert_eq!(
            t("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}"),
            "mat(delim: \"(\", a, b; c, d)"
        );
        assert_eq!(
            t("\\begin{bmatrix}1&0\\\\0&1\\\\\\end{bmatrix}"),
            "mat(delim: \"[\", 1, 0; 0, 1)"
        );
        assert_eq!(
            t("f(x) = \\begin{cases} 1 & x > 0 \\\\ 0 & \\text{otherwise} \\end{cases}"),
            "f \\( x \\) eq cases(1 & x gt 0, 0 & \"otherwise\")"
        );
        assert_eq!(
            t("\\begin{aligned} a &= b \\\\ &= c \\end{aligned}"),
            "a & eq b \\ & eq c"
        );
    }

    #[test]
    fn unsupported_falls_back() {
        for s in [
            "\\color{red}{x}",
            "\\boxed{x}",
            "\\begin{array}{cc} a \\end{array}",
            "\\newcommand{\\x}{y}",
            "x % comment",
            "\\frac{a}",
            "{x",
            "x}",
            "\\left( x",
            "\\begin{pmatrix} a \\end{bmatrix}",
            "",
            "   ",
            "\\unknowncommand",
        ] {
            assert!(latex_to_typst(s).is_err(), "{s} should be unsupported");
        }
    }

    #[test]
    fn straight_strokes_become_rects() {
        let svg = r##"<g><path fill="none" stroke="#ffffff" stroke-width="1.6" stroke-linecap="butt" transform="translate(34.32 -0.3)" d="M 0 0h 21.16"/><path d="M1 2L3 4"/></g>"##;
        let out = straight_strokes_as_rects(svg);
        assert_eq!(
            out,
            r##"<g><rect x="0" y="-0.8" width="21.16" height="1.6" fill="#ffffff" transform="translate(34.32 -0.3)"/><path d="M1 2L3 4"/></g>"##
        );
        let v = r##"<path fill="none" stroke="#fff" stroke-width="2" d="M 0 0v -10"/>"##;
        assert_eq!(
            straight_strokes_as_rects(v),
            r##"<rect x="-1" y="-10" width="2" height="10" fill="#fff"/>"##
        );
    }

    #[test]
    fn never_panics_on_garbage() {
        let pieces = [
            "\\",
            "{",
            "}",
            "^",
            "_",
            "&",
            "\\\\",
            "\\frac",
            "\\sqrt",
            "[",
            "]",
            "\\left",
            "\\right",
            "(",
            ")",
            "\\begin{pmatrix}",
            "\\end{pmatrix}",
            "\\text{",
            "x",
            "1",
            " ",
            "\\alpha",
            "/",
            ",",
            "\"",
            "#",
            "%",
            "$",
            "é",
            "\u{1F600}",
            "\\not",
            "\\overbrace",
            ".",
        ];
        let mut seed = 0x2545F4914F6CDD1Du64;
        for _ in 0..20_000 {
            let mut s = String::new();
            for _ in 0..(seed % 12) {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                s.push_str(pieces[(seed % pieces.len() as u64) as usize]);
            }
            let _ = latex_to_typst(&s);
        }
    }
}
