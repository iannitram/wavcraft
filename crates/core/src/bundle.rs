//! Export and import of the application settings as one file, and the preference reset (A017,
//! A290).
//!
//! The settings live in the config folder (`dirs::config_dir()/wavcraft`): `settings.txt`, and
//! when they exist `shortcuts.txt` (shortcut overrides) and `workspaces/<name>.txt` (saved
//! workspaces). Neither of the last two is written by the app yet; the bundle carries them so a
//! later feature's files travel with the rest without a change here.
//!
//! The bundle is one text file:
//!
//! ```text
//! WAVCRAFT-SETTINGS 1
//! file settings.txt 54
//! <exactly 54 bytes of UTF-8>
//! file shortcuts.txt 12
//! <exactly 12 bytes>
//! end
//! ```
//!
//! [`Bundle::parse`] checks everything before [`apply`] touches a file: the header, each name
//! (only the three kinds above, no folders, no `..`), sizes, that every file is text, and that
//! `settings.txt` is present and made only of keys this version knows. A bundle that fails any
//! check changes nothing. [`apply`] writes the new files beside the old ones first, then moves
//! each old file aside as `<name>.old` (replacing an older `.old`) and renames the new one in.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use crate::settings::Settings;

/// The first line of a bundle.
pub const MAGIC: &str = "WAVCRAFT-SETTINGS 1";
/// A bundle larger than this is refused.
pub const MAX_BUNDLE_BYTES: usize = 4 * 1024 * 1024;
/// More files than this is refused.
pub const MAX_FILES: usize = 64;
/// The file every bundle carries.
pub const SETTINGS_FILE: &str = "settings.txt";
/// Shortcut overrides, when the app has any.
pub const SHORTCUTS_FILE: &str = "shortcuts.txt";
const WORKSPACES_DIR: &str = "workspaces";

/// The files that are preferences (what a reset puts aside; workspaces are the person's work,
/// not preferences, so a reset leaves them).
const PREFERENCE_FILES: [&str; 2] = [SETTINGS_FILE, SHORTCUTS_FILE];

/// Whether `name` is a file a bundle may carry: `settings.txt`, `shortcuts.txt`, or
/// `workspaces/<name>.txt` with a plain name.
pub fn is_bundle_name(name: &str) -> bool {
    if name == SETTINGS_FILE || name == SHORTCUTS_FILE {
        return true;
    }
    let Some(stem) = name.strip_prefix("workspaces/").and_then(|n| n.strip_suffix(".txt")) else { return false };
    !stem.is_empty()
        && stem.len() <= 64
        && !stem.starts_with('.')
        && stem.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'))
}

/// The files of a settings bundle: relative name to text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bundle {
    pub files: BTreeMap<String, String>,
}

impl Bundle {
    /// Read the settings files in `dir` (the config folder). `settings.txt` is always in the
    /// result: the defaults' text when the file is missing.
    pub fn collect(dir: &Path) -> io::Result<Bundle> {
        let mut files = BTreeMap::new();
        for name in [SETTINGS_FILE, SHORTCUTS_FILE] {
            match std::fs::read_to_string(dir.join(name)) {
                Ok(t) => {
                    files.insert(name.to_string(), t);
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        files.entry(SETTINGS_FILE.to_string()).or_insert_with(|| Settings::default().to_text());
        if let Ok(rd) = std::fs::read_dir(dir.join(WORKSPACES_DIR)) {
            for entry in rd.flatten() {
                let name = format!("{WORKSPACES_DIR}/{}", entry.file_name().to_string_lossy());
                if is_bundle_name(&name) && entry.file_type().is_ok_and(|t| t.is_file()) {
                    files.insert(name, std::fs::read_to_string(entry.path())?);
                }
            }
        }
        Ok(Bundle { files })
    }

    /// The bundle's text.
    pub fn to_text(&self) -> String {
        let mut out = format!("{MAGIC}\n");
        for (name, text) in &self.files {
            out.push_str(&format!("file {name} {}\n{text}\n", text.len()));
        }
        out.push_str("end\n");
        out
    }

    /// Read and check a bundle. The error says what is wrong, in words.
    pub fn parse(text: &str) -> Result<Bundle, String> {
        if text.len() > MAX_BUNDLE_BYTES {
            return Err(format!("the file is larger than {} MiB, so it is not a settings file", MAX_BUNDLE_BYTES / 1024 / 1024));
        }
        let mut rest = text;
        let take_line = |rest: &mut &str| -> Option<String> {
            let (line, tail) = rest.split_once('\n')?;
            *rest = tail;
            Some(line.trim_end_matches('\r').to_string())
        };
        if take_line(&mut rest).as_deref() != Some(MAGIC) {
            return Err(format!("this is not a WavCraft settings file (its first line should be \"{MAGIC}\")"));
        }
        let mut files = BTreeMap::new();
        loop {
            let Some(line) = take_line(&mut rest) else { return Err("the file ends early (no \"end\" line); it may be cut short".into()) };
            if line == "end" {
                break;
            }
            let Some((name, len)) = line.strip_prefix("file ").and_then(|spec| spec.rsplit_once(' ')) else {
                return Err(format!("unexpected line {line:?}"));
            };
            let len: usize = len.parse().map_err(|_| format!("bad length in {line:?}"))?;
            if !is_bundle_name(name) {
                return Err(format!("{name:?} is not a settings file WavCraft keeps"));
            }
            if files.len() >= MAX_FILES {
                return Err(format!("more than {MAX_FILES} files"));
            }
            if len > rest.len() || !rest.is_char_boundary(len) {
                return Err(format!("{name} is cut short or damaged"));
            }
            let (body, tail) = rest.split_at(len);
            let Some(tail) = tail.strip_prefix('\n') else { return Err(format!("{name} is damaged (its length does not match)")) };
            rest = tail;
            if body.contains('\0') {
                return Err(format!("{name} is not text"));
            }
            if files.insert(name.to_string(), body.to_string()).is_some() {
                return Err(format!("{name} appears twice"));
            }
        }
        let Some(settings) = files.get(SETTINGS_FILE) else { return Err("there is no settings.txt in it".into()) };
        Settings::validate(settings).map_err(|e| format!("settings.txt: {e}"))?;
        Ok(Bundle { files })
    }
}

/// Write the bundle of the settings in `dir` to `path` (replacing the file whole).
pub fn export(dir: &Path, path: &Path) -> io::Result<()> {
    let text = Bundle::collect(dir)?.to_text();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp-export");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Read the bundle at `path` and check it; nothing is changed.
pub fn read_checked(path: &Path) -> Result<Bundle, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    if meta.len() > MAX_BUNDLE_BYTES as u64 {
        return Err("the file is too large to be a settings file".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| "the file is not text, so it is not a settings file".to_string())?;
    Bundle::parse(&text)
}

/// Check the bundle at `path`, then replace the settings in `dir` with it. Every old file the
/// bundle replaces stays beside the new one as `<name>.old`. Returns the names written. If the
/// bundle fails a check, or a write fails, the settings are as they were (a failure part-way
/// through puts the files already moved back).
pub fn import(dir: &Path, path: &Path) -> Result<Vec<String>, String> {
    let bundle = read_checked(path)?;
    apply(dir, &bundle)
}

/// Write a checked [`Bundle`] into `dir`; see [`import`].
pub fn apply(dir: &Path, bundle: &Bundle) -> Result<Vec<String>, String> {
    // Check again: `apply` may be handed a Bundle built by hand.
    if let Some(bad) = bundle.files.keys().find(|n| !is_bundle_name(n)) {
        return Err(format!("{bad:?} is not a settings file WavCraft keeps"));
    }
    let settings = bundle.files.get(SETTINGS_FILE).ok_or("there is no settings.txt in it")?;
    Settings::validate(settings).map_err(|e| format!("settings.txt: {e}"))?;
    let fail = |what: &str, e: io::Error| format!("{what}: {e}");
    // 1. Stage every new file beside its target.
    let mut staged: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
    let cleanup = |staged: &[(std::path::PathBuf, std::path::PathBuf)]| {
        for (tmp, _) in staged {
            let _ = std::fs::remove_file(tmp);
        }
    };
    for (name, text) in &bundle.files {
        let target = dir.join(name);
        let tmp = target.with_file_name(format!("{}.new", target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
        let r = target.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&tmp, text));
        if let Err(e) = r {
            cleanup(&staged);
            let _ = std::fs::remove_file(&tmp);
            return Err(fail(&format!("could not write {name}"), e));
        }
        staged.push((tmp, target));
    }
    // 2. Put each old file aside and move the new one in; undo on a failure.
    let mut moved: Vec<(std::path::PathBuf, Option<std::path::PathBuf>)> = Vec::new();
    for (tmp, target) in &staged {
        let old = old_path(target);
        let had = target.exists();
        let step = (|| -> io::Result<()> {
            if had {
                std::fs::copy(target, &old)?;
            }
            std::fs::rename(tmp, target)
        })();
        match step {
            Ok(()) => moved.push((target.clone(), had.then_some(old))),
            Err(e) => {
                for (t, old) in moved.iter().rev() {
                    match old {
                        Some(o) => {
                            let _ = std::fs::copy(o, t);
                        }
                        None => {
                            let _ = std::fs::remove_file(t);
                        }
                    }
                }
                cleanup(&staged);
                return Err(fail(&format!("could not replace {}", target.display()), e));
            }
        }
    }
    Ok(bundle.files.keys().cloned().collect())
}

/// Put the preference files in `dir` aside as `<name>.old` (replacing an older `.old`), so the
/// next start uses the defaults. Returns the names moved; workspaces are left where they are.
pub fn reset_preferences(dir: &Path) -> io::Result<Vec<String>> {
    let mut moved = Vec::new();
    for name in PREFERENCE_FILES {
        let path = dir.join(name);
        if path.is_file() {
            let old = old_path(&path);
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&path, &old)?;
            moved.push(name.to_string());
        }
    }
    Ok(moved)
}

fn old_path(path: &Path) -> std::path::PathBuf {
    path.with_file_name(format!("{}.old", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wavcraft-bundle-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn write(dir: &Path, name: &str, text: &str) {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(p, text).expect("write");
    }

    #[test]
    fn export_then_import_round_trips_every_file() {
        let (a, b) = (scratch("rt-a"), scratch("rt-b"));
        write(&a, "settings.txt", "session_folder = /some/where\n");
        write(&a, "shortcuts.txt", "edit.undo = Ctrl+U\nnaïve = ünï\n");
        write(&a, "workspaces/Mixing 1.txt", "panel = left\n\nlast line without newline");
        let file = a.join("out").join("bundle.wavset");
        export(&a, &file).expect("export");
        let names = import(&b, &file).expect("import");
        assert_eq!(names, ["settings.txt", "shortcuts.txt", "workspaces/Mixing 1.txt"]);
        assert_eq!(Bundle::collect(&a).expect("a"), Bundle::collect(&b).expect("b"));
        assert_eq!(std::fs::read_to_string(b.join("workspaces/Mixing 1.txt")).expect("read"), "panel = left\n\nlast line without newline");
    }

    #[test]
    fn a_missing_settings_file_exports_the_defaults() {
        let a = scratch("defaults");
        let b = Bundle::collect(&a).expect("collect");
        assert_eq!(b.files.len(), 1);
        assert_eq!(Settings::parse(&b.files["settings.txt"]), Settings::default());
    }

    #[test]
    fn import_keeps_the_old_files_aside() {
        let (a, b) = (scratch("aside-a"), scratch("aside-b"));
        write(&a, "settings.txt", "session_folder = /new\n");
        let file = a.join("s.wavset");
        export(&a, &file).expect("export");
        write(&b, "settings.txt", "session_folder = /old\n");
        import(&b, &file).expect("import");
        assert_eq!(std::fs::read_to_string(b.join("settings.txt")).expect("r"), "session_folder = /new\n");
        assert_eq!(std::fs::read_to_string(b.join("settings.txt.old")).expect("r"), "session_folder = /old\n");
    }

    #[test]
    fn a_bad_bundle_changes_nothing() {
        let a = scratch("bad");
        write(&a, "settings.txt", "session_folder = /keep\n");
        let good = Bundle { files: BTreeMap::from([("settings.txt".to_string(), "session_folder = /x\n".to_string())]) }.to_text();
        let cases: Vec<(&str, String)> = vec![
            ("empty", String::new()),
            ("wrong header", "hello\nend\n".into()),
            ("no end", good.replace("end\n", "")),
            ("cut short", good[..good.len() - 12].to_string()),
            ("wrong length", good.replace("file settings.txt 20", "file settings.txt 19")),
            ("no settings.txt", format!("{MAGIC}\nfile shortcuts.txt 1\nx\nend\n")),
            ("traversal", format!("{MAGIC}\nfile ../settings.txt 1\nx\nend\n")),
            ("absolute", format!("{MAGIC}\nfile /etc/passwd 1\nx\nend\n")),
            ("nested", format!("{MAGIC}\nfile workspaces/a/b.txt 1\nx\nend\n")),
            ("dot-dot workspace", format!("{MAGIC}\nfile workspaces/..txt 1\nx\nend\n")),
            ("unknown file", format!("{MAGIC}\nfile other.bin 1\nx\nend\n")),
            ("unknown key", format!("{MAGIC}\nfile settings.txt 9\nbogus = 1\nend\n")),
            ("line without =", format!("{MAGIC}\nfile settings.txt 4\nabcd\nend\n")),
            ("twice", format!("{MAGIC}\nfile settings.txt 0\n\nfile settings.txt 0\n\nend\n")),
        ];
        for (what, text) in cases {
            let file = a.join("in.wavset");
            std::fs::write(&file, &text).expect("write");
            assert!(import(&a, &file).is_err(), "{what} must be refused");
            assert_eq!(std::fs::read_to_string(a.join("settings.txt")).expect("r"), "session_folder = /keep\n", "{what} changed the file");
            assert!(!a.join("settings.txt.old").exists(), "{what} left a .old");
            assert!(!a.join("settings.txt.new").exists(), "{what} left a .new");
        }
    }

    #[test]
    fn a_file_that_is_not_text_or_is_huge_is_refused() {
        let a = scratch("binary");
        let file = a.join("b.wavset");
        std::fs::write(&file, [0xff, 0xfe, 0x00, 0x01]).expect("write");
        assert!(read_checked(&file).is_err());
        assert!(Bundle::parse(&"x".repeat(MAX_BUNDLE_BYTES + 1)).is_err());
        assert!(read_checked(&a.join("missing")).is_err());
    }

    #[test]
    fn reset_puts_preferences_aside_and_leaves_workspaces() {
        let a = scratch("reset");
        write(&a, "settings.txt", "session_folder = /x\n");
        write(&a, "shortcuts.txt", "a = b\n");
        write(&a, "workspaces/w.txt", "w\n");
        write(&a, "settings.txt.old", "stale\n");
        let moved = reset_preferences(&a).expect("reset");
        assert_eq!(moved, ["settings.txt", "shortcuts.txt"]);
        assert!(!a.join("settings.txt").exists());
        assert_eq!(std::fs::read_to_string(a.join("settings.txt.old")).expect("r"), "session_folder = /x\n");
        assert_eq!(std::fs::read_to_string(a.join("shortcuts.txt.old")).expect("r"), "a = b\n");
        assert!(a.join("workspaces/w.txt").is_file());
        assert!(reset_preferences(&a).expect("again").is_empty(), "nothing left to move");
    }

    #[test]
    fn names_are_checked() {
        for ok in ["settings.txt", "shortcuts.txt", "workspaces/Mix 1.txt", "workspaces/a-b_c.d.txt"] {
            assert!(is_bundle_name(ok), "{ok}");
        }
        for bad in [
            "",
            "workspaces/.txt",
            "workspaces/.hidden.txt",
            "workspaces/../x.txt",
            "workspaces/a/b.txt",
            "Settings.txt",
            "settings.txt/",
            "workspaces/a.bin",
            "workspaces/é.txt",
        ] {
            assert!(!is_bundle_name(bad), "{bad}");
        }
    }
}
