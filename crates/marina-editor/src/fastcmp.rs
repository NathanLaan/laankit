//! Common prefix and suffix lengths of two byte strings, compared a block
//! at a time (so the compiler can use wide loads) before finishing byte by
//! byte. Every edit diffs the note against its previous text in a few
//! places (history, highlighter, rope); at 50,000 lines a byte-at-a-time
//! iterator made each of those cost milliseconds.

const BLOCK: usize = 64;

/// Bytes `a` and `b` share at the start.
pub fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + BLOCK <= n && a[i..i + BLOCK] == b[i..i + BLOCK] {
        i += BLOCK;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// Bytes `a` and `b` share at the end, at most `max`.
pub fn common_suffix(a: &[u8], b: &[u8], max: usize) -> usize {
    let n = a.len().min(b.len()).min(max);
    let (la, lb) = (a.len(), b.len());
    let mut i = 0;
    while i + BLOCK <= n && a[la - i - BLOCK..la - i] == b[lb - i - BLOCK..lb - i] {
        i += BLOCK;
    }
    while i < n && a[la - i - 1] == b[lb - i - 1] {
        i += 1;
    }
    i
}

/// Newlines in `b` (a filter-count the compiler vectorizes).
pub fn newlines(b: &[u8]) -> usize {
    b.iter().filter(|&&c| c == b'\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_and_suffix() {
        let a = "x".repeat(200) + "abc" + &"y".repeat(150);
        let b = "x".repeat(200) + "aXc" + &"y".repeat(150);
        assert_eq!(common_prefix(a.as_bytes(), b.as_bytes()), 201);
        assert_eq!(common_suffix(a.as_bytes(), b.as_bytes(), usize::MAX), 151);
        assert_eq!(common_suffix(a.as_bytes(), b.as_bytes(), 100), 100);
        assert_eq!(common_prefix(b"ab", b"abc"), 2);
        assert_eq!(common_suffix(b"", b"abc", 9), 0);
        assert_eq!(newlines(b"a\nb\n"), 2);
    }
}
