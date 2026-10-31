//! YAML with js-yaml 3 semantics — the engine behind gray-matter, which the
//! Electron build used for frontmatter. Loading follows js-yaml's default
//! safe schema (YAML 1.1-style implicit types, timestamps → dates), and
//! `dump` is a port of js-yaml 3's `safeDump`, so notes written by either
//! build come out byte-identical and never churn in git.

use chrono::{DateTime, NaiveDate, SecondsFormat, TimeZone, Utc};
use serde_json::{Map, Number, Value};
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::TScalarStyle;

#[derive(Debug, Clone, PartialEq)]
pub enum Yaml {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// An unquoted YAML timestamp (a JS `Date` in js-yaml).
    Date(DateTime<Utc>),
    Seq(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

impl Yaml {
    pub fn get(&self, key: &str) -> Option<&Yaml> {
        match self {
            Yaml::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// JSON view (what the renderer receives). Dates become ISO strings.
    pub fn to_json(&self) -> Value {
        match self {
            Yaml::Null => Value::Null,
            Yaml::Bool(b) => Value::Bool(*b),
            Yaml::Int(i) => Value::Number((*i).into()),
            Yaml::Float(f) => Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            Yaml::Str(s) => Value::String(s.clone()),
            Yaml::Date(d) => Value::String(iso(d)),
            Yaml::Seq(s) => Value::Array(s.iter().map(Yaml::to_json).collect()),
            Yaml::Map(m) => Value::Object(
                m.iter()
                    .map(|(k, v)| (k.clone(), v.to_json()))
                    .collect::<Map<_, _>>(),
            ),
        }
    }

    pub fn from_json(v: &Value) -> Yaml {
        match v {
            Value::Null => Yaml::Null,
            Value::Bool(b) => Yaml::Bool(*b),
            Value::Number(n) => match n.as_i64() {
                Some(i) => Yaml::Int(i),
                None => {
                    let f = n.as_f64().unwrap_or(0.0);
                    if f.fract() == 0.0 && f.abs() < 9.0e15 {
                        Yaml::Int(f as i64)
                    } else {
                        Yaml::Float(f)
                    }
                }
            },
            Value::String(s) => Yaml::Str(s.clone()),
            Value::Array(a) => Yaml::Seq(a.iter().map(Yaml::from_json).collect()),
            Value::Object(o) => Yaml::Map(
                o.iter()
                    .map(|(k, v)| (k.clone(), Yaml::from_json(v)))
                    .collect(),
            ),
        }
    }
}

pub fn iso(d: &DateTime<Utc>) -> String {
    d.to_rfc3339_opts(SecondsFormat::Millis, true)
}

// --- Loading ------------------------------------------------------------------

/// Parse one YAML document (js-yaml `safeLoad`). Empty input loads as Null.
pub fn load(src: &str) -> Result<Yaml, String> {
    let mut parser = Parser::new_from_str(src);
    let mut b = Builder::default();
    loop {
        let (ev, mark) = parser.next_token().map_err(|e| e.to_string())?;
        match ev {
            Event::StreamEnd => break,
            Event::DocumentStart => {
                b.docs += 1;
                if b.docs > 1 {
                    return Err("expected a single document in the stream, but found more".into());
                }
            }
            ev => b
                .event(ev)
                .map_err(|e| format!("{e} at line {}", mark.line()))?,
        }
    }
    Ok(b.root.unwrap_or(Yaml::Null))
}

enum Frame {
    Seq(usize, Vec<Yaml>),
    Map(usize, Vec<(String, Yaml)>, Option<(String, bool)>),
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    anchors: std::collections::HashMap<usize, Yaml>,
    root: Option<Yaml>,
    docs: usize,
}

impl Builder {
    fn event(&mut self, ev: Event) -> Result<(), String> {
        match ev {
            Event::Scalar(value, style, anchor, tag) => {
                let is_plain = style == TScalarStyle::Plain;
                let forced_str = tag.as_ref().is_some_and(|t| t.suffix == "str");
                let node = if is_plain && !forced_str {
                    resolve_plain(&value)
                } else {
                    Yaml::Str(value.clone())
                };
                // A plain `<<` in key position is a merge key.
                let merge = is_plain && value == "<<";
                self.push(node, anchor, merge)
            }
            Event::SequenceStart(anchor, _) => {
                self.stack.push(Frame::Seq(anchor, Vec::new()));
                Ok(())
            }
            Event::MappingStart(anchor, _) => {
                self.stack.push(Frame::Map(anchor, Vec::new(), None));
                Ok(())
            }
            Event::SequenceEnd => match self.stack.pop() {
                Some(Frame::Seq(anchor, items)) => self.push(Yaml::Seq(items), anchor, false),
                _ => Err("unbalanced sequence".into()),
            },
            Event::MappingEnd => match self.stack.pop() {
                Some(Frame::Map(anchor, items, _)) => self.push(Yaml::Map(items), anchor, false),
                _ => Err("unbalanced mapping".into()),
            },
            Event::Alias(id) => {
                let node = self.anchors.get(&id).cloned().ok_or("unidentified alias")?;
                self.push(node, 0, false)
            }
            _ => Ok(()),
        }
    }

    fn push(&mut self, node: Yaml, anchor: usize, merge_key: bool) -> Result<(), String> {
        if anchor > 0 {
            self.anchors.insert(anchor, node.clone());
        }
        match self.stack.last_mut() {
            None => {
                self.root = Some(node);
                Ok(())
            }
            Some(Frame::Seq(_, items)) => {
                items.push(node);
                Ok(())
            }
            Some(Frame::Map(_, items, pending)) => match pending.take() {
                None => {
                    *pending = Some((js_key(&node), merge_key));
                    Ok(())
                }
                Some((_, true)) => {
                    let sources = match node {
                        Yaml::Map(m) => vec![m],
                        Yaml::Seq(s) => s
                            .into_iter()
                            .map(|n| match n {
                                Yaml::Map(m) => Ok(m),
                                _ => Err("cannot merge mappings; the provided source object is unacceptable"),
                            })
                            .collect::<Result<_, _>>()?,
                        _ => return Err("cannot merge mappings; the provided source object is unacceptable".into()),
                    };
                    for src in sources {
                        for (k, v) in src {
                            if !items.iter().any(|(ek, _)| *ek == k) {
                                items.push((k, v));
                            }
                        }
                    }
                    Ok(())
                }
                Some((key, false)) => {
                    if items.iter().any(|(k, _)| *k == key) {
                        return Err("duplicated mapping key".into());
                    }
                    items.push((key, node));
                    Ok(())
                }
            },
        }
    }
}

/// js-yaml stringifies mapping keys (`String(key)` in JS).
fn js_key(node: &Yaml) -> String {
    match node {
        Yaml::Null => "null".into(),
        Yaml::Bool(b) => b.to_string(),
        Yaml::Int(i) => i.to_string(),
        Yaml::Float(f) => js_number(*f),
        Yaml::Str(s) => s.clone(),
        Yaml::Date(d) => iso(d),
        Yaml::Seq(s) => s.iter().map(js_key).collect::<Vec<_>>().join(","),
        Yaml::Map(_) => "[object Object]".into(),
    }
}

fn resolve_plain(s: &str) -> Yaml {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Yaml::Null,
        "true" | "True" | "TRUE" => return Yaml::Bool(true),
        "false" | "False" | "FALSE" => return Yaml::Bool(false),
        _ => {}
    }
    if is_yaml_int(s) {
        if let Some(i) = construct_int(s) {
            return Yaml::Int(i);
        }
    }
    if is_yaml_float(s) {
        return Yaml::Float(construct_float(s));
    }
    if let Some(d) = construct_timestamp(s) {
        return Yaml::Date(d);
    }
    Yaml::Str(s.to_string())
}

fn is_yaml_int(data: &str) -> bool {
    let b = data.as_bytes();
    let max = b.len();
    if max == 0 {
        return false;
    }
    let mut i = 0;
    if b[0] == b'-' || b[0] == b'+' {
        i += 1;
    }
    if i < max && b[i] == b'0' {
        if i + 1 == max {
            return true;
        }
        i += 1;
        let (digits_ok, start): (fn(u8) -> bool, usize) = match b[i] {
            b'b' => (|c| c == b'0' || c == b'1', i + 1),
            b'x' => (|c: u8| c.is_ascii_hexdigit(), i + 1),
            _ => (|c| (b'0'..=b'7').contains(&c), i),
        };
        let mut has = false;
        let mut last = 0u8;
        for &c in &b[start..] {
            last = c;
            if c == b'_' {
                continue;
            }
            if !digits_ok(c) {
                return false;
            }
            has = true;
        }
        return has && last != b'_';
    }
    if i < max && b[i] == b'_' {
        return false;
    }
    let mut has = false;
    let mut last = 0u8;
    while i < max {
        let c = b[i];
        last = c;
        if c == b'_' {
            i += 1;
            continue;
        }
        if c == b':' {
            break;
        }
        if !c.is_ascii_digit() {
            return false;
        }
        has = true;
        i += 1;
    }
    if !has || last == b'_' {
        return false;
    }
    if last != b':' {
        return true;
    }
    // Sexagesimal: (:[0-5]?[0-9])+
    data[i..].split(':').skip(1).all(|p| {
        let p = p.as_bytes();
        match p.len() {
            1 => p[0].is_ascii_digit(),
            2 => (b'0'..=b'5').contains(&p[0]) && p[1].is_ascii_digit(),
            _ => false,
        }
    }) && !data[i..].ends_with(':')
}

fn construct_int(data: &str) -> Option<i64> {
    let mut v: String = data.replace('_', "");
    let mut sign = 1i64;
    if v.starts_with('-') || v.starts_with('+') {
        if v.starts_with('-') {
            sign = -1;
        }
        v.remove(0);
    }
    if v == "0" {
        return Some(0);
    }
    if let Some(bin) = v.strip_prefix("0b") {
        return i64::from_str_radix(bin, 2).ok().map(|n| sign * n);
    }
    if let Some(hex) = v.strip_prefix("0x") {
        return i64::from_str_radix(hex, 16).ok().map(|n| sign * n);
    }
    if v.starts_with('0') {
        return i64::from_str_radix(&v, 8).ok().map(|n| sign * n);
    }
    if v.contains(':') {
        let mut value = 0i64;
        for part in v.split(':') {
            value = value * 60 + part.parse::<i64>().ok()?;
        }
        return Some(sign * value);
    }
    v.parse::<i64>().ok().map(|n| sign * n)
}

fn is_yaml_float(s: &str) -> bool {
    if s.ends_with('_') {
        return false;
    }
    let b = s.as_bytes();
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(s, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    let digits_us = |t: &str| t.bytes().all(|c| c.is_ascii_digit() || c == b'_');
    let exp_ok = |t: &str| {
        let t = t.strip_prefix(['-', '+']).unwrap_or(t);
        !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit())
    };
    // [-+]?(0|[1-9][0-9_]*)(\.[0-9_]*)?([eE][-+]?[0-9]+)?
    let (mant, exp) = match unsigned.find(['e', 'E']) {
        Some(i) => (&unsigned[..i], Some(&unsigned[i + 1..])),
        None => (unsigned, None),
    };
    if exp.is_some_and(|e| !exp_ok(e)) {
        return false;
    }
    let (int, frac) = match mant.find('.') {
        Some(i) => (&mant[..i], Some(&mant[i + 1..])),
        None => (mant, None),
    };
    let int_ok = int == "0"
        || (int
            .as_bytes()
            .first()
            .is_some_and(|c| (b'1'..=b'9').contains(c))
            && digits_us(int));
    if int_ok && frac.is_none_or(digits_us) {
        // Plain integers are ints, not floats; js-yaml checks int first.
        return true;
    }
    // \.[0-9_]+([eE][-+]?[0-9]+)?  (no sign)
    if b.first() == Some(&b'.')
        && int.is_empty()
        && frac.is_some_and(|f| !f.is_empty() && digits_us(f))
    {
        return true;
    }
    // Sexagesimal float: [-+]?[0-9][0-9_]*(:[0-5]?[0-9])+\.[0-9_]*
    if exp.is_none() {
        if let (Some(dot), true) = (mant.find('.'), mant.contains(':')) {
            let head = &mant[..dot];
            let mut parts = head.split(':');
            let first = parts.next().unwrap_or("");
            let first_ok =
                first.as_bytes().first().is_some_and(u8::is_ascii_digit) && digits_us(first);
            let rest_ok = parts.all(|p| {
                let p = p.as_bytes();
                match p.len() {
                    1 => p[0].is_ascii_digit(),
                    2 => (b'0'..=b'5').contains(&p[0]) && p[1].is_ascii_digit(),
                    _ => false,
                }
            });
            return first_ok && rest_ok && digits_us(&mant[dot + 1..]);
        }
    }
    false
}

fn construct_float(s: &str) -> f64 {
    let v = s.replace('_', "").to_lowercase();
    let sign = if v.starts_with('-') { -1.0 } else { 1.0 };
    let v = v.trim_start_matches(['-', '+']);
    match v {
        ".inf" => sign * f64::INFINITY,
        ".nan" => f64::NAN,
        _ if v.contains(':') => {
            let mut value = 0.0;
            for p in v.split(':') {
                value = value * 60.0 + p.parse::<f64>().unwrap_or(0.0);
            }
            sign * value
        }
        _ => sign * v.parse::<f64>().unwrap_or(f64::NAN),
    }
}

fn construct_timestamp(s: &str) -> Option<DateTime<Utc>> {
    let b = s.as_bytes();
    // ^\d{4}-\d\d-\d\d$
    if b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && s.bytes()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        let d = NaiveDate::from_ymd_opt(
            s[0..4].parse().ok()?,
            s[5..7].parse().ok()?,
            s[8..10].parse().ok()?,
        )?;
        return Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?));
    }
    // ^\d{4}-\d\d?-\d\d?([Tt]|[ \t]+)\d\d?:\d\d:\d\d(\.\d*)?([ \t]*(Z|[-+]\d\d?(:\d\d)?))?$
    let num = |s: &str, lo: usize, hi: usize| -> Option<u32> {
        (s.len() >= lo && s.len() <= hi && s.bytes().all(|c| c.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    let (date, rest) = s.split_at(s.find(['T', 't', ' ', '\t'])?);
    let mut dp = date.split('-');
    let (y, mo, d) = (
        num(dp.next()?, 4, 4)?,
        num(dp.next()?, 1, 2)?,
        num(dp.next()?, 1, 2)?,
    );
    if dp.next().is_some() {
        return None;
    }
    let rest = if rest.starts_with(['T', 't']) {
        &rest[1..]
    } else {
        rest.trim_start_matches([' ', '\t'])
    };
    let tz_at = rest.find(['Z', '+', '-', ' ', '\t']).unwrap_or(rest.len());
    let (time, tz) = rest.split_at(tz_at);
    let (hms, frac) = match time.split_once('.') {
        Some((h, f)) => (h, Some(f)),
        None => (time, None),
    };
    let mut tp = hms.split(':');
    let (h, mi, sec) = (
        num(tp.next()?, 1, 2)?,
        num(tp.next()?, 2, 2)?,
        num(tp.next()?, 2, 2)?,
    );
    if tp.next().is_some() || frac.is_some_and(|f| !f.bytes().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let ms: u32 = frac
        .map(|f| format!("{:0<3}", &f[..f.len().min(3)]).parse().unwrap_or(0))
        .unwrap_or(0);
    let tz = tz.trim_start_matches([' ', '\t']);
    let offset_min: i64 = match tz {
        "" | "Z" => 0,
        _ => {
            let sign = if tz.starts_with('-') {
                -1
            } else if tz.starts_with('+') {
                1
            } else {
                return None;
            };
            let body = &tz[1..];
            let (th, tm) = match body.split_once(':') {
                Some((a, b)) => (num(a, 1, 2)?, num(b, 2, 2)?),
                None => (num(body, 1, 2)?, 0),
            };
            sign * (th as i64 * 60 + tm as i64)
        }
    };
    let base = Utc.from_utc_datetime(
        &NaiveDate::from_ymd_opt(y as i32, mo, d)?.and_hms_milli_opt(h, mi, sec, ms)?,
    );
    Some(base - chrono::Duration::minutes(offset_min))
}

// --- Dumping (js-yaml 3 safeDump) --------------------------------------------

const INDENT: usize = 2;
const LINE_WIDTH: usize = 80;

/// `yaml.safeDump(value)` with js-yaml 3 defaults.
pub fn dump(v: &Yaml) -> String {
    let mut out = write_node(0, v, true, true, false);
    out.push('\n');
    out
}

fn js_number(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if f.fract() == 0.0 && f.abs() < 1e21 {
        return format!("{}", f as i128);
    }
    let abs = f.abs();
    if (1e-7..1e21).contains(&abs) {
        return format!("{f}");
    }
    // JS switches to exponent form: 1.5e+21, 1e-7.
    let s = format!("{f:e}");
    match s.split_once('e') {
        Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
        _ => s,
    }
}

fn represent_float(f: f64) -> String {
    if f.is_nan() {
        return ".nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            ".inf".into()
        } else {
            "-.inf".into()
        };
    }
    if f == 0.0 && f.is_sign_negative() {
        return "-0.0".into();
    }
    let res = js_number(f);
    // SCIENTIFIC_WITHOUT_DOT: /^[-+]?[0-9]+e/
    let unsigned = res.trim_start_matches(['-', '+']);
    let digits = unsigned.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && unsigned.as_bytes().get(digits) == Some(&b'e') {
        res.replacen('e', ".e", 1)
    } else {
        res
    }
}

fn next_line(level: usize) -> String {
    format!("\n{}", " ".repeat(INDENT * level))
}

/// Returns the rendered node, mirroring js-yaml's `writeNode` (`state.dump`).
fn write_node(level: usize, v: &Yaml, block: bool, compact: bool, is_key: bool) -> String {
    match v {
        Yaml::Null => "null".into(),
        Yaml::Bool(b) => b.to_string(),
        // JS has one number type: integral values use the int representer.
        Yaml::Int(i) => i.to_string(),
        Yaml::Float(f)
            if f.fract() == 0.0 && !(*f == 0.0 && f.is_sign_negative()) && f.is_finite() =>
        {
            js_number(*f)
        }
        Yaml::Float(f) => represent_float(*f),
        Yaml::Date(d) => iso(d),
        Yaml::Str(s) => write_scalar(s, level, is_key),
        Yaml::Seq(items) => {
            if block && !items.is_empty() {
                let mut r = String::new();
                for (i, item) in items.iter().enumerate() {
                    let dump = write_node(level + 1, item, true, true, false);
                    if !compact || i != 0 {
                        r += &next_line(level);
                    }
                    r += if dump.starts_with('\n') { "-" } else { "- " };
                    r += &dump;
                }
                r
            } else {
                let parts: Vec<String> = items
                    .iter()
                    .map(|i| write_node(level, i, false, false, false))
                    .collect();
                format!("[{}]", parts.join(", "))
            }
        }
        Yaml::Map(pairs) => {
            if block && !pairs.is_empty() {
                let mut r = String::new();
                for (i, (k, val)) in pairs.iter().enumerate() {
                    let mut pair = String::new();
                    if !compact || i != 0 {
                        pair += &next_line(level);
                    }
                    let key = write_node(level + 1, &Yaml::Str(k.clone()), true, true, true);
                    let explicit = key.len() > 1024;
                    if explicit {
                        pair += if key.starts_with('\n') { "?" } else { "? " };
                    }
                    pair += &key;
                    if explicit {
                        pair += &next_line(level);
                    }
                    let dump = write_node(level + 1, val, true, explicit, false);
                    pair += if dump.starts_with('\n') { ":" } else { ": " };
                    pair += &dump;
                    r += &pair;
                }
                r
            } else {
                let parts: Vec<String> = pairs
                    .iter()
                    .map(|(k, val)| {
                        let key = write_node(level, &Yaml::Str(k.clone()), false, false, false);
                        let q = if key.len() > 1024 { "? " } else { "" };
                        format!("{q}{key}: {}", write_node(level, val, false, false, false))
                    })
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
        }
    }
}

const DEPRECATED_BOOLEANS: [&str; 16] = [
    "y", "Y", "yes", "Yes", "YES", "on", "On", "ON", "n", "N", "no", "No", "NO", "off", "Off",
    "OFF",
];

fn is_white(c: u16) -> bool {
    c == 0x20 || c == 0x09
}

fn is_printable(c: u16) -> bool {
    (0x20..=0x7E).contains(&c)
        || ((0xA1..=0xD7FF).contains(&c) && c != 0x2028 && c != 0x2029)
        || ((0xE000..=0xFFFD).contains(&c) && c != 0xFEFF)
}

fn is_ns_char(c: u16) -> bool {
    is_printable(c) && !is_white(c) && c != 0xFEFF && c != 0x0D && c != 0x0A
}

fn is_plain_safe(c: u16, prev: Option<u16>) -> bool {
    is_printable(c)
        && c != 0xFEFF
        && !matches!(c, 0x2C | 0x5B | 0x5D | 0x7B | 0x7D | 0x3A)
        && (c != 0x23 || prev.is_some_and(is_ns_char))
}

fn is_plain_safe_first(c: u16) -> bool {
    is_printable(c)
        && c != 0xFEFF
        && !is_white(c)
        && !matches!(
            c,
            0x2D | 0x3F
                | 0x3A
                | 0x2C
                | 0x5B
                | 0x5D
                | 0x7B
                | 0x7D
                | 0x23
                | 0x26
                | 0x2A
                | 0x21
                | 0x7C
                | 0x3D
                | 0x3E
                | 0x27
                | 0x22
                | 0x25
                | 0x40
                | 0x60
        )
}

/// Would this plain string load as a non-string (null/bool/int/float/date/merge)?
fn is_ambiguous(s: &str) -> bool {
    !matches!(resolve_plain(s), Yaml::Str(_)) || s == "<<"
}

enum Style {
    Plain,
    Single,
    Literal,
    Folded,
    Double,
}

fn choose_style(s: &[u16], single_line_only: bool, line_width: usize, text: &str) -> Style {
    let mut has_line_break = false;
    let mut has_foldable = false;
    let mut prev_break: isize = -1;
    let mut plain = is_plain_safe_first(s[0]) && !is_white(s[s.len() - 1]);
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if !single_line_only && c == 0x0A {
            has_line_break = true;
            let line_len = i as isize - prev_break - 1;
            has_foldable = has_foldable
                || (line_len > line_width as isize && s[(prev_break + 1) as usize] != 0x20);
            prev_break = i as isize;
        } else if !is_printable(c) {
            return Style::Double;
        }
        let prev = if i > 0 { Some(s[i - 1]) } else { None };
        plain = plain && is_plain_safe(c, prev);
        i += 1;
    }
    if !single_line_only {
        let line_len = i as isize - prev_break - 1;
        has_foldable = has_foldable
            || (line_len > line_width as isize
                && s.get((prev_break + 1) as usize).copied() != Some(0x20));
    }
    if !has_line_break && !has_foldable {
        return if plain && !is_ambiguous(text) {
            Style::Plain
        } else {
            Style::Single
        };
    }
    if has_foldable {
        Style::Folded
    } else {
        Style::Literal
    }
}

fn write_scalar(text: &str, level: usize, is_key: bool) -> String {
    if text.is_empty() {
        return "''".into();
    }
    if DEPRECATED_BOOLEANS.contains(&text) {
        return format!("'{text}'");
    }
    let indent = INDENT * level.max(1);
    let line_width = LINE_WIDTH.min(40).max(LINE_WIDTH.saturating_sub(indent));
    let units: Vec<u16> = text.encode_utf16().collect();
    match choose_style(&units, is_key, line_width, text) {
        Style::Plain => text.to_string(),
        Style::Single => format!("'{}'", text.replace('\'', "''")),
        Style::Literal => format!(
            "|{}{}",
            block_header(&units),
            drop_ending_newline(&indent_string(&units, indent))
        ),
        Style::Folded => {
            let folded = fold_string(&units, line_width);
            format!(
                ">{}{}",
                block_header(&units),
                drop_ending_newline(&indent_string(&folded, indent))
            )
        }
        Style::Double => format!("\"{}\"", escape_string(&units)),
    }
}

fn block_header(s: &[u16]) -> String {
    let leading_nl = s.iter().take_while(|&&c| c == 0x0A).count();
    let indicator = if s.get(leading_nl) == Some(&0x20) {
        INDENT.to_string()
    } else {
        String::new()
    };
    let clip = s.last() == Some(&0x0A);
    let keep = clip && (s.len() >= 2 && s[s.len() - 2] == 0x0A || s.len() == 1);
    let chomp = if keep {
        "+"
    } else if clip {
        ""
    } else {
        "-"
    };
    format!("{indicator}{chomp}\n")
}

fn drop_ending_newline(s: &str) -> &str {
    s.strip_suffix('\n').unwrap_or(s)
}

fn indent_string(s: &[u16], spaces: usize) -> String {
    let ind = " ".repeat(spaces);
    let text = String::from_utf16_lossy(s);
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if !line.is_empty() && line != "\n" {
            out += &ind;
        }
        out += line;
    }
    out
}

fn fold_string(s: &[u16], width: usize) -> Vec<u16> {
    let first_lf = s.iter().position(|&c| c == 0x0A).unwrap_or(s.len());
    let mut result = fold_line(&s[..first_lf], width);
    let mut prev_more_indented = s.first() == Some(&0x0A) || s.first() == Some(&0x20);
    let mut i = first_lf;
    // /(\n+)([^\n]*)/g
    while i < s.len() {
        let start = i;
        while i < s.len() && s[i] == 0x0A {
            i += 1;
        }
        let prefix = &s[start..i];
        let line_start = i;
        while i < s.len() && s[i] != 0x0A {
            i += 1;
        }
        let line = &s[line_start..i];
        let more_indented = line.first() == Some(&0x20);
        result.extend_from_slice(prefix);
        if !prev_more_indented && !more_indented && !line.is_empty() {
            result.push(0x0A);
        }
        result.extend(fold_line(line, width));
        prev_more_indented = more_indented;
    }
    result
}

fn fold_line(line: &[u16], width: usize) -> Vec<u16> {
    if line.is_empty() || line[0] == 0x20 {
        return line.to_vec();
    }
    // breakRe = / [^ ]/g
    let breaks: Vec<usize> = (0..line.len().saturating_sub(1))
        .filter(|&i| line[i] == 0x20 && line[i + 1] != 0x20)
        .collect();
    let (mut start, mut curr) = (0usize, 0usize);
    let mut result: Vec<u16> = Vec::new();
    for next in breaks {
        if next - start > width {
            let end = if curr > start { curr } else { next };
            result.push(0x0A);
            result.extend_from_slice(&line[start..end]);
            start = end + 1;
        }
        curr = next;
    }
    result.push(0x0A);
    if line.len() - start > width && curr > start {
        result.extend_from_slice(&line[start..curr]);
        result.push(0x0A);
        result.extend_from_slice(&line[curr + 1..]);
    } else {
        result.extend_from_slice(&line[start..]);
    }
    result[1..].to_vec()
}

fn escape_string(s: &[u16]) -> String {
    let mut out = String::new();
    let mut i = 0;
    let hex = |c: u32| {
        let (h, len) = if c <= 0xFF {
            ('x', 2)
        } else if c <= 0xFFFF {
            ('u', 4)
        } else {
            ('U', 8)
        };
        format!("\\{h}{:0>len$}", format!("{c:X}"), len = len)
    };
    while i < s.len() {
        let c = s[i];
        if (0xD800..=0xDBFF).contains(&c) {
            if let Some(&n) = s.get(i + 1) {
                if (0xDC00..=0xDFFF).contains(&n) {
                    out += &hex((c as u32 - 0xD800) * 0x400 + n as u32 - 0xDC00 + 0x10000);
                    i += 2;
                    continue;
                }
            }
        }
        let esc = match c {
            0x00 => Some("\\0"),
            0x07 => Some("\\a"),
            0x08 => Some("\\b"),
            0x09 => Some("\\t"),
            0x0A => Some("\\n"),
            0x0B => Some("\\v"),
            0x0C => Some("\\f"),
            0x0D => Some("\\r"),
            0x1B => Some("\\e"),
            0x22 => Some("\\\""),
            0x5C => Some("\\\\"),
            0x85 => Some("\\N"),
            0xA0 => Some("\\_"),
            0x2028 => Some("\\L"),
            0x2029 => Some("\\P"),
            _ => None,
        };
        match esc {
            Some(e) => out += e,
            None if is_printable(c) => out += &String::from_utf16_lossy(&[c]),
            None => out += &hex(c as u32),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(src: &str) -> String {
        dump(&load(src).unwrap())
    }

    #[test]
    fn typical_frontmatter() {
        let src = "id: 6f1c2a4e-1111-4222-8333-944455556666\nname: Meeting notes\ntags:\n  - work\n  - 'true'\ncreated: '2026-01-02T03:04:05.000Z'\nupdated: 2026-01-02T03:04:05.000Z\n";
        let y = load(src).unwrap();
        assert!(matches!(y.get("updated"), Some(Yaml::Date(_))));
        assert_eq!(
            y.get("created"),
            Some(&Yaml::Str("2026-01-02T03:04:05.000Z".into()))
        );
        assert_eq!(dump(&y), src);
    }

    #[test]
    fn scalars_quote_like_js_yaml() {
        let m = |v: Yaml| dump(&Yaml::Map(vec![("k".into(), v)]));
        assert_eq!(m(Yaml::Str("123".into())), "k: '123'\n");
        assert_eq!(m(Yaml::Str("yes".into())), "k: 'yes'\n");
        assert_eq!(m(Yaml::Str("a: b".into())), "k: 'a: b'\n");
        assert_eq!(m(Yaml::Str("- x".into())), "k: '- x'\n");
        assert_eq!(m(Yaml::Str("it's".into())), "k: it's\n");
        assert_eq!(m(Yaml::Str("#tag".into())), "k: '#tag'\n");
        assert_eq!(m(Yaml::Str("a #b".into())), "k: 'a #b'\n");
        assert_eq!(m(Yaml::Str("a#b".into())), "k: a#b\n");
        assert_eq!(m(Yaml::Str("".into())), "k: ''\n");
        assert_eq!(m(Yaml::Str("x\ny\n".into())), "k: |\n  x\n  y\n");
        assert_eq!(m(Yaml::Str("tab\there".into())), "k: \"tab\\there\"\n");
        assert_eq!(m(Yaml::Str("😀 hi".into())), "k: \"\\U0001F600 hi\"\n");
        assert_eq!(m(Yaml::Seq(vec![])), "k: []\n");
        assert_eq!(m(Yaml::Map(vec![])), "k: {}\n");
        assert_eq!(m(Yaml::Float(1.5)), "k: 1.5\n");
        assert_eq!(m(Yaml::Null), "k: null\n");
    }

    #[test]
    fn nested_structures() {
        let src = "presentation:\n  theme: dark\n  size:\n    w: 16\n    h: 9\nlist:\n  - a: 1\n    b: 2\n  - - x\n    - 'y'\n";
        assert_eq!(rt(src), src);
    }

    #[test]
    fn long_strings_fold() {
        let long = "word ".repeat(30).trim_end().to_string();
        let out = dump(&Yaml::Map(vec![("k".into(), Yaml::Str(long.clone()))]));
        assert!(out.starts_with("k: >-\n  word"));
        assert_eq!(load(&out).unwrap().get("k"), Some(&Yaml::Str(long)));
    }

    #[test]
    fn yaml11_numbers_and_errors() {
        assert_eq!(load("a: 0x1F").unwrap().get("a"), Some(&Yaml::Int(31)));
        assert_eq!(load("a: 010").unwrap().get("a"), Some(&Yaml::Int(8)));
        assert_eq!(load("a: 1:30").unwrap().get("a"), Some(&Yaml::Int(90)));
        assert_eq!(load("a: 1_000").unwrap().get("a"), Some(&Yaml::Int(1000)));
        assert_eq!(load("a: .5").unwrap().get("a"), Some(&Yaml::Float(0.5)));
        assert_eq!(load("a: ~").unwrap().get("a"), Some(&Yaml::Null));
        assert!(load("a: 1\na: 2").is_err());
        assert_eq!(
            load("base: &b {x: 1}\nd:\n  <<: *b\n  y: 2")
                .unwrap()
                .get("d")
                .unwrap()
                .get("x"),
            Some(&Yaml::Int(1))
        );
    }
}

/// Differential check against js-yaml 3. Not run by default:
///   YAML_DIFF_CASES=<cases.json> cargo test -p noteliner-core yaml_diff -- --ignored --nocapture
/// where each case is { data, dumped, back } produced by js-yaml's
/// safeDump/safeLoad.
#[cfg(test)]
#[test]
#[ignore]
fn yaml_diff() {
    let path = std::env::var("YAML_DIFF_CASES").expect("YAML_DIFF_CASES");
    let cases: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let (mut dump_bad, mut load_bad) = (0, 0);
    for c in &cases {
        let ours = dump(&Yaml::from_json(&c["data"]));
        let theirs = c["dumped"].as_str().unwrap();
        if ours != theirs {
            dump_bad += 1;
            if dump_bad <= 5 {
                eprintln!("DUMP MISMATCH\n--- js-yaml\n{theirs}--- ours\n{ours}");
            }
        }
        match load(theirs) {
            Ok(y) if y.to_json() == c["back"] => {}
            other => {
                load_bad += 1;
                if load_bad <= 5 {
                    eprintln!(
                        "LOAD MISMATCH\n{theirs}\n js: {}\n us: {:?}",
                        c["back"],
                        other.map(|y| y.to_json())
                    );
                }
            }
        }
    }
    eprintln!(
        "{} cases: {dump_bad} dump mismatches, {load_bad} load mismatches",
        cases.len()
    );
    assert_eq!((dump_bad, load_bad), (0, 0));
}
