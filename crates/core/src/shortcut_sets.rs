//! Shortcut sets (roadmap A293): the keymap saved under a name, switched between, deleted. The
//! built-in "default" set is the shipped table and is never stored or deleted. A set is a file
//! in the same text form as the key file (only what differs from the defaults), in a
//! `shortcut-sets` folder in the config folder; `active-shortcut-set.txt` names the set in use.

use std::io;
use std::path::{Path, PathBuf};

use crate::shortcuts::{Keymap, Overrides};

/// The built-in set: the shipped table.
pub const DEFAULT_SET: &str = "default";
const MAX_NAME: usize = 40;

/// A set name as stored: trimmed, 1 to 40 letters, digits, spaces, `-` or `_`, and not "default"
/// (any case), which is the built-in set.
pub fn valid_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a set needs a name".to_string());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("a set name is at most {MAX_NAME} characters"));
    }
    if !name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')) {
        return Err("a set name uses letters, digits, spaces, - and _ only".to_string());
    }
    if name.eq_ignore_ascii_case(DEFAULT_SET) {
        return Err("\"default\" is the built-in set and can't be saved over or deleted".to_string());
    }
    Ok(name.to_string())
}

/// Where the sets live.
#[derive(Debug, Clone)]
pub struct SetStore {
    dir: PathBuf,
}

impl SetStore {
    /// A store in `dir` (the config folder's `wavcraft` folder).
    pub fn new(dir: impl Into<PathBuf>) -> SetStore {
        SetStore { dir: dir.into() }
    }

    /// The store in the config folder, next to the key file.
    pub fn default_store() -> Option<SetStore> {
        dirs::config_dir().map(|d| SetStore::new(d.join("wavcraft")))
    }

    fn sets_dir(&self) -> PathBuf {
        self.dir.join("shortcut-sets")
    }
    fn set_path(&self, name: &str) -> PathBuf {
        self.sets_dir().join(format!("{name}.txt"))
    }
    fn active_path(&self) -> PathBuf {
        self.dir.join("active-shortcut-set.txt")
    }

    /// The saved sets' names, sorted. The built-in default is not listed.
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.sets_dir())
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".txt")).map(str::to_string))
                    .filter(|n| valid_name(n).is_ok())
                    .collect()
            })
            .unwrap_or_default();
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// Whether a set of this name is saved.
    pub fn exists(&self, name: &str) -> bool {
        self.set_path(name).is_file()
    }

    /// Save `map` as the set `name` (replacing a set of that name). Returns the name as stored.
    pub fn save(&self, name: &str, map: &Keymap) -> Result<String, String> {
        let name = valid_name(name)?;
        let text = map.overrides().to_text();
        let write = || -> io::Result<()> {
            std::fs::create_dir_all(self.sets_dir())?;
            let path = self.set_path(&name);
            let tmp = path.with_extension("txt.tmp");
            std::fs::write(&tmp, text)?;
            std::fs::rename(&tmp, path)
        };
        write().map_err(|e| format!("could not save the set {name:?}: {e}"))?;
        Ok(name)
    }

    /// The keymap of the set `name`; "default" is the shipped table.
    pub fn load(&self, name: &str) -> Result<Keymap, String> {
        if name.eq_ignore_ascii_case(DEFAULT_SET) {
            return Ok(Keymap::new());
        }
        let text = std::fs::read_to_string(self.set_path(name)).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => format!("no set named {name:?}"),
            _ => format!("could not read the set {name:?}: {e}"),
        })?;
        Overrides::parse(&text).and_then(|o| Keymap::from_overrides(&o)).map_err(|why| format!("the set {name:?} is broken ({why})"))
    }

    /// Delete the set `name`. The built-in default can't be deleted.
    pub fn delete(&self, name: &str) -> Result<(), String> {
        if name.eq_ignore_ascii_case(DEFAULT_SET) {
            return Err("the default set is built in and can't be deleted".to_string());
        }
        match std::fs::remove_file(self.set_path(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(format!("no set named {name:?}")),
            Err(e) => Err(format!("could not delete the set {name:?}: {e}")),
        }
    }

    /// The set in use: `None` is the default. A name whose file is gone counts as the default.
    pub fn active(&self) -> Option<String> {
        let name = std::fs::read_to_string(self.active_path()).ok()?.trim().to_string();
        (valid_name(&name).is_ok() && self.exists(&name)).then_some(name)
    }

    /// Make `name` the set in use at the next start (`None`: the default).
    pub fn set_active(&self, name: Option<&str>) -> io::Result<()> {
        match name {
            Some(n) => {
                std::fs::create_dir_all(&self.dir)?;
                std::fs::write(self.active_path(), n)
            }
            None => match std::fs::remove_file(self.active_path()) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
        }
    }

    /// The keymap to start with: the active set when there is one that loads, else `fallback`
    /// (the key file's keymap). The second value says why a set was not used, to be logged.
    pub fn startup(&self, fallback: Keymap) -> (Keymap, Option<String>) {
        match self.active() {
            None => (fallback, None),
            Some(name) => match self.load(&name) {
                Ok(map) => (map, None),
                Err(why) => (fallback, Some(format!("{why}: using the key file instead"))),
            },
        }
    }

    /// The folder the store works in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}
