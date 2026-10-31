//! Spell checking for the editor. The webview builds used the system
//! checker for the system language (`spellCheckEnabled`, F7); the native
//! build loads a Hunspell dictionary into `laankit-editor`'s checker.
//!
//! Dictionaries are looked up for the system language (`LANG`, e.g.
//! `en_CA.UTF-8` → `en_CA`, then `en_US`) in:
//!
//! | Where                                            | Platform        |
//! |--------------------------------------------------|-----------------|
//! | `<userData>/dictionaries/`                       | all             |
//! | `/usr/share/hunspell`, `/usr/share/myspell[/dicts]` | Linux        |
//! | `~/Library/Spelling`, `/Library/Spelling`        | macOS           |
//!
//! `<userData>` is `~/.config/NoteLiner`, `%APPDATA%\NoteLiner` or
//! `~/Library/Application Support/NoteLiner`. Windows has no shared Hunspell
//! folder, so put `en_US.aff` and `en_US.dic` (e.g. from LibreOffice's
//! dictionaries) in `<userData>\dictionaries`. Without a dictionary, the
//! editor simply doesn't underline anything.
//!
//! "Add to dictionary" words are kept in `<userData>/dictionary.txt`, one per
//! line (a native-build file; the webview builds kept theirs in Chromium's
//! profile).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use laankit_editor::{Hunspell, SpellChecker};

const WORDS_FILE: &str = "dictionary.txt";

/// The dictionary plus the user's own words.
pub struct AppSpell {
    dict: Hunspell,
    words: RwLock<HashSet<String>>,
    words_file: PathBuf,
}

impl SpellChecker for AppSpell {
    fn check(&self, word: &str) -> bool {
        let known = self
            .words
            .read()
            .is_ok_and(|w| w.contains(word) || w.contains(&word.to_lowercase()));
        known || self.dict.check(word)
    }

    fn suggest(&self, word: &str) -> Vec<String> {
        self.dict.suggest(word)
    }
}

impl AppSpell {
    /// Accepts `word` from now on, and remembers it in `dictionary.txt`.
    pub fn add_word(&self, word: &str) {
        let added = self
            .words
            .write()
            .is_ok_and(|mut w| w.insert(word.to_string()));
        if !added {
            return;
        }
        let mut list: Vec<String> = self
            .words
            .read()
            .map(|w| w.iter().cloned().collect())
            .unwrap_or_default();
        list.sort();
        if let Some(dir) = self.words_file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&self.words_file, list.join("\n") + "\n") {
            log::warn!("Couldn't save {}: {e}", self.words_file.display());
        }
    }
}

/// A dictionary loading in the background, picked up by the app once ready.
#[derive(Clone, Default)]
pub struct Loader {
    slot: Arc<OnceLock<Option<Arc<AppSpell>>>>,
    started: Arc<Mutex<bool>>,
}

impl Loader {
    /// Starts loading (once) for the system language.
    pub fn start(&self, user_data: &Path) {
        let Ok(mut started) = self.started.lock() else {
            return;
        };
        if *started {
            return;
        }
        *started = true;
        let slot = self.slot.clone();
        let user_data = user_data.to_path_buf();
        std::thread::spawn(move || {
            let _ = slot.set(load(&user_data, &system_language()));
        });
    }

    /// The checker, once loaded; `Some(None)` when there's no dictionary.
    pub fn ready(&self) -> Option<Option<Arc<AppSpell>>> {
        self.slot.get().cloned()
    }
}

/// Loads the dictionary for `lang` and the user's words.
pub fn load(user_data: &Path, lang: &str) -> Option<Arc<AppSpell>> {
    let (aff, dic) = find_dictionary(user_data, lang)?;
    let read = |p: &Path| std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned());
    let (aff_text, dic_text) = match (read(&aff), read(&dic)) {
        (Ok(a), Ok(d)) => (a, d),
        (Err(e), _) | (_, Err(e)) => {
            log::warn!("Couldn't read {}: {e}", dic.display());
            return None;
        }
    };
    let dict = match Hunspell::new(&aff_text, &dic_text) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Couldn't load {}: {e}", dic.display());
            return None;
        }
    };
    let words_file = user_data.join(WORDS_FILE);
    let words = std::fs::read_to_string(&words_file)
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    log::info!("Spell checking with {}", dic.display());
    Some(Arc::new(AppSpell {
        dict,
        words: RwLock::new(words),
        words_file,
    }))
}

/// `en_CA.UTF-8` → `en_CA` (as `webkit::system_language` does).
pub fn system_language() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .map(|v| {
            v.split(['.', '@'])
                .next()
                .unwrap_or("en_US")
                .replace('-', "_")
        })
        .unwrap_or_else(|| "en_US".into())
}

/// The `.aff`/`.dic` pair for `lang` (or its fallbacks) in the search folders.
pub fn find_dictionary(user_data: &Path, lang: &str) -> Option<(PathBuf, PathBuf)> {
    let mut langs = vec![lang.to_string()];
    if let Some(base) = lang.split('_').next() {
        if base == "en" {
            langs.push("en_US".into());
            langs.push("en_GB".into());
        }
        langs.push(base.to_string());
    }
    for dir in search_dirs(user_data) {
        for l in &langs {
            let aff = dir.join(format!("{l}.aff"));
            let dic = dir.join(format!("{l}.dic"));
            if aff.is_file() && dic.is_file() {
                return Some((aff, dic));
            }
        }
    }
    None
}

fn search_dirs(user_data: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![user_data.join("dictionaries")];
    if cfg!(target_os = "macos") {
        dirs.push(laankit_core::paths::home_dir().join("Library/Spelling"));
        dirs.push(PathBuf::from("/Library/Spelling"));
    } else if cfg!(unix) {
        for d in [
            "/usr/share/hunspell",
            "/usr/share/myspell",
            "/usr/share/myspell/dicts",
        ] {
            dirs.push(PathBuf::from(d));
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\n";
    const DIC: &str = "3\nhello\nworld\nnote\n";

    #[test]
    fn user_dictionary_and_words() {
        let dir = tempfile::tempdir().unwrap();
        let dicts = dir.path().join("dictionaries");
        std::fs::create_dir_all(&dicts).unwrap();
        std::fs::write(dicts.join("xx_YY.aff"), AFF).unwrap();
        std::fs::write(dicts.join("xx_YY.dic"), DIC).unwrap();
        let spell = load(dir.path(), "xx_YY").expect("found in <userData>/dictionaries");
        assert!(spell.check("hello"));
        assert!(!spell.check("helo"));
        spell.add_word("LaanKit");
        assert!(spell.check("LaanKit"));
        let saved = std::fs::read_to_string(dir.path().join(WORDS_FILE)).unwrap();
        assert_eq!(saved, "LaanKit\n");
        // Reloading keeps the user's words.
        assert!(load(dir.path(), "xx_YY").unwrap().check("LaanKit"));
    }

    #[test]
    fn no_dictionary_degrades() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_dictionary(dir.path(), "zz_QQ").is_none());
        assert!(load(dir.path(), "zz_QQ").is_none());
    }
}
