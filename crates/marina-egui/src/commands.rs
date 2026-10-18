//! The command registry, natively: `command-palette/registry.svelte.js` and
//! `lib/fuzzy.js`.
//!
//! Every user-invokable action (toolbar button, keyboard shortcut, palette
//! entry) is a [`Command`]. Each command owns its shortcut display, its key
//! bindings, its applicability (`when`) and what it does.
//!
//! What it does is an **action value** of the app's own type `A` (usually an
//! enum), not a closure as in the Svelte registry. [`Registry::run`] and
//! [`Registry::dispatch_keys`] hand the action back, and the app applies it
//! in its update loop with full `&mut self` access. A stored `FnMut` would
//! have to capture the app state it changes, which Rust won't allow while
//! the app also owns the registry. Action values also make dispatch easy to
//! test: the tests just compare values.
//!
//! `when` is a plain `fn(&C) -> bool` over an app-defined context `C` (a
//! view of the app state, often the app itself), evaluated when the palette
//! lists commands or a key is dispatched, like the Svelte `when()` closures.

use egui::{Key, KeyboardShortcut, ModifierNames, Modifiers};
use serde::{Deserialize, Serialize};

/// Recently used command ids kept for the palette's ordering.
const RECENT_CAP: usize = 8;

/// A key binding. The Svelte predicates compare either `e.key` (the logical
/// key, after the keyboard layout) or `e.code` (the physical key position);
/// [`KeyMatch`] picks which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub modifiers: Modifiers,
    pub key: Key,
    pub matching: KeyMatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyMatch {
    /// The logical key (`e.key === 'n'`), with exactly these modifiers.
    Logical,
    /// The physical key (`e.code === 'KeyD'`), with exactly these modifiers.
    /// Use it for Shift and Alt shortcuts, whose logical key changes with
    /// the layout (Ctrl+Shift+7 is `/` on a German keyboard). Falls back to
    /// the logical key where the platform reports no physical key.
    Physical,
    /// The logical key with *at least* these modifiers, for keys that need
    /// Shift to type on some layouts (`e.key === '+'`).
    LogicalLoose,
}

impl Binding {
    /// `modifiers` + the logical `key`.
    pub const fn logical(modifiers: Modifiers, key: Key) -> Self {
        Self {
            modifiers,
            key,
            matching: KeyMatch::Logical,
        }
    }

    /// `modifiers` + the physical `key`.
    pub const fn physical(modifiers: Modifiers, key: Key) -> Self {
        Self {
            modifiers,
            key,
            matching: KeyMatch::Physical,
        }
    }

    /// At least `modifiers` + the logical `key`.
    pub const fn loose(modifiers: Modifiers, key: Key) -> Self {
        Self {
            modifiers,
            key,
            matching: KeyMatch::LogicalLoose,
        }
    }

    /// Whether a key press with these properties triggers the binding.
    pub fn matches(&self, key: Key, physical_key: Option<Key>, modifiers: Modifiers) -> bool {
        match self.matching {
            KeyMatch::Logical => key == self.key && modifiers.matches_exact(self.modifiers),
            KeyMatch::Physical => {
                physical_key.unwrap_or(key) == self.key && modifiers.matches_exact(self.modifiers)
            }
            KeyMatch::LogicalLoose => {
                key == self.key && modifiers.matches_logically(self.modifiers)
            }
        }
    }

    /// `Ctrl+Shift+D` (or `⌃⇧D` style names on macOS when `is_mac`).
    pub fn label(&self, is_mac: bool) -> String {
        let names = if is_mac {
            ModifierNames::SYMBOLS
        } else {
            ModifierNames::NAMES
        };
        KeyboardShortcut::new(self.modifiers, self.key).format(&names, is_mac)
    }
}

/// A user-invokable action. `A` is the app's action type, `C` the context
/// that `when` reads.
#[derive(Clone)]
pub struct Command<A, C = ()> {
    /// Dotted and stable, e.g. `file.new`. Used for recents.
    pub id: String,
    /// Shown in the palette and the shortcuts list.
    pub label: String,
    /// Palette grouping: `File`, `View`, `Tags`, …
    pub section: String,
    /// The shortcut as displayed, e.g. `Ctrl+Shift+,`. When `None`, the first
    /// binding is formatted instead (see [`Command::shortcut_label`]).
    pub shortcut: Option<String>,
    /// Keys that run the command. The first matching command in
    /// registration order wins.
    pub bindings: Vec<Binding>,
    /// If it returns `false`, the command is hidden from the palette and its
    /// keys are ignored.
    pub when: Option<fn(&C) -> bool>,
    pub action: A,
}

impl<A, C> Command<A, C> {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        section: impl Into<String>,
        action: A,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            section: section.into(),
            shortcut: None,
            bindings: Vec::new(),
            when: None,
            action,
        }
    }

    /// Adds a key binding.
    pub fn key(mut self, binding: Binding) -> Self {
        self.bindings.push(binding);
        self
    }

    /// Sets the displayed shortcut text.
    pub fn shortcut(mut self, text: impl Into<String>) -> Self {
        self.shortcut = Some(text.into());
        self
    }

    pub fn when(mut self, when: fn(&C) -> bool) -> Self {
        self.when = Some(when);
        self
    }

    /// The displayed shortcut: the explicit text, else the first binding.
    pub fn shortcut_label(&self) -> Option<String> {
        self.shortcut.clone().or_else(|| {
            self.bindings
                .first()
                .map(|b| b.label(cfg!(target_os = "macos")))
        })
    }
}

/// One row of [`Registry::shortcut_list`] (the `ShortcutsList` input).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutEntry {
    pub keys: String,
    pub action: String,
    pub section: String,
}

/// The app's commands, plus the recently used ones (which the app persists
/// with [`Registry::recents`] / [`Registry::load_recents`]).
pub struct Registry<A, C = ()> {
    commands: Vec<Command<A, C>>,
    recent_ids: Vec<String>,
}

impl<A, C> Default for Registry<A, C> {
    fn default() -> Self {
        Self {
            commands: Vec::new(),
            recent_ids: Vec::new(),
        }
    }
}

/// The persisted part of a [`Registry`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recents {
    pub ids: Vec<String>,
}

impl<A: Clone, C> Registry<A, C> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a command, replacing one with the same id in place.
    pub fn register(&mut self, cmd: Command<A, C>) {
        match self.commands.iter_mut().find(|c| c.id == cmd.id) {
            Some(existing) => *existing = cmd,
            None => self.commands.push(cmd),
        }
    }

    pub fn unregister(&mut self, id: &str) {
        self.commands.retain(|c| c.id != id);
    }

    pub fn get(&self, id: &str) -> Option<&Command<A, C>> {
        self.commands.iter().find(|c| c.id == id)
    }

    /// Every command, in registration order.
    pub fn commands(&self) -> &[Command<A, C>] {
        &self.commands
    }

    pub fn is_applicable(cmd: &Command<A, C>, ctx: &C) -> bool {
        cmd.when.is_none_or(|when| when(ctx))
    }

    /// The command's action if it exists and applies; records the use.
    pub fn run(&mut self, id: &str, ctx: &C) -> Option<A> {
        let cmd = self.get(id)?;
        if !Self::is_applicable(cmd, ctx) {
            return None;
        }
        let action = cmd.action.clone();
        self.record_use(id);
        Some(action)
    }

    /// Consumes this frame's key presses that trigger a command and returns
    /// their actions, in order. For each press the first applicable command
    /// (in registration order) with a matching binding wins, as in
    /// `dispatchKeyEvent`. Call it before drawing widgets that also read
    /// keys (text fields), so a command's keys don't reach them.
    pub fn dispatch_keys(&mut self, egui_ctx: &egui::Context, ctx: &C) -> Vec<A> {
        let mut hits: Vec<usize> = Vec::new();
        egui_ctx.input_mut(|input| {
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    return true;
                };
                let hit = self.commands.iter().position(|cmd| {
                    cmd.bindings
                        .iter()
                        .any(|b| b.matches(*key, *physical_key, *modifiers))
                        && Self::is_applicable(cmd, ctx)
                });
                match hit {
                    Some(i) => {
                        hits.push(i);
                        false
                    }
                    None => true,
                }
            });
        });
        hits.into_iter()
            .map(|i| {
                let id = self.commands[i].id.clone();
                self.record_use(&id);
                self.commands[i].action.clone()
            })
            .collect()
    }

    pub fn record_use(&mut self, id: &str) {
        self.recent_ids.retain(|x| x != id);
        self.recent_ids.insert(0, id.to_string());
        self.recent_ids.truncate(RECENT_CAP);
    }

    /// The recently used ids, most recent first, for persisting.
    pub fn recents(&self) -> Recents {
        Recents {
            ids: self.recent_ids.clone(),
        }
    }

    pub fn load_recents(&mut self, recents: Recents) {
        self.recent_ids = recents.ids;
        self.recent_ids.truncate(RECENT_CAP);
    }

    /// For the palette: applicable commands, recently used first, then
    /// alphabetical by label.
    pub fn applicable_commands(&self, ctx: &C) -> Vec<&Command<A, C>> {
        let rank = |id: &str| {
            self.recent_ids
                .iter()
                .position(|r| r == id)
                .unwrap_or(usize::MAX)
        };
        let mut out: Vec<&Command<A, C>> = self
            .commands
            .iter()
            .filter(|c| Self::is_applicable(c, ctx))
            .collect();
        out.sort_by(|a, b| {
            rank(&a.id)
                .cmp(&rank(&b.id))
                .then_with(|| compare_labels(&a.label, &b.label))
        });
        out
    }

    /// For the shortcuts list: every command with a shortcut, sorted by
    /// section and then label.
    pub fn shortcut_list(&self) -> Vec<ShortcutEntry> {
        let mut out: Vec<ShortcutEntry> = self
            .commands
            .iter()
            .filter_map(|c| {
                Some(ShortcutEntry {
                    keys: c.shortcut_label()?,
                    action: c.label.clone(),
                    section: c.section.clone(),
                })
            })
            .collect();
        out.sort_by(|a, b| {
            compare_labels(&a.section, &b.section)
                .then_with(|| compare_labels(&a.action, &b.action))
        });
        out
    }
}

/// Approximates `localeCompare`: case-insensitive first, then by case.
fn compare_labels(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| b.cmp(a))
}

/// `fuzzyScore` from `lib/fuzzy.js`: 0 for no match, otherwise higher is
/// better. A prefix match scores ~1000, a substring match `500 - position`,
/// and a subsequence match gets word-boundary boosts. An empty pattern
/// scores 1, so everything is listed.
///
/// Works on UTF-16 code units of the lowercased strings, like the
/// JavaScript, so the scores are identical for any input.
pub fn fuzzy_score(target: &str, pattern: &str) -> i32 {
    if pattern.is_empty() {
        return 1;
    }
    if target.is_empty() {
        return 0;
    }
    let t: Vec<u16> = target.to_lowercase().encode_utf16().collect();
    let p: Vec<u16> = pattern.to_lowercase().encode_utf16().collect();
    let target_len = target.encode_utf16().count() as i32;
    let pattern_len = pattern.encode_utf16().count() as i32;

    if t.starts_with(&p) {
        return 1000
            + if target_len - pattern_len == 0 {
                100
            } else {
                0
            };
    }
    if let Some(idx) = t.windows(p.len()).position(|w| w == p.as_slice()) {
        return 500 - idx as i32;
    }

    // Subsequence match.
    let mut score = 0;
    let mut pi = 0;
    let mut matched = 0;
    for i in 0..t.len() {
        if pi >= p.len() {
            break;
        }
        if t[i] != p[pi] {
            continue;
        }
        matched += 1;
        pi += 1;
        let prev = if i == 0 { b' ' as u16 } else { t[i - 1] };
        let boundary = b" -_./".iter().any(|&c| prev == c as u16);
        score += if boundary { 12 } else { 2 };
    }
    if pi < p.len() {
        return 0;
    }
    // Penalize long targets where the match is sparse.
    (score - (t.len() as i32 - matched) / 8).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Act {
        New,
        Delete,
        Dup,
        Palette,
    }

    struct Ctx {
        has_selection: bool,
    }

    fn registry() -> Registry<Act, Ctx> {
        let mut r = Registry::new();
        r.register(
            Command::new("file.new", "New File", "File", Act::New)
                .key(Binding::logical(Modifiers::COMMAND, Key::N))
                .shortcut("Ctrl+N"),
        );
        r.register(
            Command::new("file.delete", "Delete File", "File", Act::Delete)
                .key(Binding::logical(Modifiers::COMMAND, Key::D))
                .when(|c: &Ctx| c.has_selection),
        );
        r.register(
            Command::new("file.duplicate", "Duplicate File", "File", Act::Dup)
                .key(Binding::physical(
                    Modifiers::COMMAND | Modifiers::SHIFT,
                    Key::D,
                ))
                .when(|c: &Ctx| c.has_selection),
        );
        r.register(Command::new(
            "view.palette",
            "Command Palette",
            "View",
            Act::Palette,
        ));
        r
    }

    fn key_event(key: Key, physical: Option<Key>, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: physical,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn dispatch(
        r: &mut Registry<Act, Ctx>,
        ctx: &Ctx,
        events: Vec<egui::Event>,
    ) -> (Vec<Act>, usize) {
        let egui_ctx = egui::Context::default();
        let mut out = Vec::new();
        let mut left = 0;
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut output = egui_ctx.run_ui(input, |ui| {
            out = r.dispatch_keys(ui.ctx(), ctx);
            left = ui.input(|i| i.events.len());
        });
        output.textures_delta.clear();
        (out, left)
    }

    #[test]
    fn register_replaces_in_place() {
        let mut r = registry();
        r.register(Command::new("file.new", "New Note", "File", Act::New));
        assert_eq!(r.commands()[0].label, "New Note");
        assert_eq!(r.commands().len(), 4);
        r.unregister("file.new");
        assert!(r.get("file.new").is_none());
    }

    #[test]
    fn when_gates_run_and_keys() {
        let mut r = registry();
        let no = Ctx {
            has_selection: false,
        };
        let yes = Ctx {
            has_selection: true,
        };
        assert_eq!(r.run("file.delete", &no), None);
        assert_eq!(r.run("file.delete", &yes), Some(Act::Delete));

        let ctrl_d = || key_event(Key::D, Some(Key::D), Modifiers::COMMAND);
        let (acts, left) = dispatch(&mut r, &no, vec![ctrl_d()]);
        assert!(acts.is_empty());
        assert_eq!(left, 1, "an unhandled key stays for the widgets");
        let (acts, left) = dispatch(&mut r, &yes, vec![ctrl_d()]);
        assert_eq!(acts, vec![Act::Delete]);
        assert_eq!(left, 0, "a handled key is consumed");
    }

    #[test]
    fn exact_modifiers_and_physical_keys() {
        let mut r = registry();
        let ctx = Ctx {
            has_selection: true,
        };
        // Ctrl+Shift+D must not run Ctrl+D's command.
        let (acts, _) = dispatch(
            &mut r,
            &ctx,
            vec![key_event(
                Key::D,
                Some(Key::D),
                Modifiers::COMMAND | Modifiers::SHIFT,
            )],
        );
        assert_eq!(acts, vec![Act::Dup]);
        // A layout where that key types something else still matches physically.
        let (acts, _) = dispatch(
            &mut r,
            &ctx,
            vec![key_event(
                Key::E,
                Some(Key::D),
                Modifiers::COMMAND | Modifiers::SHIFT,
            )],
        );
        assert_eq!(acts, vec![Act::Dup]);
        // Ctrl+N with Alt held is not Ctrl+N.
        let (acts, _) = dispatch(
            &mut r,
            &ctx,
            vec![key_event(Key::N, None, Modifiers::COMMAND | Modifiers::ALT)],
        );
        assert!(acts.is_empty());
    }

    #[test]
    fn loose_binding_allows_shift() {
        let b = Binding::loose(Modifiers::COMMAND, Key::Plus);
        assert!(b.matches(
            Key::Plus,
            Some(Key::Equals),
            Modifiers::COMMAND | Modifiers::SHIFT
        ));
        assert!(!b.matches(Key::Plus, None, Modifiers::NONE));
    }

    #[test]
    fn recents_order_the_palette() {
        let mut r = registry();
        let ctx = Ctx {
            has_selection: false,
        };
        let labels = |r: &Registry<Act, Ctx>| {
            r.applicable_commands(&ctx)
                .iter()
                .map(|c| c.label.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(&r), ["Command Palette", "New File"]);
        r.run("file.new", &ctx);
        assert_eq!(labels(&r), ["New File", "Command Palette"]);

        for i in 0..20 {
            r.record_use(&format!("x.{i}"));
        }
        assert_eq!(r.recents().ids.len(), RECENT_CAP);
        assert_eq!(r.recents().ids[0], "x.19");
        let mut fresh = registry();
        fresh.load_recents(Recents {
            ids: vec!["view.palette".into(), "file.new".into()],
        });
        assert_eq!(labels(&fresh), ["Command Palette", "New File"]);
    }

    #[test]
    fn shortcut_list_sorted_with_labels() {
        let r = registry();
        let list = r.shortcut_list();
        let actions: Vec<&str> = list.iter().map(|s| s.action.as_str()).collect();
        assert_eq!(actions, ["Delete File", "Duplicate File", "New File"]);
        assert_eq!(list[2].keys, "Ctrl+N");
        assert!(list[1].keys.contains('D'));
    }

    #[test]
    fn fuzzy_tiers() {
        assert_eq!(fuzzy_score("anything", ""), 1);
        assert_eq!(fuzzy_score("", "a"), 0);
        assert_eq!(fuzzy_score("New File", "new file"), 1100);
        assert_eq!(fuzzy_score("New File", "new"), 1000);
        assert_eq!(fuzzy_score("Open New File", "file"), 491);
        assert_eq!(fuzzy_score("toggle-sidebar", "ts"), 23);
        assert_eq!(fuzzy_score("New File", "xyz"), 0);
    }

    /// Scores a few hundred generated pairs with both `fuzzy.js` (through
    /// Node) and [`fuzzy_score`]. Skipped when Node or the JS source isn't
    /// available.
    #[test]
    fn fuzzy_matches_javascript() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let js = root.join("packages/desktop-ui/src/lib/fuzzy.js");
        if !js.exists()
            || std::process::Command::new("node")
                .arg("--version")
                .output()
                .is_err()
        {
            eprintln!("skipping: node or fuzzy.js not available");
            return;
        }
        let pairs = generated_pairs();
        let input: Vec<serde_json::Value> = pairs
            .iter()
            .map(|(t, p)| serde_json::json!([t, p]))
            .collect();
        let script = format!(
            "import {{ fuzzyScore }} from {js:?};\n\
             const pairs = JSON.parse(process.argv[1]);\n\
             console.log(JSON.stringify(pairs.map(([t, p]) => fuzzyScore(t, p))));",
            js = format!("file://{}", js.canonicalize().unwrap().display()),
        );
        let out = std::process::Command::new("node")
            .args([
                "--input-type=module",
                "-e",
                &script,
                &serde_json::to_string(&input).unwrap(),
            ])
            .output()
            .expect("run node");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let expected: Vec<i32> = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(expected.len(), pairs.len());
        for ((t, p), want) in pairs.iter().zip(expected) {
            assert_eq!(fuzzy_score(t, p), want, "fuzzyScore({t:?}, {p:?})");
        }
    }

    /// Labels, file names and edge cases, each paired with prefixes,
    /// substrings, subsequences, misses and mixed case.
    fn generated_pairs() -> Vec<(String, String)> {
        let targets = [
            "New File",
            "Delete File",
            "Duplicate File",
            "Toggle Sidebar",
            "toggle-sidebar",
            "Open Recent Project",
            "View: Zoom In",
            "notes/2026/october/meeting-notes.md",
            "snake_case_name.rs",
            "Command Palette",
            "a",
            "Ünïcödé Straße",
            "ΣΊΣΥΦΟΣ",
            "emoji 🎉 party",
            "x".repeat(80).as_str(),
            "Git: Commit and Push",
            "Settings…",
        ]
        .map(String::from);
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut out = Vec::new();
        for t in &targets {
            let chars: Vec<char> = t.chars().collect();
            out.push((t.clone(), String::new()));
            out.push((t.clone(), t.to_uppercase()));
            out.push((t.clone(), "zzq".into()));
            for _ in 0..20 {
                let n = chars.len();
                let a = (next() as usize) % n;
                let b = (a + 1 + (next() as usize) % 4).min(n);
                // Substring.
                out.push((t.clone(), chars[a..b].iter().collect()));
                // Subsequence: every char kept with probability 1/3.
                let sub: String = chars.iter().filter(|_| next() % 3 == 0).collect();
                out.push((t.clone(), sub));
                // Prefix, with random case.
                let pre: String = chars[..b]
                    .iter()
                    .map(|c| {
                        if next() % 2 == 0 {
                            c.to_uppercase().next().unwrap_or(*c)
                        } else {
                            *c
                        }
                    })
                    .collect();
                out.push((t.clone(), pre));
            }
        }
        out
    }
}
