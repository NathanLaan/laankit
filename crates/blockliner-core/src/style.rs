//! Shape colours: a shape's Mermaid `style` line (`style api
//! fill:#dbeafe,stroke:#3b82f6,color:#111827`), the preset swatches, and the
//! label colour that reads on a fill.
//!
//! A colour keeps its text exactly as written, so a hand-written value
//! BlockLiner can't draw (`rgb(…)`, an unusual name) is still written back
//! unchanged; it's drawn in the default colour.

/// A colour as written in a `style` line, and its RGBA where it could be
/// parsed (hex, and the CSS names Mermaid examples use).
#[derive(Clone, Debug, PartialEq)]
pub struct Colour {
    pub text: String,
    pub rgba: Option<[u8; 4]>,
}

impl Colour {
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        Self {
            text: text.to_owned(),
            rgba: hex(text).or_else(|| named(text)),
        }
    }

    /// A value typed into the Custom… field: `#rgb` or `#rrggbb` only,
    /// normalised to lower case.
    pub fn custom(text: &str) -> Option<Self> {
        let t = text.trim().to_ascii_lowercase();
        let t = if t.starts_with('#') {
            t
        } else {
            format!("#{t}")
        };
        matches!(t.len(), 4 | 7)
            .then(|| Self::parse(&t))
            .filter(|c| c.rgba.is_some())
    }
}

fn hex(t: &str) -> Option<[u8; 4]> {
    let h = t.strip_prefix('#')?;
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&h[i..=i], 16).ok().map(|v| v * 17);
    let pair = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some(match h.len() {
        3 => [digit(0)?, digit(1)?, digit(2)?, 255],
        4 => [digit(0)?, digit(1)?, digit(2)?, digit(3)?],
        6 => [pair(0)?, pair(2)?, pair(4)?, 255],
        8 => [pair(0)?, pair(2)?, pair(4)?, pair(6)?],
        _ => return None,
    })
}

fn named(t: &str) -> Option<[u8; 4]> {
    let rgb = match t.to_ascii_lowercase().as_str() {
        "none" | "transparent" => return Some([0, 0, 0, 0]),
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "gray" | "grey" => [128, 128, 128],
        "lightgray" | "lightgrey" => [211, 211, 211],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "lightgreen" => [144, 238, 144],
        "blue" => [0, 0, 255],
        "lightblue" => [173, 216, 230],
        "yellow" => [255, 255, 0],
        "orange" => [255, 165, 0],
        "purple" => [128, 0, 128],
        "pink" => [255, 192, 203],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "teal" => [0, 128, 128],
        "navy" => [0, 0, 128],
        "brown" => [165, 42, 42],
        _ => return None,
    };
    Some([rgb[0], rgb[1], rgb[2], 255])
}

/// A swatch: a light fill and a darker stroke of the same hue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub fill: &'static str,
    pub stroke: &'static str,
}

pub const PRESETS: [Preset; 8] = [
    Preset {
        name: "Gray",
        fill: "#f3f4f6",
        stroke: "#6b7280",
    },
    Preset {
        name: "Blue",
        fill: "#dbeafe",
        stroke: "#3b82f6",
    },
    Preset {
        name: "Teal",
        fill: "#ccfbf1",
        stroke: "#14b8a6",
    },
    Preset {
        name: "Green",
        fill: "#dcfce7",
        stroke: "#22c55e",
    },
    Preset {
        name: "Yellow",
        fill: "#fef9c3",
        stroke: "#eab308",
    },
    Preset {
        name: "Orange",
        fill: "#ffedd5",
        stroke: "#f97316",
    },
    Preset {
        name: "Red",
        fill: "#fee2e2",
        stroke: "#ef4444",
    },
    Preset {
        name: "Purple",
        fill: "#ede9fe",
        stroke: "#8b5cf6",
    },
];

/// Label colours on a fill: near-black or white.
pub const DARK_TEXT: &str = "#111827";
pub const LIGHT_TEXT: &str = "#ffffff";

/// The label colour that contrasts more with `fill` (WCAG contrast ratio).
pub fn contrast(fill: [u8; 4]) -> Colour {
    let lum = |rgb: [u8; 3]| {
        let c = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * c(rgb[0]) + 0.7152 * c(rgb[1]) + 0.0722 * c(rgb[2])
    };
    let l = lum([fill[0], fill[1], fill[2]]);
    let dark = lum([0x11, 0x18, 0x27]);
    let on_dark = (l + 0.05) / (dark + 0.05);
    let on_light = 1.05 / (l + 0.05);
    Colour::parse(if on_dark >= on_light {
        DARK_TEXT
    } else {
        LIGHT_TEXT
    })
}

/// A shape's own style. Empty for most shapes; nothing is written then.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub fill: Option<Colour>,
    pub stroke: Option<Colour>,
    /// The label colour.
    pub color: Option<Colour>,
    /// Other properties (`stroke-width`, `stroke-dasharray`, …), in order,
    /// written back unchanged.
    pub extra: Vec<(String, String)>,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        self.fill.is_none()
            && self.stroke.is_none()
            && self.color.is_none()
            && self.extra.is_empty()
    }

    /// The properties of a `style` line (`fill:#f9f,stroke:#333`). `None`
    /// if they don't read as `key:value` pairs.
    pub fn parse(props: &str) -> Option<Self> {
        let mut s = Style::default();
        for item in split_props(props) {
            let (k, v) = item.split_once(':')?;
            let (k, v) = (k.trim(), v.trim());
            if k.is_empty() || v.is_empty() {
                return None;
            }
            s.set(k, v);
        }
        (!s.is_empty()).then_some(s)
    }

    fn set(&mut self, k: &str, v: &str) {
        match k {
            "fill" => self.fill = Some(Colour::parse(v)),
            "stroke" => self.stroke = Some(Colour::parse(v)),
            "color" => self.color = Some(Colour::parse(v)),
            _ => match self.extra.iter_mut().find(|(key, _)| key == k) {
                Some(entry) => entry.1 = v.to_owned(),
                None => self.extra.push((k.to_owned(), v.to_owned())),
            },
        }
    }

    /// `other` on top of this one: its properties win.
    pub fn merge(&mut self, other: Style) {
        if other.fill.is_some() {
            self.fill = other.fill;
        }
        if other.stroke.is_some() {
            self.stroke = other.stroke;
        }
        if other.color.is_some() {
            self.color = other.color;
        }
        for (k, v) in other.extra {
            self.set(&k, &v);
        }
    }

    /// As written after `style <id> `: fill, stroke, color, then the rest.
    pub fn props(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        for (k, c) in [
            ("fill", &self.fill),
            ("stroke", &self.stroke),
            ("color", &self.color),
        ] {
            if let Some(c) = c {
                out.push(format!("{k}:{}", c.text));
            }
        }
        out.extend(self.extra.iter().map(|(k, v)| format!("{k}:{v}")));
        out.join(",")
    }

    /// A new fill (`None`: the default). A preset fill brings its stroke
    /// when the stroke was unset or came with the old fill, and the label
    /// colour follows the fill unless it was set otherwise.
    pub fn set_fill(&mut self, fill: Option<Colour>) {
        let old_pair = self.fill.as_ref().and_then(preset_of_fill);
        let stroke_follows = match (&self.stroke, old_pair) {
            (None, _) => true,
            (Some(s), Some(p)) => s.text == p.stroke,
            (Some(_), None) => false,
        };
        let color_follows = match (&self.color, self.fill.as_ref().and_then(|f| f.rgba)) {
            (None, _) => true,
            (Some(c), Some(rgba)) => *c == contrast(rgba),
            (Some(_), None) => false,
        };
        if stroke_follows {
            let pair = fill.as_ref().and_then(preset_of_fill);
            match (pair, fill.is_some()) {
                (Some(p), _) => self.stroke = Some(Colour::parse(p.stroke)),
                (None, false) => self.stroke = None,
                // A custom fill leaves the stroke as it is.
                (None, true) => {}
            }
        }
        if color_follows {
            self.color = fill.as_ref().and_then(|f| f.rgba).map(contrast);
        }
        self.fill = fill;
    }

    pub fn set_stroke(&mut self, stroke: Option<Colour>) {
        self.stroke = stroke;
    }

    /// Back to the theme's colours. Other properties stay.
    pub fn reset(&mut self) {
        self.fill = None;
        self.stroke = None;
        self.color = None;
    }

    /// The label colour to draw: the shape's own, else one that reads on
    /// its fill, else `None` (the theme's).
    pub fn label_rgba(&self) -> Option<[u8; 4]> {
        if let Some(c) = self.color.as_ref().and_then(|c| c.rgba) {
            return Some(c);
        }
        let fill = self.fill.as_ref()?.rgba.filter(|f| f[3] > 0)?;
        contrast(fill).rgba
    }
}

/// One property (a fill, a stroke) across a selection of shapes.
#[derive(Clone, Debug, PartialEq)]
pub enum Common {
    /// No shapes to ask.
    None,
    /// They all have this value (`None`: all the default).
    Same(Option<Colour>),
    /// They differ.
    Mixed,
}

/// What `values` have in common. Colours compare by what they draw (so
/// `#DBEAFE` matches `#dbeafe`), else by their text; the default counts as
/// a value of its own.
pub fn common<'a>(values: impl IntoIterator<Item = Option<&'a Colour>>) -> Common {
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return Common::None;
    };
    for v in values {
        if !same(first, v) {
            return Common::Mixed;
        }
    }
    Common::Same(first.cloned())
}

fn same(a: Option<&Colour>, b: Option<&Colour>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => match (a.rgba, b.rgba) {
            (Some(x), Some(y)) => x == y,
            _ => a.text.eq_ignore_ascii_case(&b.text),
        },
        _ => false,
    }
}

/// The preset whose fill (or `stroke`) is `c`, for ringing its swatch.
pub fn preset_index(c: &Colour, stroke: bool) -> Option<usize> {
    PRESETS.iter().position(|p| {
        let hex = if stroke { p.stroke } else { p.fill };
        same(Some(&Colour::parse(hex)), Some(c))
    })
}

fn preset_of_fill(fill: &Colour) -> Option<Preset> {
    PRESETS.into_iter().find(|p| p.fill == fill.text)
}

/// `a:1,b:rgb(1,2,3)` → `a:1`, `b:rgb(1,2,3)`: commas inside brackets
/// don't split.
fn split_props(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out.into_iter().filter(|p| !p.trim().is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(Colour::parse("#f9f").rgba, Some([255, 153, 255, 255]));
        assert_eq!(Colour::parse("#DBEAFE").rgba, Some([219, 234, 254, 255]));
        assert_eq!(Colour::parse("#11223380").rgba, Some([17, 34, 51, 128]));
        assert_eq!(Colour::parse("red").rgba, Some([255, 0, 0, 255]));
        assert_eq!(Colour::parse("none").rgba, Some([0, 0, 0, 0]));
        let odd = Colour::parse("rgb(1, 2, 3)");
        assert_eq!((odd.text.as_str(), odd.rgba), ("rgb(1, 2, 3)", None));
        assert_eq!(Colour::parse("#ggg").rgba, None);

        assert_eq!(Colour::custom("ABC").unwrap().text, "#abc");
        assert_eq!(Colour::custom(" #a0b1c2 ").unwrap().text, "#a0b1c2");
        assert_eq!(Colour::custom("#abcd"), None, "no alpha in Custom…");
        assert_eq!(Colour::custom("red"), None);
        assert_eq!(Colour::custom("#12345g"), None);
    }

    #[test]
    fn props_parse_merge_and_write() {
        let s = Style::parse("fill:#f9f, stroke:#333 ,stroke-width:4px,color:rgb(1,2,3)").unwrap();
        assert_eq!(s.fill.as_ref().unwrap().text, "#f9f");
        assert_eq!(s.color.as_ref().unwrap().text, "rgb(1,2,3)");
        assert_eq!(s.extra, [("stroke-width".to_owned(), "4px".to_owned())]);
        assert_eq!(
            s.props(),
            "fill:#f9f,stroke:#333,color:rgb(1,2,3),stroke-width:4px"
        );
        assert_eq!(Style::parse("fill"), None);
        assert_eq!(Style::parse(""), None);

        let mut a = Style::parse("fill:#fff,stroke-width:2px").unwrap();
        a.merge(Style::parse("stroke:#000,stroke-width:3px").unwrap());
        assert_eq!(a.props(), "fill:#fff,stroke:#000,stroke-width:3px");
    }

    #[test]
    fn contrast_picks_the_readable_label() {
        for p in PRESETS {
            let fill = Colour::parse(p.fill).rgba.unwrap();
            assert_eq!(contrast(fill).text, DARK_TEXT, "{}", p.name);
        }
        assert_eq!(contrast([0, 0, 0, 255]).text, LIGHT_TEXT);
        assert_eq!(contrast([255, 255, 255, 255]).text, DARK_TEXT);
        assert_eq!(contrast([30, 64, 175, 255]).text, LIGHT_TEXT, "dark blue");
    }

    #[test]
    fn a_preset_fill_brings_its_stroke_and_label_colour() {
        let blue = PRESETS[1];
        let red = PRESETS[6];
        let mut s = Style::default();
        s.set_fill(Some(Colour::parse(blue.fill)));
        assert_eq!(s.props(), "fill:#dbeafe,stroke:#3b82f6,color:#111827");
        // Another preset: the paired stroke follows.
        s.set_fill(Some(Colour::parse(red.fill)));
        assert_eq!(s.props(), "fill:#fee2e2,stroke:#ef4444,color:#111827");
        // A stroke picked on its own stays.
        s.set_stroke(Some(Colour::parse("#000")));
        s.set_fill(Some(Colour::parse(blue.fill)));
        assert_eq!(s.stroke.as_ref().unwrap().text, "#000");
        // A dark custom fill: white labels, stroke kept.
        s.set_fill(Colour::custom("#1e3a8a"));
        assert_eq!(s.props(), "fill:#1e3a8a,stroke:#000,color:#ffffff");
        // Default: no fill, no label colour; the own stroke stays.
        s.set_fill(None);
        assert_eq!(s.props(), "stroke:#000");
        s.reset();
        assert!(s.is_empty());

        // A paired stroke goes with its fill.
        let mut s = Style::default();
        s.set_fill(Some(Colour::parse(blue.fill)));
        s.set_fill(None);
        assert!(s.is_empty());

        // A label colour set by hand is kept.
        let mut s = Style::parse("color:#ff0000").unwrap();
        s.set_fill(Some(Colour::parse(blue.fill)));
        assert_eq!(s.color.as_ref().unwrap().text, "#ff0000");
    }

    #[test]
    fn what_a_selection_has_in_common() {
        let c = Colour::parse;
        let (blue, upper, red) = (c("#dbeafe"), c("#DBEAFE"), c("red"));
        assert_eq!(common([]), Common::None);
        assert_eq!(common([None, None]), Common::Same(None));
        assert_eq!(common([Some(&blue)]), Common::Same(Some(blue.clone())));
        assert_eq!(
            common([Some(&blue), Some(&upper)]),
            Common::Same(Some(blue.clone())),
            "same colour, different case"
        );
        assert_eq!(common([Some(&blue), Some(&red)]), Common::Mixed);
        assert_eq!(
            common([Some(&blue), None]),
            Common::Mixed,
            "default is a value"
        );
        let (odd, odd2) = (c("rgb(1,2,3)"), c("RGB(1,2,3)"));
        assert_eq!(common([Some(&odd), Some(&odd2)]), Common::Same(Some(odd)));

        assert_eq!(preset_index(&upper, false), Some(1));
        assert_eq!(preset_index(&c("#3b82f6"), true), Some(1));
        assert_eq!(preset_index(&c("#3b82f6"), false), None);
    }

    #[test]
    fn label_colours() {
        assert_eq!(Style::default().label_rgba(), None);
        let s = Style::parse("fill:#000").unwrap();
        assert_eq!(s.label_rgba(), Some([255, 255, 255, 255]));
        let s = Style::parse("fill:none").unwrap();
        assert_eq!(s.label_rgba(), None, "no fill to contrast with");
        let s = Style::parse("fill:#000,color:red").unwrap();
        assert_eq!(s.label_rgba(), Some([255, 0, 0, 255]));
    }
}
