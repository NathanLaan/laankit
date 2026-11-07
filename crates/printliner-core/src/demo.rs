//! Three made-up printers for `printliner --demo` and the UI tests, so the
//! dashboard can be developed and screenshotted with no Bambu account.
//!
//! The reports are a function of the seconds since the demo started:
//!
//! | Serial          | Printer        | Does |
//! |-----------------|----------------|------|
//! | `DEMO00X1C0001` | Workshop X1C   | Prints a 20-minute job, finishes, rests a minute, and starts again. |
//! | `DEMO00P1S0002` | Bench P1S      | Idle, with an AMS of four trays. |
//! | `DEMO000A10003` | Front Desk A1  | Failed, with a fatal HMS alert. |

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};

use crate::cloud::{Device, Task};

pub const X1C: &str = "DEMO00X1C0001";
pub const P1S: &str = "DEMO00P1S0002";
pub const A1: &str = "DEMO000A10003";

/// The demo job's length and the rest after it, in seconds.
const JOB_SECS: f64 = 20.0 * 60.0;
const REST_SECS: f64 = 60.0;

pub fn devices() -> Vec<Device> {
    let device = |serial: &str, name: &str, model: &str, product: &str| Device {
        serial: serial.into(),
        name: name.into(),
        model: model.into(),
        product: product.into(),
        online: true,
    };
    vec![
        device(X1C, "Workshop X1C", "BL-P001", "X1 Carbon"),
        device(P1S, "Bench P1S", "C12", "P1S"),
        device(A1, "Front Desk A1", "N2S", "A1"),
    ]
}

/// The printer's full report `secs` into the demo.
pub fn report(serial: &str, secs: f64) -> Value {
    let wobble = (secs / 7.0).sin() as f32 * 0.6;
    match serial {
        X1C => {
            // Start a third of the way in, so the first screen shows progress.
            let t = (secs + JOB_SECS / 3.0) % (JOB_SECS + REST_SECS);
            if t < JOB_SECS {
                let done = t / JOB_SECS;
                json!({ "print": {
                    "gcode_state": "RUNNING",
                    "mc_percent": (done * 100.0) as u32,
                    "mc_remaining_time": ((JOB_SECS - t) / 60.0).ceil() as u32,
                    "layer_num": (done * 240.0) as u32,
                    "total_layer_num": 240,
                    "subtask_name": "Cable clips x12",
                    "task_id": "900001",
                    "nozzle_temper": 220.0 + wobble,
                    "nozzle_target_temper": 220,
                    "bed_temper": 55.0 + wobble / 2.0,
                    "bed_target_temper": 55,
                    "chamber_temper": 33.0 + wobble / 3.0,
                    "spd_lvl": 2,
                    "wifi_signal": "-52dBm",
                    "print_error": 0,
                    "hms": [],
                    "ams": ams("0", ["PLA", "PLA", "PETG", ""], ["2F6F4FFF", "F2F2F2FF", "1A1A1AFF", ""]),
                }})
            } else {
                let cooled = ((t - JOB_SECS) / REST_SECS) as f32;
                json!({ "print": {
                    "gcode_state": "FINISH",
                    "mc_percent": 100,
                    "mc_remaining_time": 0,
                    "layer_num": 240,
                    "total_layer_num": 240,
                    "subtask_name": "Cable clips x12",
                    "task_id": "900001",
                    "nozzle_temper": 220.0 - 150.0 * cooled,
                    "nozzle_target_temper": 0,
                    "bed_temper": 55.0 - 20.0 * cooled,
                    "bed_target_temper": 0,
                    "chamber_temper": 33.0,
                    "spd_lvl": 2,
                    "wifi_signal": "-52dBm",
                    "print_error": 0,
                    "hms": [],
                    "ams": ams("255", ["PLA", "PLA", "PETG", ""], ["2F6F4FFF", "F2F2F2FF", "1A1A1AFF", ""]),
                }})
            }
        }
        P1S => json!({ "print": {
            "gcode_state": "IDLE",
            "mc_percent": 0,
            "mc_remaining_time": 0,
            "task_id": "0",
            "nozzle_temper": 26.0 + wobble,
            "nozzle_target_temper": 0,
            "bed_temper": 24.0 + wobble,
            "bed_target_temper": 0,
            "spd_lvl": 2,
            "wifi_signal": "-61dBm",
            "print_error": 0,
            "hms": [],
            "ams": ams("255", ["PLA", "PLA-CF", "PETG", "TPU"], ["D9381EFF", "333333FF", "1F5FBFFF", "F2C744FF"]),
        }}),
        A1 => json!({ "print": {
            "gcode_state": "FAILED",
            "mc_percent": 12,
            "mc_remaining_time": 0,
            "layer_num": 9,
            "total_layer_num": 120,
            "subtask_name": "Phone stand",
            "task_id": "900003",
            "nozzle_temper": 31.0 + wobble,
            "nozzle_target_temper": 0,
            "bed_temper": 28.0 + wobble,
            "bed_target_temper": 0,
            "spd_lvl": 2,
            "wifi_signal": "-70dBm",
            "print_error": 0x0300_8003_u32,
            "hms": [{ "attr": 0x0300_0100_u32, "code": 0x0001_0003_u32 }],
            "vt_tray": { "id": "254", "tray_type": "PLA", "tray_color": "FFFFFFFF", "remain": -1 },
            "ams": { "tray_now": "254" },
        }}),
        _ => json!({}),
    }
}

fn ams(tray_now: &str, materials: [&str; 4], colours: [&str; 4]) -> Value {
    const REMAIN: [i32; 4] = [80, 45, -1, 12];
    let trays: Vec<Value> = (0..4)
        .map(|i| {
            if materials[i].is_empty() {
                json!({ "id": i.to_string() })
            } else {
                json!({
                    "id": i.to_string(),
                    "tray_type": materials[i],
                    "tray_color": colours[i],
                    "remain": REMAIN[i],
                })
            }
        })
        .collect();
    json!({ "tray_now": tray_now, "ams": [{ "id": "0", "humidity": "3", "tray": trays }] })
}

/// A few past tasks for each printer, ending before `now`.
pub fn tasks(serial: &str, now: DateTime<Utc>) -> Vec<Task> {
    let titles: &[(&str, &str, f64, i64)] = match serial {
        X1C => &[
            ("Cable clips x12", "completed", 18.4, 1200),
            ("Hex key holder", "completed", 42.0, 5400),
            ("Bracket v3", "cancelled", 6.1, 900),
            ("Bracket v2", "completed", 31.9, 4300),
        ],
        P1S => &[
            ("Drawer labels", "completed", 9.3, 2100),
            ("Desk grommet", "completed", 15.0, 2700),
        ],
        A1 => &[
            ("Phone stand", "failed", 3.2, 600),
            ("Keychain", "completed", 2.4, 900),
        ],
        _ => &[],
    };
    let mut end = now - Duration::minutes(30);
    titles
        .iter()
        .enumerate()
        .map(|(i, (title, status, grams, secs))| {
            let start = end - Duration::seconds(*secs);
            let task = Task {
                id: format!("{serial}-{i}"),
                title: title.to_string(),
                cover: None,
                status: status.to_string(),
                weight_g: Some(*grams),
                duration_secs: Some(*secs),
                start: Some(start),
                end: Some(end),
                serial: serial.to_string(),
            };
            end = start - Duration::hours(5);
            task
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{GcodeState, PrinterStatus};

    #[test]
    fn printers_do_what_the_table_says() {
        let s = |serial, secs| PrinterStatus::from_state(&report(serial, secs));
        let x1c = s(X1C, 0.0);
        assert_eq!(x1c.state, GcodeState::Running);
        assert_eq!(x1c.percent, Some(33));
        assert_eq!(s(X1C, JOB_SECS * 2.0 / 3.0 + 1.0).state, GcodeState::Finish);
        assert_eq!(s(P1S, 0.0).state, GcodeState::Idle);
        assert_eq!(s(P1S, 0.0).ams.units[0].trays.len(), 4);
        let a1 = s(A1, 0.0);
        assert_eq!(a1.state, GcodeState::Failed);
        assert_eq!(a1.hms.len(), 1);
        assert_eq!(a1.ams.active_tray().map(|t| t.id), Some(254));
    }

    #[test]
    fn tasks_are_newest_first() {
        let now = Utc::now();
        let t = tasks(X1C, now);
        assert_eq!(t.len(), 4);
        assert!(t[0].start > t[1].start);
        assert!(t[0].end.unwrap() < now);
    }
}
