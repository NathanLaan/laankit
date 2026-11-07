//! A printer's status. Printers send a full report when asked (`pushall`)
//! and then mostly changes, so each report is merged into the last full
//! state ([`merge_json`]) and the result read with [`PrinterStatus::from_state`].

use serde_json::Value;

use crate::ams::{self, Ams};
use crate::hms::{self, Hms};
use crate::values::{f64_of, i64_of, string_of};

/// Deep-merges `patch` into `base`: objects merge key by key, everything
/// else (arrays included) replaces.
pub fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(existing) => merge_json(existing, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// `print.gcode_state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GcodeState {
    Idle,
    /// Heating, levelling, and the other steps before the first layer.
    Prepare,
    Running,
    Pause,
    Finish,
    Failed,
    /// Nothing reported yet, or a word PrintLiner doesn't know.
    #[default]
    Unknown,
}

impl GcodeState {
    pub fn parse(s: &str) -> GcodeState {
        match s.trim().to_ascii_uppercase().as_str() {
            "IDLE" => GcodeState::Idle,
            "PREPARE" | "SLICING" => GcodeState::Prepare,
            "RUNNING" => GcodeState::Running,
            "PAUSE" => GcodeState::Pause,
            "FINISH" => GcodeState::Finish,
            "FAILED" => GcodeState::Failed,
            _ => GcodeState::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GcodeState::Idle => "Idle",
            GcodeState::Prepare => "Preparing",
            GcodeState::Running => "Printing",
            GcodeState::Pause => "Paused",
            GcodeState::Finish => "Finished",
            GcodeState::Failed => "Failed",
            GcodeState::Unknown => "Unknown",
        }
    }

    /// A job is under way (including paused).
    pub fn is_active(self) -> bool {
        matches!(
            self,
            GcodeState::Prepare | GcodeState::Running | GcodeState::Pause
        )
    }

    /// Nothing is printing: idle, or the last job ended.
    pub fn is_ready(self) -> bool {
        matches!(
            self,
            GcodeState::Idle | GcodeState::Finish | GcodeState::Failed
        )
    }
}

/// A printer's status, read from its merged report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrinterStatus {
    pub state: GcodeState,
    pub percent: Option<u8>,
    pub remaining_min: Option<u32>,
    pub layer: Option<u32>,
    pub total_layers: Option<u32>,
    /// `subtask_name`, else `gcode_file`.
    pub job_name: Option<String>,
    /// `task_id`, else `subtask_id`; `None` when there's no job.
    pub job_id: Option<String>,
    /// `mc_print_stage`, as reported.
    pub stage: Option<String>,
    pub nozzle_temp: Option<f32>,
    pub nozzle_target: Option<f32>,
    pub bed_temp: Option<f32>,
    pub bed_target: Option<f32>,
    pub chamber_temp: Option<f32>,
    /// `spd_lvl`: 1 silent, 2 standard, 3 sport, 4 ludicrous.
    pub speed_level: Option<u8>,
    /// `wifi_signal`, e.g. `-45dBm`.
    pub wifi_signal: Option<String>,
    /// `print_error`, when not 0.
    pub print_error: Option<i64>,
    pub hms: Vec<Hms>,
    pub ams: Ams,
}

impl PrinterStatus {
    /// Reads a merged report (`{"print": {...}}`).
    pub fn from_state(state: &Value) -> PrinterStatus {
        let Some(p) = state.get("print") else {
            return PrinterStatus::default();
        };
        let num = |k: &str| p.get(k).and_then(f64_of);
        let int = |k: &str| p.get(k).and_then(i64_of);
        let text = |k: &str| p.get(k).and_then(string_of);
        let u32_of = |k: &str| int(k).and_then(|n| u32::try_from(n).ok());
        let temp = |k: &str| num(k).map(|t| t as f32);
        let job_id = text("task_id")
            .filter(|id| id != "0")
            .or_else(|| text("subtask_id").filter(|id| id != "0"));
        PrinterStatus {
            state: text("gcode_state")
                .map(|s| GcodeState::parse(&s))
                .unwrap_or_default(),
            percent: int("mc_percent")
                .and_then(|n| u8::try_from(n).ok())
                .map(|n| n.min(100)),
            remaining_min: u32_of("mc_remaining_time"),
            layer: u32_of("layer_num"),
            total_layers: u32_of("total_layer_num").filter(|n| *n > 0),
            job_name: text("subtask_name").or_else(|| text("gcode_file")),
            job_id,
            stage: text("mc_print_stage"),
            nozzle_temp: temp("nozzle_temper"),
            nozzle_target: temp("nozzle_target_temper"),
            bed_temp: temp("bed_temper"),
            bed_target: temp("bed_target_temper"),
            chamber_temp: temp("chamber_temper"),
            speed_level: int("spd_lvl").and_then(|n| u8::try_from(n).ok()),
            wifi_signal: text("wifi_signal"),
            print_error: int("print_error").filter(|e| *e != 0),
            hms: p.get("hms").map(hms::parse).unwrap_or_default(),
            ams: ams::parse(p.get("ams"), p.get("vt_tray")),
        }
    }

    pub fn speed_label(&self) -> Option<&'static str> {
        Some(match self.speed_level? {
            1 => "Silent",
            2 => "Standard",
            3 => "Sport",
            4 => "Ludicrous",
            _ => return None,
        })
    }
}

/// The `pushall` request, which asks a printer for its full report.
pub fn pushall(sequence_id: u64) -> String {
    format!(
        r#"{{"pushing":{{"sequence_id":"{sequence_id}","command":"pushall","version":1,"push_target":1}}}}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn merge_replaces_arrays_and_merges_objects() {
        let mut base = json!({ "print": { "a": 1, "list": [1, 2], "nested": { "x": 1 } } });
        merge_json(
            &mut base,
            &json!({ "print": { "list": [3], "nested": { "y": 2 }, "b": "new" } }),
        );
        assert_eq!(
            base,
            json!({ "print": { "a": 1, "list": [3], "nested": { "x": 1, "y": 2 }, "b": "new" } })
        );
    }

    #[test]
    fn full_report() {
        let s = PrinterStatus::from_state(&fixture("report_full.json"));
        assert_eq!(s.state, GcodeState::Running);
        assert_eq!(s.percent, Some(43));
        assert_eq!(s.remaining_min, Some(52));
        assert_eq!((s.layer, s.total_layers), (Some(87), Some(200)));
        assert_eq!(s.job_name.as_deref(), Some("Cable clips"));
        assert_eq!(s.job_id.as_deref(), Some("123456789"));
        assert_eq!(s.nozzle_temp, Some(219.8));
        assert_eq!(s.bed_target, Some(55.0));
        assert_eq!(s.speed_label(), Some("Standard"));
        assert_eq!(s.wifi_signal.as_deref(), Some("-48dBm"));
        assert_eq!(s.print_error, None);
        assert_eq!(s.hms.len(), 1);
        assert_eq!(s.ams.units.len(), 1);
        assert_eq!(s.ams.active_tray().map(|t| t.id), Some(0));
    }

    #[test]
    fn delta_merges_into_full_state() {
        let mut state = fixture("report_full.json");
        merge_json(&mut state, &fixture("report_delta.json"));
        let s = PrinterStatus::from_state(&state);
        // Changed by the delta, as strings…
        assert_eq!(s.percent, Some(44));
        assert_eq!(s.nozzle_temp, Some(220.1));
        // …and kept from the full report.
        assert_eq!(s.job_name.as_deref(), Some("Cable clips"));
        assert_eq!(s.ams.units.len(), 1);
        // The delta's empty hms list clears the alert.
        assert!(s.hms.is_empty());
    }

    #[test]
    fn no_job_and_empty_report() {
        let s = PrinterStatus::from_state(&json!({ "print": {
            "gcode_state": "IDLE", "task_id": "0", "subtask_id": "0", "print_error": 0
        }}));
        assert_eq!(s.state, GcodeState::Idle);
        assert!(s.state.is_ready());
        assert_eq!(s.job_id, None);
        assert_eq!(s.print_error, None);
        assert_eq!(
            PrinterStatus::from_state(&json!({})),
            PrinterStatus::default()
        );
    }

    #[test]
    fn pushall_message() {
        let v: Value = serde_json::from_str(&pushall(7)).unwrap();
        assert_eq!(v["pushing"]["command"], "pushall");
        assert_eq!(v["pushing"]["sequence_id"], "7");
    }
}
