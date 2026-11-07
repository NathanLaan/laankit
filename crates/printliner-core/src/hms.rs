//! HMS (Health Management System) entries: what a printer reports when
//! something needs attention. Each `{attr, code}` pair shows as
//! `AAAA_BBBB_CCCC_DDDD`, the four 16-bit halves in hex, which is how Bambu's
//! HMS wiki and the printer's screen name it.

use serde_json::Value;

use crate::values::i64_of;

/// Bambu's HMS wiki, which explains each code.
pub const WIKI_URL: &str = "https://wiki.bambulab.com/en/hms/home";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Fatal,
    Serious,
    Common,
    Info,
    Unknown,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Fatal => "Fatal",
            Severity::Serious => "Serious",
            Severity::Common => "Warning",
            Severity::Info => "Info",
            Severity::Unknown => "Notice",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hms {
    pub attr: u32,
    pub code: u32,
}

impl Hms {
    /// `AAAA_BBBB_CCCC_DDDD`.
    pub fn display_code(self) -> String {
        format!(
            "{:04X}_{:04X}_{:04X}_{:04X}",
            self.attr >> 16,
            self.attr & 0xFFFF,
            self.code >> 16,
            self.code & 0xFFFF
        )
    }

    /// From the code's upper half: 1 fatal, 2 serious, 3 common, 4 info.
    pub fn severity(self) -> Severity {
        match self.code >> 16 {
            1 => Severity::Fatal,
            2 => Severity::Serious,
            3 => Severity::Common,
            4 => Severity::Info,
            _ => Severity::Unknown,
        }
    }
}

/// Reads the report's `hms` array, most severe first.
pub fn parse(v: &Value) -> Vec<Hms> {
    let mut out: Vec<Hms> = v
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|e| {
                    let attr = e.get("attr").and_then(i64_of)?;
                    let code = e.get("code").and_then(i64_of)?;
                    Some(Hms {
                        attr: u32::try_from(attr).ok()?,
                        code: u32::try_from(code).ok()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|h| h.severity());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_code_and_severity() {
        let h = Hms {
            attr: 0x0300_0100,
            code: 0x0001_0001,
        };
        assert_eq!(h.display_code(), "0300_0100_0001_0001");
        assert_eq!(h.severity(), Severity::Fatal);
        let h = Hms {
            attr: 0x0700_2000,
            code: 0x0003_0002,
        };
        assert_eq!(h.severity(), Severity::Common);
    }

    #[test]
    fn parses_and_sorts() {
        let v = json!([
            { "attr": 0x0700_2000_i64, "code": 0x0004_0001_i64 },
            { "attr": "50331904", "code": 65537 },
            { "attr": -1, "code": 1 },
            { "code": 1 }
        ]);
        let list = parse(&v);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].display_code(), "0300_0100_0001_0001");
        assert_eq!(list[1].severity(), Severity::Info);
        assert!(parse(&json!(null)).is_empty());
    }
}
