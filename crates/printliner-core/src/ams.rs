//! The AMS (automatic material system): up to four units of four trays,
//! plus the external spool holder (`vt_tray`).
//!
//! `ams.tray_now` names the loaded tray: `unit * 4 + tray`, 254 for the
//! external spool, and 255 for none.

use serde_json::Value;

use crate::values::{i64_of, string_of};

/// One filament slot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tray {
    /// The tray's index in its unit (0 to 3), or 254 for the external spool.
    pub id: u8,
    /// The material (`PLA`, `PETG-CF`, …); `None` for an empty tray.
    pub material: Option<String>,
    /// RGBA, from `tray_color` (`RRGGBBAA`).
    pub colour: Option<[u8; 4]>,
    /// Percent left, when the AMS knows (`remain` is -1 when it doesn't).
    pub remain: Option<u8>,
    /// The tray feeding the printer now.
    pub active: bool,
}

impl Tray {
    pub fn is_empty(&self) -> bool {
        self.material.is_none()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Unit {
    pub id: u8,
    /// The AMS's humidity level as reported (`"1"` dry to `"5"` wet).
    pub humidity: Option<String>,
    pub trays: Vec<Tray>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ams {
    pub units: Vec<Unit>,
    /// The external spool holder.
    pub external: Option<Tray>,
}

impl Ams {
    /// The tray feeding the printer now.
    pub fn active_tray(&self) -> Option<&Tray> {
        self.units
            .iter()
            .flat_map(|u| u.trays.iter())
            .chain(self.external.iter())
            .find(|t| t.active)
    }
}

/// Reads the report's `print.ams` object and `print.vt_tray`.
pub fn parse(ams: Option<&Value>, vt_tray: Option<&Value>) -> Ams {
    let now = ams
        .and_then(|a| a.get("tray_now"))
        .and_then(i64_of)
        .unwrap_or(255);
    let units = ams
        .and_then(|a| a.get("ams"))
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|u| {
                    let id = u8::try_from(u.get("id").and_then(i64_of)?).ok()?;
                    let trays = u
                        .get("tray")
                        .and_then(Value::as_array)
                        .map(|trays| {
                            trays
                                .iter()
                                .filter_map(|t| {
                                    let mut tray = tray(t)?;
                                    tray.active = now == i64::from(id) * 4 + i64::from(tray.id);
                                    Some(tray)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(Unit {
                        id,
                        humidity: u.get("humidity").and_then(string_of),
                        trays,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let external = vt_tray.and_then(tray).map(|mut t| {
        t.id = 254;
        t.active = now == 254;
        t
    });
    Ams { units, external }
}

fn tray(t: &Value) -> Option<Tray> {
    let id = u8::try_from(t.get("id").and_then(i64_of)?).ok()?;
    let empty_uuid = t
        .get("tray_uuid")
        .and_then(Value::as_str)
        .is_some_and(|u| !u.is_empty() && u.chars().all(|c| c == '0'));
    let material = t
        .get("tray_type")
        .and_then(string_of)
        .filter(|_| !empty_uuid);
    Some(Tray {
        id,
        colour: material
            .as_ref()
            .and(t.get("tray_color").and_then(Value::as_str))
            .and_then(parse_colour),
        remain: t
            .get("remain")
            .and_then(i64_of)
            .and_then(|r| u8::try_from(r).ok())
            .filter(|r| *r <= 100),
        material,
        active: false,
    })
}

/// `RRGGBBAA` (or `RRGGBB`).
pub fn parse_colour(hex: &str) -> Option<[u8; 4]> {
    let hex = hex.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample(tray_now: &str) -> Ams {
        let ams = json!({
            "tray_now": tray_now,
            "ams": [{
                "id": "0",
                "humidity": "4",
                "tray": [
                    { "id": "0", "tray_type": "PLA", "tray_color": "FF0000FF", "remain": 80, "tray_uuid": "A1B2" },
                    { "id": "1", "tray_type": "PETG", "tray_color": "00FF00", "remain": -1 },
                    { "id": "2" },
                    { "id": "3", "tray_type": "PLA", "tray_uuid": "00000000000000000000000000000000" }
                ]
            }]
        });
        let vt = json!({ "id": "254", "tray_type": "TPU", "tray_color": "000000FF", "remain": 0 });
        parse(Some(&ams), Some(&vt))
    }

    #[test]
    fn trays() {
        let a = sample("1");
        let u = &a.units[0];
        assert_eq!(u.humidity.as_deref(), Some("4"));
        assert_eq!(u.trays[0].colour, Some([255, 0, 0, 255]));
        assert_eq!(u.trays[0].remain, Some(80));
        assert_eq!(u.trays[1].remain, None);
        assert_eq!(u.trays[1].colour, Some([0, 255, 0, 255]));
        assert!(u.trays[2].is_empty());
        // An all-zero uuid is an empty tray, whatever it says about material.
        assert!(u.trays[3].is_empty());
        assert_eq!(a.active_tray().map(|t| t.id), Some(1));
    }

    #[test]
    fn external_and_none() {
        assert_eq!(sample("254").active_tray().map(|t| t.id), Some(254));
        assert_eq!(sample("255").active_tray(), None);
        assert_eq!(parse(None, None), Ams::default());
    }
}
