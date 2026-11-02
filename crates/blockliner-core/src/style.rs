//! Shape and connector colours: a shape's Mermaid `style` line (`style api
//! fill:#dbeafe,stroke:#3b82f6,color:#111827`) or an edge's `linkStyle`
//! line, the preset swatches, and the label colour that reads on a fill.
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

/// A shape's or connector's own style. Empty for most; nothing is written
/// then.
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

    /// Back to the theme's look: the default colours, a solid stroke of the
    /// default weight, and Normal text. Other properties stay.
    pub fn reset(&mut self) {
        self.fill = None;
        self.stroke = None;
        self.color = None;
        for key in [DASH_KEY, WEIGHT_KEY, FONT_WEIGHT_KEY, FONT_STYLE_KEY] {
            self.set_extra(key, None);
        }
    }

    fn get_extra(&self, key: &str) -> Option<&str> {
        self.extra
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Sets (or with `None` removes) a property in `extra`, keeping its
    /// place among the others when it had one, so a hand-written line's
    /// order stays.
    fn set_extra(&mut self, key: &str, value: Option<String>) {
        let at = self.extra.iter().position(|(k, _)| k == key);
        match (value, at) {
            (None, Some(i)) => {
                self.extra.remove(i);
            }
            (None, None) => {}
            (Some(v), Some(i)) => self.extra[i].1 = v,
            (Some(v), None) => self.extra.push((key.to_owned(), v)),
        }
    }

    /// The stroke's weight (`None`: the default, 1.5px), from its
    /// `stroke-width`. A hand-written `1.5px` is the default too.
    pub fn weight(&self) -> Option<Weight> {
        self.get_extra(WEIGHT_KEY)
            .and_then(Weight::parse)
            .filter(|w| *w != Weight::Preset(Weight::DEFAULT))
    }

    /// A new weight (`None` or 1.5px: the default, not written).
    pub fn set_weight(&mut self, weight: Option<Weight>) {
        let weight = weight.filter(|w| *w != Weight::Preset(Weight::DEFAULT));
        if weight.is_none() && self.weight().is_none() {
            // Already the default: a hand-written `1.5px` stays as written.
            if self.get_extra(WEIGHT_KEY).and_then(Weight::parse).is_some() {
                return;
            }
        }
        self.set_extra(WEIGHT_KEY, weight.map(|w| w.value()));
    }

    /// The label is bold: `font-weight` is `bold`, `bolder` or 600–900.
    pub fn bold(&self) -> bool {
        self.get_extra(FONT_WEIGHT_KEY).is_some_and(|v| {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "bold" | "bolder")
                || v.parse::<f32>().is_ok_and(|n| (600.0..=900.0).contains(&n))
        })
    }

    /// Bold (`font-weight:bold`) or Normal (removed). A hand-written value
    /// that already reads as the one asked for stays as written.
    pub fn set_bold(&mut self, bold: bool) {
        if self.bold() != bold {
            self.set_extra(FONT_WEIGHT_KEY, bold.then(|| "bold".to_owned()));
        }
    }

    /// The label is italic: `font-style` is `italic` or `oblique …`.
    pub fn italic(&self) -> bool {
        self.get_extra(FONT_STYLE_KEY).is_some_and(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "italic" || v.starts_with("oblique")
        })
    }

    /// Italic (`font-style:italic`) or Normal (removed), as [`Style::set_bold`].
    pub fn set_italic(&mut self, italic: bool) {
        if self.italic() != italic {
            self.set_extra(FONT_STYLE_KEY, italic.then(|| "italic".to_owned()));
        }
    }

    /// The stroke's dash pattern (`None`: solid), from its
    /// `stroke-dasharray`.
    pub fn dash(&self) -> Option<Dash> {
        self.extra
            .iter()
            .find(|(k, _)| k == DASH_KEY)
            .and_then(|(_, v)| Dash::parse(v))
    }

    /// A new dash pattern (`None`: solid). It keeps its place among the
    /// other properties when it had one, so a hand-written line's order
    /// stays.
    pub fn set_dash(&mut self, dash: Option<Dash>) {
        self.set_extra(DASH_KEY, dash.map(|d| d.value().to_owned()));
    }

    /// [`Style::props`] without the dash: for an edge whose dots are in its
    /// link (`-.->`).
    pub fn props_without_dash(&self) -> String {
        let mut s = self.clone();
        s.extra.retain(|(k, _)| k != DASH_KEY);
        s.props()
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

/// The property that holds a dash pattern.
pub const DASH_KEY: &str = "stroke-dasharray";

/// A stroke's dash pattern. Solid is no pattern (`Option<Dash>::None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dash {
    /// `6 4`: 6 px dashes, 4 px gaps.
    Dashed,
    /// `2 3`: round dots, 3 px apart.
    Dotted,
    /// Any other `stroke-dasharray`, kept exactly as written.
    Custom(String),
}

impl Dash {
    pub const DASHED: &'static str = "6 4";
    pub const DOTTED: &'static str = "2 3";

    /// A `stroke-dasharray` value. `None` for solid (`none`, `0`, empty).
    pub fn parse(v: &str) -> Option<Self> {
        let words: Vec<&str> = v.split_whitespace().collect();
        let norm = words.join(" ");
        match norm.as_str() {
            "" | "none" | "0" => None,
            Self::DASHED => Some(Self::Dashed),
            Self::DOTTED => Some(Self::Dotted),
            _ => Some(Self::Custom(v.trim().to_owned())),
        }
    }

    /// As written in `stroke-dasharray`.
    pub fn value(&self) -> &str {
        match self {
            Self::Dashed => Self::DASHED,
            Self::Dotted => Self::DOTTED,
            Self::Custom(v) => v,
        }
    }

    /// The menu and pane name.
    pub fn name(dash: Option<&Dash>) -> &'static str {
        match dash {
            None => "Solid",
            Some(Self::Dashed) => "Dashed",
            Some(Self::Dotted) => "Dotted",
            Some(Self::Custom(_)) => "Custom",
        }
    }

    /// The dash and gap lengths to draw at 100 %, from the first two
    /// numbers (one number is both, as in SVG). `None` when they don't
    /// parse or aren't positive: drawn solid.
    pub fn pattern(&self) -> Option<(f32, f32)> {
        let nums: Option<Vec<f32>> = self
            .value()
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|w| !w.is_empty())
            .map(|w| w.trim_end_matches("px").parse::<f32>().ok())
            .collect();
        let nums = nums?;
        let (dash, gap) = match nums.as_slice() {
            [d] => (*d, *d),
            [d, g, ..] => (*d, *g),
            [] => return None,
        };
        (dash.is_finite() && gap.is_finite() && dash > 0.0 && gap > 0.0).then_some((dash, gap))
    }
}

/// The property that holds a stroke's weight.
pub const WEIGHT_KEY: &str = "stroke-width";
/// The label's weight (Bold)…
pub const FONT_WEIGHT_KEY: &str = "font-weight";
/// …and style (Italic).
pub const FONT_STYLE_KEY: &str = "font-style";

/// A stroke's weight. The default, 1.5px, is no weight
/// (`Option<Weight>::None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Weight {
    /// One of [`Weight::PRESETS`], in tenths of a pixel.
    Preset(u8),
    /// Any other `stroke-width`, kept exactly as written.
    Custom(String),
}

/// The weights offered, in order; `None` is the default, 1.5px.
pub const WEIGHTS: [Option<Weight>; 6] = [
    Some(Weight::Preset(10)),
    None,
    Some(Weight::Preset(20)),
    Some(Weight::Preset(25)),
    Some(Weight::Preset(30)),
    Some(Weight::Preset(40)),
];

impl Weight {
    /// The presets, in tenths of a pixel.
    pub const PRESETS: [u8; 6] = [10, 15, 20, 25, 30, 40];
    /// The default weight, 1.5px.
    pub const DEFAULT: u8 = 15;

    /// A `stroke-width` value (`2px`, `2.5`, `0.2em`). `None` when empty.
    pub fn parse(v: &str) -> Option<Self> {
        let t = v.trim();
        if t.is_empty() {
            return None;
        }
        let n = t.strip_suffix("px").unwrap_or(t).trim();
        if let Ok(x) = n.parse::<f32>() {
            let tenths = (x * 10.0).round();
            if (x * 10.0 - tenths).abs() < 1e-3 && (0.0..=255.0).contains(&tenths) {
                let tenths = tenths as u8;
                if Self::PRESETS.contains(&tenths) {
                    return Some(Self::Preset(tenths));
                }
            }
        }
        Some(Self::Custom(t.to_owned()))
    }

    /// The number of pixels, as written: `1`, `2.5`.
    pub fn number_text(tenths: u8) -> String {
        if tenths.is_multiple_of(10) {
            format!("{}", tenths / 10)
        } else {
            format!("{}.{}", tenths / 10, tenths % 10)
        }
    }

    /// As written in `stroke-width`: `2.5px`.
    pub fn value(&self) -> String {
        match self {
            Self::Preset(t) => format!("{}px", Self::number_text(*t)),
            Self::Custom(v) => v.clone(),
        }
    }

    /// As written in a free line's `weight=` field: `2.5` (a Custom value
    /// as written).
    pub fn field(&self) -> String {
        match self {
            Self::Preset(t) => Self::number_text(*t),
            Self::Custom(v) => v.clone(),
        }
    }

    /// The menu and pane name: `2px`, `1.5px (Default)`, `Custom`.
    pub fn label(weight: Option<&Weight>) -> String {
        match weight {
            None => format!("{}px (Default)", Self::number_text(Self::DEFAULT)),
            Some(Self::Preset(t)) if *t == Self::DEFAULT => Self::label(None),
            Some(w @ Self::Preset(_)) => w.value(),
            Some(Self::Custom(_)) => "Custom".into(),
        }
    }

    /// The width to draw at 100 %: a preset's, or a Custom value's when it
    /// is a plain or `px` number from 0.5 to 8; else `None` (the default).
    pub fn px(&self) -> Option<f32> {
        match self {
            Self::Preset(t) => Some(f32::from(*t) / 10.0),
            Self::Custom(v) => {
                let v = v.trim();
                let n = v.strip_suffix("px").unwrap_or(v).trim();
                n.parse::<f32>().ok().filter(|x| (0.5..=8.0).contains(x))
            }
        }
    }
}

/// What a selection's shapes and lines have in common, as [`common`] is
/// for colours.
#[derive(Clone, Debug, PartialEq)]
pub enum Agreement<T> {
    /// Nothing to ask.
    None,
    /// They all have this.
    Same(T),
    /// They differ.
    Mixed,
}

pub fn agreement<T: PartialEq>(values: impl IntoIterator<Item = T>) -> Agreement<T> {
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return Agreement::None;
    };
    for v in values {
        if v != first {
            return Agreement::Mixed;
        }
    }
    Agreement::Same(first)
}

/// The dash across a selection (`Same(None)`: all solid).
pub type DashCommon = Agreement<Option<Dash>>;

pub fn common_dash(values: impl IntoIterator<Item = Option<Dash>>) -> DashCommon {
    agreement(values)
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
    fn weights() {
        assert_eq!(Weight::parse("2px"), Some(Weight::Preset(20)));
        assert_eq!(Weight::parse(" 2.5 "), Some(Weight::Preset(25)));
        assert_eq!(Weight::parse("2.50px"), Some(Weight::Preset(25)));
        assert_eq!(Weight::parse("4.0"), Some(Weight::Preset(40)));
        assert_eq!(Weight::parse("1.5px"), Some(Weight::Preset(15)));
        assert_eq!(Weight::parse("5px"), Some(Weight::Custom("5px".into())));
        assert_eq!(Weight::parse("0.2em"), Some(Weight::Custom("0.2em".into())));
        assert_eq!(Weight::parse("999"), Some(Weight::Custom("999".into())));
        assert_eq!(Weight::parse(""), None);
        assert_eq!(Weight::Preset(25).value(), "2.5px");
        assert_eq!(Weight::Preset(10).field(), "1");
        assert_eq!(Weight::label(None), "1.5px (Default)");
        assert_eq!(Weight::label(Some(&Weight::Preset(30))), "3px");
        assert_eq!(Weight::label(Some(&Weight::Custom("5px".into()))), "Custom");
        assert_eq!(Weight::Preset(25).px(), Some(2.5));
        assert_eq!(Weight::Custom("6px".into()).px(), Some(6.0));
        assert_eq!(Weight::Custom("0.2em".into()).px(), None);
        assert_eq!(Weight::Custom("12".into()).px(), None);
        let labels: Vec<String> = WEIGHTS.iter().map(|w| Weight::label(w.as_ref())).collect();
        assert_eq!(
            labels,
            ["1px", "1.5px (Default)", "2px", "2.5px", "3px", "4px"]
        );

        // In place, and the default never written.
        let mut s = Style::parse("stroke:#333,stroke-width:5px,color:#fff").unwrap();
        assert_eq!(s.weight(), Some(Weight::Custom("5px".into())));
        s.set_weight(Some(Weight::Preset(25)));
        assert_eq!(s.props(), "stroke:#333,color:#fff,stroke-width:2.5px");
        s.set_weight(Some(Weight::Preset(15)));
        assert_eq!(s.props(), "stroke:#333,color:#fff");
        s.set_weight(Some(Weight::Preset(40)));
        assert_eq!(s.props(), "stroke:#333,color:#fff,stroke-width:4px");

        // A hand-written 1.5px is the default, and stays as written.
        let mut s = Style::parse("stroke-width:1.5px").unwrap();
        assert_eq!(s.weight(), None);
        s.set_weight(None);
        assert_eq!(s.props(), "stroke-width:1.5px");
        // Reset Style removes it.
        s.reset();
        assert_eq!(s.props(), "");
    }

    #[test]
    fn bold_and_italic() {
        for (v, bold) in [
            ("bold", true),
            ("Bolder", true),
            ("600", true),
            ("900", true),
            ("normal", false),
            ("400", false),
            ("lighter", false),
        ] {
            let s = Style::parse(&format!("font-weight:{v}")).unwrap();
            assert_eq!(s.bold(), bold, "{v}");
        }
        for (v, italic) in [("italic", true), ("oblique 10deg", true), ("normal", false)] {
            let s = Style::parse(&format!("font-style:{v}")).unwrap();
            assert_eq!(s.italic(), italic, "{v}");
        }
        // A hand-written value that already reads right stays as written.
        let mut s = Style::parse("font-weight:600,fill:#fff").unwrap();
        s.set_bold(true);
        assert_eq!(s.props(), "fill:#fff,font-weight:600");
        s.set_bold(false);
        assert_eq!(s.props(), "fill:#fff");
        s.set_bold(true);
        s.set_italic(true);
        assert_eq!(s.props(), "fill:#fff,font-weight:bold,font-style:italic");
        s.reset();
        assert!(s.is_empty());
        let mut s = Style::parse("font-weight:normal").unwrap();
        s.reset();
        assert!(s.is_empty());
    }

    #[test]
    fn dashes() {
        assert_eq!(Dash::parse("6 4"), Some(Dash::Dashed));
        assert_eq!(Dash::parse(" 6   4 "), Some(Dash::Dashed));
        assert_eq!(Dash::parse("2 3"), Some(Dash::Dotted));
        assert_eq!(Dash::parse("none"), None);
        assert_eq!(Dash::parse("0"), None);
        assert_eq!(Dash::parse("5 5"), Some(Dash::Custom("5 5".into())));
        assert_eq!(Dash::Dashed.pattern(), Some((6.0, 4.0)));
        assert_eq!(Dash::Custom("3".into()).pattern(), Some((3.0, 3.0)));
        assert_eq!(Dash::Custom("8px 2px 1".into()).pattern(), Some((8.0, 2.0)));
        assert_eq!(Dash::Custom("wavy".into()).pattern(), None);
        assert_eq!(Dash::Custom("0 4".into()).pattern(), None);

        // In place among the other properties, and gone when solid.
        let mut s = Style::parse("stroke:#333,stroke-dasharray:5 5,stroke-width:2px").unwrap();
        assert_eq!(s.dash(), Some(Dash::Custom("5 5".into())));
        s.set_dash(Some(Dash::Dotted));
        assert_eq!(
            s.props(),
            "stroke:#333,stroke-dasharray:2 3,stroke-width:2px"
        );
        assert_eq!(s.props_without_dash(), "stroke:#333,stroke-width:2px");
        s.set_dash(None);
        assert_eq!(s.props(), "stroke:#333,stroke-width:2px");
        s.set_dash(Some(Dash::Dashed));
        assert_eq!(
            s.props(),
            "stroke:#333,stroke-width:2px,stroke-dasharray:6 4"
        );
        // Reset Style: back to solid and the default weight.
        s.reset();
        assert_eq!(s.props(), "");

        assert_eq!(common_dash([]), DashCommon::None);
        assert_eq!(common_dash([None, None]), DashCommon::Same(None));
        assert_eq!(
            common_dash([Some(Dash::Dotted), Some(Dash::Dotted)]),
            DashCommon::Same(Some(Dash::Dotted))
        );
        assert_eq!(common_dash([Some(Dash::Dotted), None]), DashCommon::Mixed);
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
