//! What one grid mark measures. Settings picks the unit; one grid cell is
//! one of it, so a shape 8 cells wide is 8 cm (or 8 in, …). The dimension
//! lines and the status bar read sizes through [`Unit::measure`].

/// The unit one grid mark stands for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unit {
    Mm,
    #[default]
    Cm,
    M,
    In,
    Ft,
}

impl Unit {
    /// In the order Settings lists them.
    pub const ALL: [Unit; 5] = [Unit::Mm, Unit::Cm, Unit::M, Unit::In, Unit::Ft];

    /// The pref value, and the suffix after a number.
    pub fn id(self) -> &'static str {
        match self {
            Unit::Mm => "mm",
            Unit::Cm => "cm",
            Unit::M => "m",
            Unit::In => "in",
            Unit::Ft => "ft",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Unit::Mm => "Millimetres (mm)",
            Unit::Cm => "Centimetres (cm)",
            Unit::M => "Metres (m)",
            Unit::In => "Inches (in)",
            Unit::Ft => "Feet (ft)",
        }
    }

    pub fn parse(s: &str) -> Option<Unit> {
        Unit::ALL.into_iter().find(|u| u.id() == s.trim())
    }

    /// `length` diagram pixels on a `grid`-pixel grid, as `8 cm` or
    /// `2.5 in`: up to two decimals, trailing zeros dropped.
    pub fn measure(self, length: f32, grid: f32) -> String {
        let cells = if grid > 0.0 { length / grid } else { 0.0 };
        let text = format!("{cells:.2}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        let text = if text == "-0" { "0" } else { text };
        format!("{text} {}", self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for u in Unit::ALL {
            assert_eq!(Unit::parse(u.id()), Some(u));
        }
        assert_eq!(Unit::parse("yd"), None);
    }

    #[test]
    fn measures() {
        assert_eq!(Unit::Cm.measure(160.0, 20.0), "8 cm");
        assert_eq!(Unit::In.measure(50.0, 20.0), "2.5 in");
        assert_eq!(Unit::Mm.measure(10.0, 30.0), "0.33 mm");
        assert_eq!(Unit::Ft.measure(0.0, 20.0), "0 ft");
        assert_eq!(Unit::M.measure(40.0, 40.0), "1 m");
    }
}
