//! Lenient readers for the cloud's JSON, where a number may arrive as a
//! string and a timestamp as RFC 3339, epoch seconds, or epoch milliseconds.

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

/// A number, or a string holding one.
pub fn f64_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn i64_of(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A non-empty string, or a number written as one.
pub fn string_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// RFC 3339, or epoch seconds or milliseconds (as a number or a string).
pub fn time_of(v: &Value) -> Option<DateTime<Utc>> {
    if let Value::String(s) = v {
        if let Ok(t) = DateTime::parse_from_rfc3339(s.trim()) {
            return Some(t.with_timezone(&Utc));
        }
    }
    let n = i64_of(v)?;
    if n <= 0 {
        return None;
    }
    // Seconds reach 1e12 in the year 33658; milliseconds passed it in 2001.
    if n >= 1_000_000_000_000 {
        Utc.timestamp_millis_opt(n).single()
    } else {
        Utc.timestamp_opt(n, 0).single()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_and_strings() {
        assert_eq!(f64_of(&json!("21.5")), Some(21.5));
        assert_eq!(f64_of(&json!(3)), Some(3.0));
        assert_eq!(i64_of(&json!("42")), Some(42));
        assert_eq!(i64_of(&json!(null)), None);
        assert_eq!(string_of(&json!(12)), Some("12".into()));
        assert_eq!(string_of(&json!("  ")), None);
    }

    #[test]
    fn timestamps() {
        let t = Utc.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).unwrap();
        assert_eq!(time_of(&json!("2026-11-07T12:00:00Z")), Some(t));
        assert_eq!(time_of(&json!(t.timestamp())), Some(t));
        assert_eq!(time_of(&json!(t.timestamp_millis())), Some(t));
        assert_eq!(time_of(&json!(t.timestamp().to_string())), Some(t));
        assert_eq!(time_of(&json!(0)), None);
    }
}
