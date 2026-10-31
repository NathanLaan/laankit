//! Syntax highlighting inside fenced code blocks (CodeMirror's
//! `markdown({ codeLanguages: languages })` in `Editor.svelte`).
//!
//! Engine-independent: it works on lines of text and returns token classes,
//! so the `TextEdit` layouter ([`crate::highlight`]) uses it today and a
//! custom editor core can later. It uses [`syntect`] with its bundled
//! syntaxes only (about 75 languages; plan decision 2), in pure Rust
//! (`fancy-regex`), so it builds the same on Linux, Windows and macOS.
//!
//! - **Lazy loading.** The syntax set takes a moment to deserialize, so
//!   [`syntaxes`] starts loading it on a background thread the first time
//!   it's asked for and returns `None` until it's ready. Fences stay one
//!   colour meanwhile, and the contexts that asked are repainted when it
//!   arrives. [`load_blocking`] is for tests and benchmarks.
//! - **Resumable state.** A fence is highlighted line by line from a
//!   [`FenceCode`] (syntect's parse state plus the scope stack), which the
//!   layouter keeps per line. An edit inside a fence re-highlights from the
//!   edited line until the state matches the old one again (usually at once),
//!   and an edit outside a fence doesn't touch it.
//! - **Token classes.** Scopes are reduced to a [`Token`] class by the most
//!   specific scope that matches a rule (comment, string, keyword, …); the
//!   editor maps classes to colours ([`crate::EditorColors`]).

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};

/// What a run of code is, for colouring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    /// Identifiers and anything no rule matches.
    Plain,
    Comment,
    String,
    Number,
    /// Language constants: `true`, `null`, character escapes, `self`.
    Constant,
    Keyword,
    Operator,
    Function,
    Type,
    /// Markup tags (`<div>`).
    Tag,
    /// Markup attributes and object keys.
    Attribute,
    Punctuation,
}

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static LOADING: AtomicBool = AtomicBool::new(false);
/// Contexts to repaint once the syntaxes arrive.
static WAITING: Mutex<Vec<egui::Context>> = Mutex::new(Vec::new());

/// The bundled syntaxes, or `None` while they load. The first call starts
/// the load on a background thread; `ctx` is repainted when it finishes.
pub fn syntaxes(ctx: Option<&egui::Context>) -> Option<&'static SyntaxSet> {
    if let Some(set) = SYNTAXES.get() {
        return Some(set);
    }
    if let Some(ctx) = ctx {
        if let Ok(mut waiting) = WAITING.lock() {
            if !waiting.iter().any(|c| c == ctx) {
                waiting.push(ctx.clone());
            }
        }
    }
    if !LOADING.swap(true, Ordering::SeqCst) {
        let spawned = std::thread::Builder::new()
            .name("laankit-editor-syntaxes".into())
            .spawn(|| {
                let _ = load_blocking();
                if let Ok(mut waiting) = WAITING.lock() {
                    for ctx in waiting.drain(..) {
                        ctx.request_repaint();
                    }
                }
            });
        if spawned.is_err() {
            // No threads (unlikely): load here instead.
            return Some(load_blocking());
        }
    }
    SYNTAXES.get()
}

/// The bundled syntaxes, loading them on this thread if they aren't yet.
pub fn load_blocking() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The syntax for a fence's info string (```` ```rust ```` → Rust): by file
/// extension or name, ignoring case, after a few common aliases. `None` for
/// an empty or unknown language.
pub fn find_syntax(set: &SyntaxSet, info: &str) -> Option<usize> {
    let word = info
        .trim()
        .trim_start_matches('{')
        .trim_start_matches('.')
        .split(|c: char| c.is_whitespace() || c == ',' || c == '{' || c == '}')
        .next()?
        .to_ascii_lowercase();
    if word.is_empty() {
        return None;
    }
    let token = match word.as_str() {
        "shell" | "console" | "zsh" | "shell-session" | "sh-session" => "sh",
        "c++" => "cpp",
        "c#" | "csharp" => "cs",
        "golang" => "go",
        "py3" | "python3" => "py",
        "jsonc" | "json5" => "json",
        "yml" => "yaml",
        "rs" => "rust",
        "md" => "markdown",
        "objective-c" | "objc" => "m",
        other => other,
    };
    let syntax = set.find_syntax_by_token(token)?;
    set.syntaxes().iter().position(|s| std::ptr::eq(s, syntax))
}

/// A highlighted line: token runs tiling it, in order.
pub type Runs = Vec<(Range<usize>, Token)>;

/// Where a fence's highlighting stands at the start of a line: which syntax,
/// syntect's parse state and the open scopes. Compared to decide whether the
/// lines after an edit can be reused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FenceCode {
    syntax: usize,
    parse: ParseState,
    stack: ScopeStack,
}

impl FenceCode {
    /// The state at the first line of a fence in syntax `syntax`.
    pub fn new(set: &SyntaxSet, syntax: usize) -> Option<Self> {
        let reference = set.syntaxes().get(syntax)?;
        Some(Self {
            syntax,
            parse: ParseState::new(reference),
            stack: ScopeStack::new(),
        })
    }

    /// The syntax's name (for tests and diagnostics).
    pub fn syntax_name<'a>(&self, set: &'a SyntaxSet) -> &'a str {
        set.syntaxes()
            .get(self.syntax)
            .map_or("", |s| s.name.as_str())
    }

    /// Highlights one line (without its newline): its token runs (tiling the
    /// line, in order) and the state for the next line. `None` if syntect
    /// fails on the line; the caller then shows the rest of the fence plain.
    pub fn line(&self, set: &SyntaxSet, line: &str) -> Option<(Runs, Self)> {
        let mut next = self.clone();
        // The "newlines" syntaxes expect each line to end with '\n'.
        let mut owned = String::with_capacity(line.len() + 1);
        owned.push_str(line);
        owned.push('\n');
        let ops = next.parse.parse_line(&owned, set).ok()?;
        let mut runs: Runs = Vec::new();
        let mut at = 0;
        let push = |runs: &mut Runs, range: Range<usize>, t: Token| {
            if range.is_empty() {
                return;
            }
            if let Some(last) = runs.last_mut() {
                if last.1 == t && last.0.end == range.start {
                    last.0.end = range.end;
                    return;
                }
            }
            runs.push((range, t));
        };
        for (pos, op) in &ops {
            let pos = (*pos).min(line.len());
            if pos > at {
                push(&mut runs, at..pos, classify(&next.stack));
                at = pos;
            }
            next.stack.apply(op).ok()?;
        }
        if at < line.len() {
            push(&mut runs, at..line.len(), classify(&next.stack));
        }
        Some((runs, next))
    }
}

/// Scope prefixes and their token, most specific first. The innermost scope
/// that matches any rule decides.
fn rules() -> &'static [(Scope, Token)] {
    static RULES: OnceLock<Vec<(Scope, Token)>> = OnceLock::new();
    RULES.get_or_init(|| {
        [
            ("comment", Token::Comment),
            ("punctuation.definition.comment", Token::Comment),
            ("constant.character.escape", Token::Constant),
            ("string", Token::String),
            ("punctuation.definition.string", Token::String),
            ("constant.numeric", Token::Number),
            ("constant.language", Token::Constant),
            ("constant.character", Token::Constant),
            ("constant.other", Token::Constant),
            ("support.constant", Token::Constant),
            ("variable.language", Token::Constant),
            ("keyword.operator", Token::Operator),
            ("keyword", Token::Keyword),
            ("storage", Token::Keyword),
            ("entity.name.function", Token::Function),
            ("support.function", Token::Function),
            ("variable.function", Token::Function),
            ("entity.name.type", Token::Type),
            ("entity.name.class", Token::Type),
            ("entity.name.struct", Token::Type),
            ("entity.name.enum", Token::Type),
            ("entity.name.trait", Token::Type),
            ("entity.name.impl", Token::Type),
            ("entity.other.inherited-class", Token::Type),
            ("support.type", Token::Type),
            ("support.class", Token::Type),
            ("entity.name.tag", Token::Tag),
            ("entity.other.attribute-name", Token::Attribute),
            ("support.type.property-name", Token::Attribute),
            ("punctuation", Token::Punctuation),
        ]
        .into_iter()
        .filter_map(|(s, t)| Scope::new(s).ok().map(|s| (s, t)))
        .collect()
    })
}

/// The token class of text under `stack`.
pub fn classify(stack: &ScopeStack) -> Token {
    static KEYS: OnceLock<Vec<Scope>> = OnceLock::new();
    let keys = KEYS.get_or_init(|| {
        ["meta.mapping.key", "meta.structure.dictionary.key"]
            .into_iter()
            .filter_map(|s| Scope::new(s).ok())
            .collect()
    });
    let scopes = stack.as_slice();
    // An object key (JSON's dictionary keys) is a property name even though
    // its innermost scope is a string, as CodeMirror colours it.
    if scopes
        .iter()
        .any(|s| keys.iter().any(|k| k.is_prefix_of(*s)))
    {
        return Token::Attribute;
    }
    for scope in scopes.iter().rev() {
        for (prefix, token) in rules() {
            if prefix.is_prefix_of(*scope) {
                return *token;
            }
        }
    }
    Token::Plain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(lang: &str, lines: &[&str]) -> Vec<Vec<(String, Token)>> {
        let set = load_blocking();
        let syntax = find_syntax(set, lang).unwrap_or_else(|| panic!("no syntax for {lang}"));
        let mut code = FenceCode::new(set, syntax).unwrap();
        let mut out = Vec::new();
        for line in lines {
            let (runs, next) = code.line(set, line).expect("parses");
            out.push(
                runs.into_iter()
                    .map(|(r, t)| (line[r].to_string(), t))
                    .collect(),
            );
            code = next;
        }
        out
    }

    fn has(line: &[(String, Token)], text: &str, token: Token) -> bool {
        line.iter().any(|(s, t)| s.trim() == text && *t == token)
    }

    #[test]
    fn finds_syntaxes_by_name_extension_and_alias() {
        let set = load_blocking();
        let name = |info: &str| find_syntax(set, info).map(|i| set.syntaxes()[i].name.clone());
        assert_eq!(name("rust").as_deref(), Some("Rust"));
        assert_eq!(name("RS").as_deref(), Some("Rust"));
        assert_eq!(name("python").as_deref(), Some("Python"));
        assert_eq!(name("py {.numberLines}").as_deref(), Some("Python"));
        assert_eq!(name("json").as_deref(), Some("JSON"));
        assert!(
            name("shell").is_some_and(|n| n.to_lowercase().contains("bash")),
            "{:?}",
            name("shell")
        );
        assert!(name("bash").is_some());
        assert_eq!(name("").as_deref(), None);
        assert_eq!(name("klingon").as_deref(), None);
        // The bundled set: about 75 languages.
        assert!(set.syntaxes().len() > 50, "{}", set.syntaxes().len());
    }

    #[test]
    fn rust_python_json_and_shell_tokens() {
        let rust = tokens(
            "rust",
            &[
                "fn main() {",
                "    let x = 42; // answer",
                "    println!(\"hi\");",
                "}",
            ],
        );
        assert!(has(&rust[0], "fn", Token::Keyword), "{:?}", rust[0]);
        assert!(has(&rust[0], "main", Token::Function), "{:?}", rust[0]);
        assert!(has(&rust[1], "42", Token::Number), "{:?}", rust[1]);
        assert!(has(&rust[1], "// answer", Token::Comment), "{:?}", rust[1]);
        assert!(
            rust[2]
                .iter()
                .any(|(s, t)| s.contains("hi") && *t == Token::String),
            "{:?}",
            rust[2]
        );

        let py = tokens("python", &["def f(a):", "    return None  # done"]);
        assert!(has(&py[0], "def", Token::Keyword), "{:?}", py[0]);
        assert!(has(&py[1], "None", Token::Constant), "{:?}", py[1]);
        assert!(has(&py[1], "# done", Token::Comment), "{:?}", py[1]);

        let json = tokens("json", &["{\"key\": true, \"n\": 1.5}"]);
        assert!(
            json[0]
                .iter()
                .any(|(s, t)| s.contains("key") && *t == Token::Attribute),
            "{:?}",
            json[0]
        );
        assert!(has(&json[0], "true", Token::Constant), "{:?}", json[0]);
        assert!(has(&json[0], "1.5", Token::Number), "{:?}", json[0]);

        let sh = tokens("sh", &["echo \"$HOME\" | grep x # find"]);
        assert!(
            sh[0].iter().any(|(_, t)| *t == Token::Comment),
            "{:?}",
            sh[0]
        );
        assert!(
            sh[0].iter().any(|(_, t)| *t == Token::String),
            "{:?}",
            sh[0]
        );
    }

    #[test]
    fn state_carries_across_lines() {
        // A block comment opened on one line colours the next.
        let rust = tokens("rust", &["/* start", "still comment */ let y = 1;"]);
        assert!(rust[1][0].1 == Token::Comment, "{:?}", rust[1]);
        assert!(has(&rust[1], "let", Token::Keyword), "{:?}", rust[1]);
    }

    #[test]
    fn runs_tile_the_line() {
        let set = load_blocking();
        let code = FenceCode::new(set, find_syntax(set, "rust").unwrap()).unwrap();
        for line in ["", "é → \"ü\" // ß", "fn x() -> u8 { 1 }"] {
            let (runs, _) = code.line(set, line).unwrap();
            let mut at = 0;
            for (r, _) in &runs {
                assert_eq!(r.start, at, "{line:?}: {runs:?}");
                at = r.end;
            }
            assert_eq!(at, line.len(), "{line:?}: {runs:?}");
        }
    }
}
