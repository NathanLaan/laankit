//! Status history for the trends view: a sample per printer every 60
//! seconds, appended to `<dir>/<serial>/<YYYY-MM>.jsonl`, one JSON object
//! per line. [`History::prune`] deletes months older than the keep period.

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::status::PrinterStatus;

/// How long samples are kept.
pub const KEEP_DAYS: i64 = 90;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub at: DateTime<Utc>,
    /// The `GcodeState` label.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nozzle: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bed: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamber: Option<f32>,
}

impl Sample {
    pub fn of(status: &PrinterStatus, at: DateTime<Utc>) -> Sample {
        Sample {
            at,
            state: status.state.label().to_string(),
            percent: status.state.is_active().then_some(status.percent).flatten(),
            nozzle: status.nozzle_temp,
            bed: status.bed_temp,
            chamber: status.chamber_temp,
        }
    }
}

#[derive(Clone, Debug)]
pub struct History {
    dir: PathBuf,
}

impl History {
    pub fn new(dir: impl Into<PathBuf>) -> History {
        History { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn printer_dir(&self, serial: &str) -> PathBuf {
        let safe: String = serial
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        self.dir
            .join(if safe.is_empty() { "unknown" } else { &safe })
    }

    pub fn append(&self, serial: &str, sample: &Sample) -> io::Result<()> {
        let dir = self.printer_dir(serial);
        fs::create_dir_all(&dir)?;
        let file = dir.join(month_file(sample.at));
        let mut line = serde_json::to_string(sample).map_err(io::Error::other)?;
        line.push('\n');
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)?
            .write_all(line.as_bytes())
    }

    /// The printer's samples from `since` on, oldest first. Unreadable lines
    /// (a write cut short) are skipped.
    pub fn read(&self, serial: &str, since: DateTime<Utc>) -> Vec<Sample> {
        let dir = self.printer_dir(serial);
        let first = month_file(since);
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".jsonl") && n >= first.as_str())
            })
            .collect();
        files.sort();
        let mut out = Vec::new();
        for file in files {
            let Ok(f) = fs::File::open(&file) else {
                continue;
            };
            for line in io::BufReader::new(f).lines().map_while(Result::ok) {
                if let Ok(s) = serde_json::from_str::<Sample>(&line) {
                    if s.at >= since {
                        out.push(s);
                    }
                }
            }
        }
        out
    }

    /// Deletes month files that end before `now - keep_days`.
    pub fn prune(&self, now: DateTime<Utc>, keep_days: i64) -> io::Result<()> {
        let cutoff = (now - Duration::days(keep_days)).date_naive();
        let Ok(printers) = fs::read_dir(&self.dir) else {
            return Ok(());
        };
        for printer in printers.flatten() {
            let Ok(files) = fs::read_dir(printer.path()) else {
                continue;
            };
            for f in files.flatten() {
                let name = f.file_name();
                let Some(month) = name.to_str().and_then(|n| n.strip_suffix(".jsonl")) else {
                    continue;
                };
                let Ok(start) = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
                else {
                    continue;
                };
                if next_month(start) <= cutoff {
                    fs::remove_file(f.path())?;
                }
            }
        }
        Ok(())
    }
}

fn month_file(at: DateTime<Utc>) -> String {
    format!("{:04}-{:02}.jsonl", at.year(), at.month())
}

fn next_month(d: NaiveDate) -> NaiveDate {
    let (y, m) = if d.month() == 12 {
        (d.year() + 1, 1)
    } else {
        (d.year(), d.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample(at: DateTime<Utc>, nozzle: f32) -> Sample {
        Sample {
            at,
            state: "Printing".into(),
            percent: Some(10),
            nozzle: Some(nozzle),
            bed: None,
            chamber: None,
        }
    }

    #[test]
    fn append_read_across_months() {
        let tmp = tempfile::tempdir().unwrap();
        let h = History::new(tmp.path());
        let oct = Utc.with_ymd_and_hms(2026, 10, 31, 23, 59, 0).unwrap();
        let nov = Utc.with_ymd_and_hms(2026, 11, 1, 0, 1, 0).unwrap();
        h.append("SER/../1", &sample(oct, 1.0)).unwrap();
        h.append("SER/../1", &sample(nov, 2.0)).unwrap();
        // The serial can't climb out of the folder.
        assert!(tmp.path().join("SER1").join("2026-10.jsonl").exists());
        let all = h.read("SER/../1", oct);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].nozzle, Some(2.0));
        let later = h.read("SER/../1", nov);
        assert_eq!(later, vec![sample(nov, 2.0)]);
        // A torn last line is skipped.
        let f = tmp.path().join("SER1").join("2026-11.jsonl");
        let mut text = fs::read_to_string(&f).unwrap();
        text.push_str("{\"at\":");
        fs::write(&f, text).unwrap();
        assert_eq!(h.read("SER/../1", nov).len(), 1);
    }

    #[test]
    fn prune_drops_old_months() {
        let tmp = tempfile::tempdir().unwrap();
        let h = History::new(tmp.path());
        let old = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
        let recent = Utc.with_ymd_and_hms(2026, 9, 15, 0, 0, 0).unwrap();
        h.append("A", &sample(old, 1.0)).unwrap();
        h.append("A", &sample(recent, 1.0)).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 11, 7, 0, 0, 0).unwrap();
        h.prune(now, KEEP_DAYS).unwrap();
        assert!(!tmp.path().join("A/2026-07.jsonl").exists());
        assert!(tmp.path().join("A/2026-09.jsonl").exists());
    }
}
