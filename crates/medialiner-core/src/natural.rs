//! Natural ordering for names: runs of digits compare as numbers, the rest
//! without case, so "Day 2" sorts before "Day 10" and "b" after "A".

use std::cmp::Ordering;

/// Compares `a` and `b` naturally, falling back to the exact text so the
/// order is total.
pub fn compare(a: &str, b: &str) -> Ordering {
    let mut x = a.chars().peekable();
    let mut y = b.chars().peekable();
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let m = digits(&mut x);
                let n = digits(&mut y);
                let (mt, nt) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = mt
                    .len()
                    .cmp(&nt.len())
                    .then_with(|| mt.cmp(nt))
                    .then_with(|| m.len().cmp(&n.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                let ord = c.to_lowercase().cmp(d.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                x.next();
                y.next();
            }
        }
    }
}

fn digits(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
        s.push(c);
        it.next();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_in_order_and_case_ignored() {
        let mut v = vec!["Day 10", "day 2", "Day 1", "IMG_0010", "img_009", "b", "A"];
        v.sort_by(|a, b| compare(a, b));
        assert_eq!(v, ["A", "b", "Day 1", "day 2", "Day 10", "img_009", "IMG_0010"]);
        assert_eq!(compare("a", "a"), Ordering::Equal);
        assert_ne!(compare("a", "A"), Ordering::Equal);
    }
}
