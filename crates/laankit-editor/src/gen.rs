//! A deterministic Markdown document generator: frontmatter, headings,
//! paragraphs with emphasis, links, `[[wikilinks]]` and inline code, nested
//! bullet, task and numbered lists, code fences, tables and blockquotes.
//! The same `lines` always gives the same document, so measurements repeat.

const WORDS: &[&str] = &[
    "outline",
    "note",
    "sync",
    "branch",
    "vault",
    "index",
    "render",
    "paragraph",
    "anchor",
    "commit",
    "draft",
    "review",
    "layout",
    "glyph",
    "cursor",
    "marker",
    "heading",
    "theme",
    "search",
    "backlink",
    "template",
    "preview",
    "export",
    "slide",
    "import",
    "editor",
];

/// A Markdown document of exactly `lines` lines (at least 1).
pub fn document(lines: usize) -> String {
    let mut g = Gen {
        seed: 0x2545_f491_4f6c_dd1d,
        out: Vec::with_capacity(lines + 32),
    };
    g.out.extend(
        [
            "---",
            "title: Generated note",
            "tags: [spike, editor]",
            "---",
            "",
        ]
        .map(String::from),
    );
    let mut block = 0usize;
    while g.out.len() < lines {
        match block % 7 {
            0 => g.heading(block),
            1 => g.paragraph(),
            2 => g.bullets(),
            3 => g.numbered(),
            4 => g.fence(),
            5 => g.table(),
            _ => g.quote(),
        }
        g.out.push(String::new());
        block += 1;
    }
    g.out.truncate(lines.max(1));
    g.out.join("\n")
}

/// [`document`] with `fences` large code fences (`fence_lines` lines each,
/// cycling through Rust, Python, JSON and shell) spread evenly through it;
/// still exactly `lines` lines.
pub fn document_with_fences(lines: usize, fences: usize, fence_lines: usize) -> String {
    let base = document(lines);
    let mut out: Vec<String> = base.split('\n').map(String::from).collect();
    let per = fence_lines + 2;
    if fences == 0 || fences * per >= lines {
        return base;
    }
    let gap = lines / (fences + 1);
    for f in 0..fences {
        let at = (gap * (f + 1)).min(out.len());
        let fence = big_fence(f, fence_lines);
        out.splice(at..at, fence);
    }
    out.truncate(lines.max(1));
    out.join("\n")
}

fn big_fence(index: usize, n: usize) -> Vec<String> {
    let (lang, line): (&str, fn(usize) -> String) = match index % 4 {
        0 => ("rust", |i| {
            format!("    let value_{i}: u64 = compute({i}, \"name {i}\") * 2; // step {i}")
        }),
        1 => ("python", |i| {
            format!("    result_{i} = transform(data[{i}], key='k{i}')  # step {i}")
        }),
        2 => ("json", |i| {
            format!("  \"key_{i}\": {{ \"n\": {i}, \"ok\": true, \"s\": \"v{i}\" }},")
        }),
        _ => ("sh", |i| {
            format!("echo \"step {i}\" | grep -v x > out_{i}.log  # {i}")
        }),
    };
    let mut v = Vec::with_capacity(n + 2);
    v.push(format!("```{lang}"));
    v.extend((0..n).map(line));
    v.push("```".into());
    v
}

struct Gen {
    seed: u64,
    out: Vec<String>,
}

impl Gen {
    fn heading(&mut self, block: usize) {
        let level = 1 + (block / 7) % 3;
        let title = self.words(3);
        self.out
            .push(format!("{} {}", "#".repeat(level), capitalize(&title)));
    }

    fn paragraph(&mut self) {
        for _ in 0..3 {
            let line = format!(
                "{} **{}** and *{}* see [{}](https://example.com/{}) or [[{}]] with `{}` {}.",
                capitalize(&self.words(6)),
                self.words(2),
                self.words(1),
                self.words(2),
                self.word(),
                capitalize(&self.words(2)),
                self.word(),
                self.words(8),
            );
            self.out.push(line);
        }
    }

    fn bullets(&mut self) {
        for i in 0..6 {
            let indent = "  ".repeat([0, 0, 1, 2, 1, 0][i]);
            let task = match i {
                3 => "[ ] ",
                4 => "[x] ",
                _ => "",
            };
            let text = self.words(5);
            self.out.push(format!("{indent}- {task}{text}"));
        }
    }

    fn numbered(&mut self) {
        for n in 1..=4 {
            let text = self.words(4);
            self.out.push(format!("{n}. {text}"));
        }
        let text = self.words(3);
        self.out.push(format!("   - {text}"));
    }

    fn fence(&mut self) {
        self.out.push("```rust".into());
        for i in 0..5 {
            let name = self.word();
            self.out.push(format!(
                "fn {name}_{i}(x: usize) -> usize {{ x * {i} }} // # not a heading"
            ));
        }
        self.out.push("```".into());
    }

    fn table(&mut self) {
        self.out.push("| Name | Count | Notes |".into());
        self.out.push("|---|---|---|".into());
        for i in 0..3 {
            let (a, b) = (self.word(), self.words(3));
            self.out.push(format!("| {a} | {i} | {b} |"));
        }
    }

    fn quote(&mut self) {
        for _ in 0..2 {
            let text = self.words(7);
            self.out.push(format!("> {text}"));
        }
    }

    fn words(&mut self, n: usize) -> String {
        (0..n).map(|_| self.word()).collect::<Vec<_>>().join(" ")
    }

    fn word(&mut self) -> &'static str {
        // xorshift64*
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        let r = self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d);
        WORDS[(r >> 33) as usize % WORDS.len()]
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_documents_keep_their_length() {
        let doc = document_with_fences(5000, 4, 200);
        assert_eq!(doc.split('\n').count(), 5000);
        assert_eq!(doc.matches("```python").count(), 1);
        assert!(doc.contains("```json") && doc.contains("```sh"));
    }

    #[test]
    fn exact_line_count_and_deterministic() {
        for n in [1, 10, 1000, 5000] {
            let doc = document(n);
            assert_eq!(doc.split('\n').count(), n);
            assert_eq!(doc, document(n));
        }
    }
}
