//! What the views show about a printer: the device from the account, its
//! latest status, and when it last reported.

use chrono::{DateTime, Utc};
use printliner_core::hms::Severity;
use printliner_core::{Device, GcodeState, PrinterStatus};

/// A printer with no report for this long shows as "No signal".
pub const STALE_SECS: i64 = 60;

#[derive(Clone, Debug)]
pub struct Printer {
    pub device: Device,
    /// The name PrintLiner shows: the one set in Settings, else the
    /// account's.
    pub name: String,
    pub status: Option<PrinterStatus>,
    /// When it last reported.
    pub at: Option<DateTime<Utc>>,
}

/// How a state reads at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Printing or preparing.
    Busy,
    /// Finished.
    Good,
    /// Paused, or no signal.
    Warn,
    /// Failed, or a serious alert.
    Bad,
    /// Idle, offline, or not known yet.
    Quiet,
}

impl Printer {
    /// Whether the last report is recent enough to trust.
    pub fn fresh(&self, now: DateTime<Utc>) -> bool {
        self.at
            .is_some_and(|t| (now - t).num_seconds() < STALE_SECS)
    }

    /// The state's words and tone, all things considered: the account's
    /// online flag, whether there's a report, and how old it is.
    pub fn state(&self, now: DateTime<Utc>, connected: bool) -> (&'static str, Tone) {
        if !self.device.online {
            return ("Offline", Tone::Quiet);
        }
        let Some(status) = &self.status else {
            return if connected {
                ("Waiting for status", Tone::Quiet)
            } else {
                ("Not connected", Tone::Quiet)
            };
        };
        if !connected {
            return ("Not connected", Tone::Quiet);
        }
        if !self.fresh(now) {
            return ("No signal", Tone::Warn);
        }
        let tone = match status.state {
            GcodeState::Prepare | GcodeState::Running => Tone::Busy,
            GcodeState::Pause => Tone::Warn,
            GcodeState::Finish => Tone::Good,
            GcodeState::Failed => Tone::Bad,
            GcodeState::Idle | GcodeState::Unknown => Tone::Quiet,
        };
        (status.state.label(), tone)
    }

    /// The most severe HMS alert's severity, if any.
    pub fn worst_alert(&self) -> Option<Severity> {
        self.status
            .as_ref()
            .and_then(|s| s.hms.iter().map(|h| h.severity()).min())
    }
}

/// `52 min`, `1 h 05 min`.
pub fn minutes(m: u32) -> String {
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02} min", m / 60, m % 60)
    }
}

/// `1 h 05 min` from seconds.
pub fn duration(secs: i64) -> String {
    minutes(u32::try_from((secs.max(0) + 59) / 60).unwrap_or(u32::MAX))
}

/// `220°` or `220° / 220°` with a target, `-` when unknown.
pub fn temperature(now: Option<f32>, target: Option<f32>) -> String {
    match (now, target) {
        (Some(n), Some(t)) if t > 0.0 => format!("{n:.0}° / {t:.0}°"),
        (Some(n), _) => format!("{n:.0}°"),
        (None, _) => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printer(online: bool, status: Option<GcodeState>, age_secs: i64) -> Printer {
        Printer {
            device: Device {
                serial: "S".into(),
                name: "P".into(),
                model: "C12".into(),
                product: "P1S".into(),
                online,
            },
            name: "P".into(),
            status: status.map(|state| PrinterStatus {
                state,
                ..Default::default()
            }),
            at: Some(Utc::now() - chrono::Duration::seconds(age_secs)),
        }
    }

    #[test]
    fn state_words() {
        let now = Utc::now();
        assert_eq!(
            printer(false, Some(GcodeState::Running), 0)
                .state(now, true)
                .0,
            "Offline"
        );
        assert_eq!(
            printer(true, None, 0).state(now, true).0,
            "Waiting for status"
        );
        assert_eq!(
            printer(true, Some(GcodeState::Running), 0)
                .state(now, false)
                .0,
            "Not connected"
        );
        assert_eq!(
            printer(true, Some(GcodeState::Running), 120).state(now, true),
            ("No signal", Tone::Warn)
        );
        assert_eq!(
            printer(true, Some(GcodeState::Running), 2).state(now, true),
            ("Printing", Tone::Busy)
        );
        assert_eq!(
            printer(true, Some(GcodeState::Failed), 2).state(now, true),
            ("Failed", Tone::Bad)
        );
    }

    #[test]
    fn formatting() {
        assert_eq!(minutes(52), "52 min");
        assert_eq!(minutes(65), "1 h 05 min");
        assert_eq!(duration(3600), "1 h 00 min");
        assert_eq!(duration(61), "2 min");
        assert_eq!(temperature(Some(219.6), Some(220.0)), "220° / 220°");
        assert_eq!(temperature(Some(25.2), Some(0.0)), "25°");
        assert_eq!(temperature(None, None), "-");
    }
}
