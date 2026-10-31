//! The SVG 1.1 named colours tags can be painted with (`lib/svgColors.js`):
//! the swatches in the W3C chart's hue order, each with whether white text
//! would lose contrast on it. Tag colours are stored by name in
//! `index.tagMeta[tag].color`, as picked; lookups ignore case.
//!
//! Generated from `apps/noteliner/src/renderer/lib/svgColors.js`.

use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Swatch {
    pub name: &'static str,
    pub rgb: u32,
    /// Dark text reads better on this swatch than white.
    pub dark_text: bool,
}

impl Swatch {
    pub fn color(&self) -> Color32 {
        let [_, r, g, b] = self.rgb.to_be_bytes();
        Color32::from_rgb(r, g, b)
    }

    /// The chip's text colour on this swatch (`#111` or `#fff`).
    pub fn ink(&self) -> Color32 {
        if self.dark_text {
            Color32::from_rgb(0x11, 0x11, 0x11)
        } else {
            Color32::WHITE
        }
    }
}

/// `lookupColor`: a swatch by name, ignoring case; `None` for unset or unknown.
pub fn lookup(name: Option<&str>) -> Option<&'static Swatch> {
    let name = name?;
    SVG_COLORS
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
}

pub const SVG_COLORS: [Swatch; 148] = [
    Swatch {
        name: "Pink",
        rgb: 0xffc0cb,
        dark_text: true,
    },
    Swatch {
        name: "LightPink",
        rgb: 0xffb6c1,
        dark_text: true,
    },
    Swatch {
        name: "HotPink",
        rgb: 0xff69b4,
        dark_text: true,
    },
    Swatch {
        name: "DeepPink",
        rgb: 0xff1493,
        dark_text: false,
    },
    Swatch {
        name: "PaleVioletRed",
        rgb: 0xdb7093,
        dark_text: false,
    },
    Swatch {
        name: "MediumVioletRed",
        rgb: 0xc71585,
        dark_text: false,
    },
    Swatch {
        name: "LightSalmon",
        rgb: 0xffa07a,
        dark_text: true,
    },
    Swatch {
        name: "Salmon",
        rgb: 0xfa8072,
        dark_text: true,
    },
    Swatch {
        name: "DarkSalmon",
        rgb: 0xe9967a,
        dark_text: true,
    },
    Swatch {
        name: "LightCoral",
        rgb: 0xf08080,
        dark_text: true,
    },
    Swatch {
        name: "IndianRed",
        rgb: 0xcd5c5c,
        dark_text: false,
    },
    Swatch {
        name: "Crimson",
        rgb: 0xdc143c,
        dark_text: false,
    },
    Swatch {
        name: "Firebrick",
        rgb: 0xb22222,
        dark_text: false,
    },
    Swatch {
        name: "DarkRed",
        rgb: 0x8b0000,
        dark_text: false,
    },
    Swatch {
        name: "Red",
        rgb: 0xff0000,
        dark_text: false,
    },
    Swatch {
        name: "OrangeRed",
        rgb: 0xff4500,
        dark_text: false,
    },
    Swatch {
        name: "Tomato",
        rgb: 0xff6347,
        dark_text: false,
    },
    Swatch {
        name: "Coral",
        rgb: 0xff7f50,
        dark_text: true,
    },
    Swatch {
        name: "DarkOrange",
        rgb: 0xff8c00,
        dark_text: true,
    },
    Swatch {
        name: "Orange",
        rgb: 0xffa500,
        dark_text: true,
    },
    Swatch {
        name: "Yellow",
        rgb: 0xffff00,
        dark_text: true,
    },
    Swatch {
        name: "LightYellow",
        rgb: 0xffffe0,
        dark_text: true,
    },
    Swatch {
        name: "LemonChiffon",
        rgb: 0xfffacd,
        dark_text: true,
    },
    Swatch {
        name: "LightGoldenrodYellow",
        rgb: 0xfafad2,
        dark_text: true,
    },
    Swatch {
        name: "PapayaWhip",
        rgb: 0xffefd5,
        dark_text: true,
    },
    Swatch {
        name: "Moccasin",
        rgb: 0xffe4b5,
        dark_text: true,
    },
    Swatch {
        name: "PeachPuff",
        rgb: 0xffdab9,
        dark_text: true,
    },
    Swatch {
        name: "PaleGoldenrod",
        rgb: 0xeee8aa,
        dark_text: true,
    },
    Swatch {
        name: "Khaki",
        rgb: 0xf0e68c,
        dark_text: true,
    },
    Swatch {
        name: "DarkKhaki",
        rgb: 0xbdb76b,
        dark_text: true,
    },
    Swatch {
        name: "Gold",
        rgb: 0xffd700,
        dark_text: true,
    },
    Swatch {
        name: "Cornsilk",
        rgb: 0xfff8dc,
        dark_text: true,
    },
    Swatch {
        name: "BlanchedAlmond",
        rgb: 0xffebcd,
        dark_text: true,
    },
    Swatch {
        name: "Bisque",
        rgb: 0xffe4c4,
        dark_text: true,
    },
    Swatch {
        name: "NavajoWhite",
        rgb: 0xffdead,
        dark_text: true,
    },
    Swatch {
        name: "Wheat",
        rgb: 0xf5deb3,
        dark_text: true,
    },
    Swatch {
        name: "Burlywood",
        rgb: 0xdeb887,
        dark_text: true,
    },
    Swatch {
        name: "Tan",
        rgb: 0xd2b48c,
        dark_text: true,
    },
    Swatch {
        name: "RosyBrown",
        rgb: 0xbc8f8f,
        dark_text: true,
    },
    Swatch {
        name: "SandyBrown",
        rgb: 0xf4a460,
        dark_text: true,
    },
    Swatch {
        name: "Goldenrod",
        rgb: 0xdaa520,
        dark_text: true,
    },
    Swatch {
        name: "DarkGoldenrod",
        rgb: 0xb8860b,
        dark_text: false,
    },
    Swatch {
        name: "Peru",
        rgb: 0xcd853f,
        dark_text: false,
    },
    Swatch {
        name: "Chocolate",
        rgb: 0xd2691e,
        dark_text: false,
    },
    Swatch {
        name: "SaddleBrown",
        rgb: 0x8b4513,
        dark_text: false,
    },
    Swatch {
        name: "Sienna",
        rgb: 0xa0522d,
        dark_text: false,
    },
    Swatch {
        name: "Brown",
        rgb: 0xa52a2a,
        dark_text: false,
    },
    Swatch {
        name: "Maroon",
        rgb: 0x800000,
        dark_text: false,
    },
    Swatch {
        name: "DarkOliveGreen",
        rgb: 0x556b2f,
        dark_text: false,
    },
    Swatch {
        name: "Olive",
        rgb: 0x808000,
        dark_text: false,
    },
    Swatch {
        name: "OliveDrab",
        rgb: 0x6b8e23,
        dark_text: false,
    },
    Swatch {
        name: "YellowGreen",
        rgb: 0x9acd32,
        dark_text: true,
    },
    Swatch {
        name: "LimeGreen",
        rgb: 0x32cd32,
        dark_text: true,
    },
    Swatch {
        name: "Lime",
        rgb: 0x00ff00,
        dark_text: true,
    },
    Swatch {
        name: "LawnGreen",
        rgb: 0x7cfc00,
        dark_text: true,
    },
    Swatch {
        name: "Chartreuse",
        rgb: 0x7fff00,
        dark_text: true,
    },
    Swatch {
        name: "GreenYellow",
        rgb: 0xadff2f,
        dark_text: true,
    },
    Swatch {
        name: "SpringGreen",
        rgb: 0x00ff7f,
        dark_text: true,
    },
    Swatch {
        name: "MediumSpringGreen",
        rgb: 0x00fa9a,
        dark_text: true,
    },
    Swatch {
        name: "LightGreen",
        rgb: 0x90ee90,
        dark_text: true,
    },
    Swatch {
        name: "PaleGreen",
        rgb: 0x98fb98,
        dark_text: true,
    },
    Swatch {
        name: "DarkSeaGreen",
        rgb: 0x8fbc8f,
        dark_text: true,
    },
    Swatch {
        name: "MediumAquamarine",
        rgb: 0x66cdaa,
        dark_text: true,
    },
    Swatch {
        name: "MediumSeaGreen",
        rgb: 0x3cb371,
        dark_text: true,
    },
    Swatch {
        name: "SeaGreen",
        rgb: 0x2e8b57,
        dark_text: false,
    },
    Swatch {
        name: "ForestGreen",
        rgb: 0x228b22,
        dark_text: false,
    },
    Swatch {
        name: "Green",
        rgb: 0x008000,
        dark_text: false,
    },
    Swatch {
        name: "DarkGreen",
        rgb: 0x006400,
        dark_text: false,
    },
    Swatch {
        name: "Aqua",
        rgb: 0x00ffff,
        dark_text: true,
    },
    Swatch {
        name: "Cyan",
        rgb: 0x00ffff,
        dark_text: true,
    },
    Swatch {
        name: "LightCyan",
        rgb: 0xe0ffff,
        dark_text: true,
    },
    Swatch {
        name: "PaleTurquoise",
        rgb: 0xafeeee,
        dark_text: true,
    },
    Swatch {
        name: "Aquamarine",
        rgb: 0x7fffd4,
        dark_text: true,
    },
    Swatch {
        name: "Turquoise",
        rgb: 0x40e0d0,
        dark_text: true,
    },
    Swatch {
        name: "MediumTurquoise",
        rgb: 0x48d1cc,
        dark_text: true,
    },
    Swatch {
        name: "DarkTurquoise",
        rgb: 0x00ced1,
        dark_text: true,
    },
    Swatch {
        name: "LightSeaGreen",
        rgb: 0x20b2aa,
        dark_text: false,
    },
    Swatch {
        name: "CadetBlue",
        rgb: 0x5f9ea0,
        dark_text: false,
    },
    Swatch {
        name: "DarkCyan",
        rgb: 0x008b8b,
        dark_text: false,
    },
    Swatch {
        name: "Teal",
        rgb: 0x008080,
        dark_text: false,
    },
    Swatch {
        name: "LightSteelBlue",
        rgb: 0xb0c4de,
        dark_text: true,
    },
    Swatch {
        name: "PowderBlue",
        rgb: 0xb0e0e6,
        dark_text: true,
    },
    Swatch {
        name: "LightBlue",
        rgb: 0xadd8e6,
        dark_text: true,
    },
    Swatch {
        name: "SkyBlue",
        rgb: 0x87ceeb,
        dark_text: true,
    },
    Swatch {
        name: "LightSkyBlue",
        rgb: 0x87cefa,
        dark_text: true,
    },
    Swatch {
        name: "DeepSkyBlue",
        rgb: 0x00bfff,
        dark_text: false,
    },
    Swatch {
        name: "DodgerBlue",
        rgb: 0x1e90ff,
        dark_text: false,
    },
    Swatch {
        name: "CornflowerBlue",
        rgb: 0x6495ed,
        dark_text: false,
    },
    Swatch {
        name: "SteelBlue",
        rgb: 0x4682b4,
        dark_text: false,
    },
    Swatch {
        name: "RoyalBlue",
        rgb: 0x4169e1,
        dark_text: false,
    },
    Swatch {
        name: "Blue",
        rgb: 0x0000ff,
        dark_text: false,
    },
    Swatch {
        name: "MediumBlue",
        rgb: 0x0000cd,
        dark_text: false,
    },
    Swatch {
        name: "DarkBlue",
        rgb: 0x00008b,
        dark_text: false,
    },
    Swatch {
        name: "Navy",
        rgb: 0x000080,
        dark_text: false,
    },
    Swatch {
        name: "MidnightBlue",
        rgb: 0x191970,
        dark_text: false,
    },
    Swatch {
        name: "Lavender",
        rgb: 0xe6e6fa,
        dark_text: true,
    },
    Swatch {
        name: "Thistle",
        rgb: 0xd8bfd8,
        dark_text: true,
    },
    Swatch {
        name: "Plum",
        rgb: 0xdda0dd,
        dark_text: true,
    },
    Swatch {
        name: "Violet",
        rgb: 0xee82ee,
        dark_text: true,
    },
    Swatch {
        name: "Orchid",
        rgb: 0xda70d6,
        dark_text: true,
    },
    Swatch {
        name: "Fuchsia",
        rgb: 0xff00ff,
        dark_text: false,
    },
    Swatch {
        name: "Magenta",
        rgb: 0xff00ff,
        dark_text: false,
    },
    Swatch {
        name: "MediumOrchid",
        rgb: 0xba55d3,
        dark_text: false,
    },
    Swatch {
        name: "MediumPurple",
        rgb: 0x9370db,
        dark_text: false,
    },
    Swatch {
        name: "BlueViolet",
        rgb: 0x8a2be2,
        dark_text: false,
    },
    Swatch {
        name: "DarkViolet",
        rgb: 0x9400d3,
        dark_text: false,
    },
    Swatch {
        name: "DarkOrchid",
        rgb: 0x9932cc,
        dark_text: false,
    },
    Swatch {
        name: "DarkMagenta",
        rgb: 0x8b008b,
        dark_text: false,
    },
    Swatch {
        name: "Purple",
        rgb: 0x800080,
        dark_text: false,
    },
    Swatch {
        name: "RebeccaPurple",
        rgb: 0x663399,
        dark_text: false,
    },
    Swatch {
        name: "Indigo",
        rgb: 0x4b0082,
        dark_text: false,
    },
    Swatch {
        name: "MediumSlateBlue",
        rgb: 0x7b68ee,
        dark_text: false,
    },
    Swatch {
        name: "SlateBlue",
        rgb: 0x6a5acd,
        dark_text: false,
    },
    Swatch {
        name: "DarkSlateBlue",
        rgb: 0x483d8b,
        dark_text: false,
    },
    Swatch {
        name: "White",
        rgb: 0xffffff,
        dark_text: true,
    },
    Swatch {
        name: "Snow",
        rgb: 0xfffafa,
        dark_text: true,
    },
    Swatch {
        name: "Honeydew",
        rgb: 0xf0fff0,
        dark_text: true,
    },
    Swatch {
        name: "MintCream",
        rgb: 0xf5fffa,
        dark_text: true,
    },
    Swatch {
        name: "Azure",
        rgb: 0xf0ffff,
        dark_text: true,
    },
    Swatch {
        name: "AliceBlue",
        rgb: 0xf0f8ff,
        dark_text: true,
    },
    Swatch {
        name: "GhostWhite",
        rgb: 0xf8f8ff,
        dark_text: true,
    },
    Swatch {
        name: "WhiteSmoke",
        rgb: 0xf5f5f5,
        dark_text: true,
    },
    Swatch {
        name: "Seashell",
        rgb: 0xfff5ee,
        dark_text: true,
    },
    Swatch {
        name: "Beige",
        rgb: 0xf5f5dc,
        dark_text: true,
    },
    Swatch {
        name: "OldLace",
        rgb: 0xfdf5e6,
        dark_text: true,
    },
    Swatch {
        name: "FloralWhite",
        rgb: 0xfffaf0,
        dark_text: true,
    },
    Swatch {
        name: "Ivory",
        rgb: 0xfffff0,
        dark_text: true,
    },
    Swatch {
        name: "AntiqueWhite",
        rgb: 0xfaebd7,
        dark_text: true,
    },
    Swatch {
        name: "Linen",
        rgb: 0xfaf0e6,
        dark_text: true,
    },
    Swatch {
        name: "LavenderBlush",
        rgb: 0xfff0f5,
        dark_text: true,
    },
    Swatch {
        name: "MistyRose",
        rgb: 0xffe4e1,
        dark_text: true,
    },
    Swatch {
        name: "Gainsboro",
        rgb: 0xdcdcdc,
        dark_text: true,
    },
    Swatch {
        name: "LightGray",
        rgb: 0xd3d3d3,
        dark_text: true,
    },
    Swatch {
        name: "LightGrey",
        rgb: 0xd3d3d3,
        dark_text: true,
    },
    Swatch {
        name: "Silver",
        rgb: 0xc0c0c0,
        dark_text: true,
    },
    Swatch {
        name: "DarkGray",
        rgb: 0xa9a9a9,
        dark_text: true,
    },
    Swatch {
        name: "DarkGrey",
        rgb: 0xa9a9a9,
        dark_text: true,
    },
    Swatch {
        name: "Gray",
        rgb: 0x808080,
        dark_text: false,
    },
    Swatch {
        name: "Grey",
        rgb: 0x808080,
        dark_text: false,
    },
    Swatch {
        name: "DimGray",
        rgb: 0x696969,
        dark_text: false,
    },
    Swatch {
        name: "DimGrey",
        rgb: 0x696969,
        dark_text: false,
    },
    Swatch {
        name: "LightSlateGray",
        rgb: 0x778899,
        dark_text: false,
    },
    Swatch {
        name: "LightSlateGrey",
        rgb: 0x778899,
        dark_text: false,
    },
    Swatch {
        name: "SlateGray",
        rgb: 0x708090,
        dark_text: false,
    },
    Swatch {
        name: "SlateGrey",
        rgb: 0x708090,
        dark_text: false,
    },
    Swatch {
        name: "DarkSlateGray",
        rgb: 0x2f4f4f,
        dark_text: false,
    },
    Swatch {
        name: "DarkSlateGrey",
        rgb: 0x2f4f4f,
        dark_text: false,
    },
    Swatch {
        name: "Black",
        rgb: 0x000000,
        dark_text: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_ignores_case() {
        assert_eq!(lookup(Some("tomato")).map(|s| s.name), Some("Tomato"));
        assert_eq!(lookup(Some("nope")), None);
        assert_eq!(lookup(None), None);
        assert_eq!(SVG_COLORS[0].color(), Color32::from_rgb(0xff, 0xc0, 0xcb));
    }
}
