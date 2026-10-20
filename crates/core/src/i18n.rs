//! Translatable interface text (roadmap A289): a small message catalogue, key -> text.
//!
//! English (`i18n/en.txt`) is the source: every key the code uses must be in it (a test scans the
//! source for `tr("…")` calls). Other languages are further built-in files in [`LANGUAGES`];
//! a key a language lacks falls back to English and is logged once. The pseudo-locale is made
//! from the English file by [`pseudo`] (accented letters, padded, in brackets) so untranslated
//! or clipped strings show up on screen; it needs no file.
//!
//! Text with blanks to fill uses `{0}`, `{1}`, … ([`tr_args`]). The language is a setting
//! (`language = <code>` in the settings file); [`set_language`] switches it at run time.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, PoisonError};

/// The English source catalogue.
const EN: &str = include_str!("../i18n/en.txt");

/// The code of the source language.
pub const ENGLISH: &str = "en";
/// The code of the pseudo-locale.
pub const PSEUDO: &str = "pseudo";

/// A built-in language: its code and, except for the generated pseudo-locale, its file.
/// To add one: put `i18n/<code>.txt` beside `en.txt`, list it here and add `language.<code>`
/// to `en.txt`.
pub const LANGUAGES: &[(&str, Option<&str>)] = &[(ENGLISH, Some(EN)), (PSEUDO, None)];

/// The language codes the app offers, in menu order.
pub fn language_codes() -> Vec<&'static str> {
    LANGUAGES.iter().map(|(c, _)| *c).collect()
}

/// Whether `code` names a built-in language.
pub fn is_language(code: &str) -> bool {
    LANGUAGES.iter().any(|(c, _)| *c == code)
}

/// The keys and texts of one catalogue file: `key = text` per line, `#` for comments.
pub fn parse(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

/// The pseudo-locale form of `text`: vowels and a few consonants accented, about a third longer
/// (`~` padding, as translations often are), in brackets so a clipped end shows. `{n}` blanks
/// are left alone.
pub fn pseudo(text: &str) -> String {
    let mut out = String::from("[");
    let (mut letters, mut in_blank) = (0usize, false);
    for c in text.chars() {
        match c {
            '{' => in_blank = true,
            '}' => in_blank = false,
            _ => {}
        }
        if in_blank || c == '}' {
            out.push(c);
            continue;
        }
        if c.is_alphabetic() {
            letters += 1;
        }
        out.push(match c {
            'a' => 'á',
            'e' => 'é',
            'i' => 'í',
            'o' => 'ó',
            'u' => 'ú',
            'y' => 'ý',
            'c' => 'ç',
            'n' => 'ñ',
            's' => 'š',
            'z' => 'ž',
            'A' => 'Á',
            'E' => 'É',
            'I' => 'Í',
            'O' => 'Ó',
            'U' => 'Ú',
            other => other,
        });
    }
    out.extend(std::iter::repeat_n('~', letters / 3));
    out.push(']');
    out
}

/// Fill `{0}`, `{1}`, … in `template` with `args` (a blank with no argument stays as written).
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = template.to_string();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

/// One language's catalogue with English behind it.
pub struct Translator {
    code: String,
    strings: HashMap<String, String>,
    english: HashMap<String, String>,
}

impl Translator {
    /// A translator for the built-in language `code` (an unknown code gives English).
    pub fn builtin(code: &str) -> Translator {
        let english = parse(EN);
        let code = if is_language(code) { code } else { ENGLISH };
        let strings = match LANGUAGES.iter().find(|(c, _)| *c == code) {
            Some((_, Some(file))) => parse(file),
            _ if code == PSEUDO => english.iter().map(|(k, v)| (k.clone(), pseudo(v))).collect(),
            _ => english.clone(),
        };
        Translator { code: code.to_string(), strings, english }
    }

    /// A translator over a given catalogue text (tests, and languages not built in).
    pub fn from_text(code: &str, text: &str) -> Translator {
        Translator { code: code.to_string(), strings: parse(text), english: parse(EN) }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    /// The text for `key`: this language, else English, else `None`. The bool is true when the
    /// language lacked the key.
    pub fn lookup(&self, key: &str) -> (Option<&str>, bool) {
        match self.strings.get(key) {
            Some(t) => (Some(t), false),
            None => (self.english.get(key).map(String::as_str), true),
        }
    }

    /// English keys this language lacks, sorted.
    pub fn missing(&self) -> Vec<String> {
        let mut v: Vec<String> = self.english.keys().filter(|k| !self.strings.contains_key(*k)).cloned().collect();
        v.sort();
        v
    }
}

struct State {
    translator: Translator,
    /// Keys already logged as missing (log once).
    logged: BTreeSet<String>,
    /// Keys looked up that no catalogue has at all.
    unknown: BTreeSet<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    let state = guard.get_or_insert_with(|| State { translator: Translator::builtin(ENGLISH), logged: BTreeSet::new(), unknown: BTreeSet::new() });
    f(state)
}

/// Switch the interface language. False (nothing changed) for a code that is not built in.
pub fn set_language(code: &str) -> bool {
    if !is_language(code) {
        return false;
    }
    with_state(|s| {
        s.translator = Translator::builtin(code);
        s.logged.clear();
    });
    true
}

/// The current language's code.
pub fn language() -> String {
    with_state(|s| s.translator.code().to_string())
}

/// The text for `key` in the current language. A key the language lacks gives the English text
/// and is logged once; a key nobody has gives the key itself.
pub fn tr(key: &str) -> String {
    with_state(|s| {
        let (text, lacked) = s.translator.lookup(key);
        let text = text.map(str::to_string);
        if lacked && s.logged.insert(key.to_string()) {
            if text.is_none() {
                s.unknown.insert(key.to_string());
            }
            log::warn!(target: "wavcraft", "i18n: no text for key {key:?} in language {:?}; {}", s.translator.code(), if text.is_some() { "using English" } else { "showing the key" });
        }
        text.unwrap_or_else(|| key.to_string())
    })
}

/// [`tr`] with `{0}`, `{1}`, … filled from `args`.
pub fn tr_args(key: &str, args: &[&str]) -> String {
    fill(&tr(key), args)
}

/// Keys missing in the current language: English keys it lacks, and keys the code asked for that
/// no catalogue has. Sorted.
pub fn missing() -> Vec<String> {
    with_state(|s| {
        let mut v: BTreeSet<String> = s.translator.missing().into_iter().collect();
        v.extend(s.unknown.iter().cloned());
        v.into_iter().collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_the_language_lacks_falls_back_to_english() {
        let t = Translator::from_text("xx", "menu.file = Datei\n");
        assert_eq!(t.lookup("menu.file"), (Some("Datei"), false));
        assert_eq!(t.lookup("menu.edit"), (Some("Edit"), true), "English text, flagged as missing");
        assert_eq!(t.lookup("no.such.key"), (None, true));
        assert!(t.missing().contains(&"menu.edit".to_string()));
        assert!(!t.missing().contains(&"menu.file".to_string()));
    }

    #[test]
    fn pseudo_accents_pads_brackets_and_keeps_blanks() {
        assert_eq!(pseudo("Save"), "[Sávé~]");
        let p = pseudo("Opening {0}… {1}%");
        assert!(p.starts_with('[') && p.ends_with(']'));
        assert!(p.contains("{0}") && p.contains("{1}"), "{p}");
        assert!(p.chars().count() > "Opening {0}… {1}%".chars().count());
        assert!(fill(&pseudo("Cursor {0}"), &["1.5"]).contains("1.5"));
    }

    #[test]
    fn the_pseudo_locale_covers_every_english_key() {
        let t = Translator::builtin(PSEUDO);
        assert!(t.missing().is_empty());
        assert_ne!(t.lookup("menu.file").0, Some("File"));
    }

    #[test]
    fn fill_replaces_numbered_blanks() {
        assert_eq!(fill("{1} before {0}", &["a", "b"]), "b before a");
        assert_eq!(fill("keeps {2}", &["a"]), "keeps {2}");
    }

    #[test]
    fn the_english_file_has_no_duplicate_or_empty_entries() {
        let mut seen = BTreeSet::new();
        for line in EN.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (k, v) = line.split_once('=').unwrap_or((line, ""));
            assert!(seen.insert(k.trim().to_string()), "duplicate key {k}");
            assert!(!v.trim().is_empty(), "empty text for {k}");
        }
    }

    #[test]
    fn every_action_in_the_table_has_its_menu_text_in_english() {
        let en = parse(EN);
        for a in crate::actions::TABLE {
            assert_eq!(en.get(&format!("action.{}", a.name)).map(String::as_str), Some(a.menu), "{}", a.name);
        }
    }

    /// Every key the code asks for is in the English file (a scan of the source for `tr("…")`
    /// and `tr_args("…", …)`).
    #[test]
    fn every_key_used_in_the_code_exists_in_english() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = Vec::new();
        for dir in ["crates/core/src", "apps/wavcraft/src"] {
            collect(&root.join(dir), &mut files);
        }
        assert!(files.iter().any(|f| f.ends_with("app.rs")), "the scan found the app source");
        let en = parse(EN);
        let mut used = 0;
        for f in files.iter().filter(|f| !f.ends_with("i18n.rs")) {
            let text = std::fs::read_to_string(f).unwrap_or_default();
            for key in keys_in(&text) {
                used += 1;
                assert!(en.contains_key(&key), "{} uses key {key:?}, which i18n/en.txt lacks", f.display());
            }
        }
        assert!(used > 20, "found only {used} keys; the scan is not seeing the calls");
    }

    fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// The string literals right after `tr(` / `tr_args(` (not part of a longer name).
    fn keys_in(text: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for call in ["tr(\"", "tr_args(\""] {
            let mut rest = text;
            while let Some(i) = rest.find(call) {
                let before = rest[..i].chars().next_back();
                let after = &rest[i + call.len()..];
                if !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                    && let Some(end) = after.find('"')
                {
                    keys.push(after[..end].to_string());
                }
                rest = after;
            }
        }
        keys
    }

    #[test]
    fn the_scan_reads_calls_and_skips_longer_names() {
        assert_eq!(keys_in("x(tr(\"a.b\"), tr_args(\"c\", &[]), str(\"no\"), mytr(\"no\"))"), ["a.b", "c"]);
    }

    #[test]
    fn switching_language_changes_the_text_and_unknown_codes_are_refused() {
        assert!(set_language(PSEUDO));
        assert_eq!(language(), PSEUDO);
        assert_ne!(tr("menu.file"), "File");
        assert!(missing().is_empty());
        assert!(!set_language("klingon"));
        assert_eq!(language(), PSEUDO, "unchanged");
        assert_eq!(tr("no.such.key"), "no.such.key");
        assert_eq!(missing(), ["no.such.key"]);
        assert!(set_language(ENGLISH));
        assert_eq!(tr("menu.file"), "File");
    }
}
