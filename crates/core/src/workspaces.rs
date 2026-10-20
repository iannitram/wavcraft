//! Workspaces (roadmap A009, A010): a named snapshot of the panel layout (which panels are shown,
//! the zones, the tab order, the front tabs and the zone sizes), kept as one small text file per
//! workspace in `<config dir>/wavcraft/workspaces/`.
//!
//! The built-in workspaces ([`BUILTIN`]: Default, Editing, Recording, Repair; roadmap A011) are
//! code, not files: each is its own arrangement and set of shown panels, none is ever written, saved
//! over or deleted, no saved workspace may take one of their names (compared without regard to
//! case), and resetting one gives back its built-in state. [`DEFAULT`] is the registry's own layout
//! (A001: the editor in the centre, the History panel on the right, Selection along the bottom). The workspace in use is remembered by the app in the settings file; this
//! module never touches a window.

use crate::panels::{DockSide, Layout, PanelSet};
use std::path::PathBuf;

/// The first built-in workspace's name: the registry's own layout, and the one a fresh install uses.
pub const DEFAULT: &str = "Default";
/// The built-in workspaces, in menu order.
pub const BUILTIN: [&str; 4] = [DEFAULT, "Editing", "Recording", "Repair"];

/// Whether `name` (any case) is a built-in workspace.
pub fn is_builtin(name: &str) -> bool {
    builtin_name(name).is_some()
}

/// A built-in workspace's own spelling.
pub fn builtin_name(name: &str) -> Option<&'static str> {
    BUILTIN.iter().copied().find(|b| same(b, name.trim()))
}

/// The layout of a built-in workspace (by its own spelling).
fn builtin_snapshot(name: &str) -> Snapshot {
    let (mut set, mut layout) = (PanelSet::default(), Layout::default());
    let place = |layout: &mut Layout, id: &str, zone: DockSide| {
        let _ = layout.move_to(id, zone, None);
    };
    match name {
        // Editing: the edit history beside the audio on the left, the selection on the right.
        "Editing" => {
            place(&mut layout, "history", DockSide::Left);
            place(&mut layout, "selection", DockSide::Right);
        }
        // Recording: the waveform only, the history out of the way, the selection below.
        "Recording" => {
            set.set("spectral", false);
            set.set("history", false);
        }
        // Repair: the spectral display with the selection beside it; the waveform and history below.
        "Repair" => {
            set.set("waveform", false);
            place(&mut layout, "selection", DockSide::Right);
            place(&mut layout, "history", DockSide::Bottom);
        }
        _ => {}
    }
    let (tabs, sizes) = layout.encode();
    Snapshot { hidden: set.hidden_ids(), tabs, sizes }
}
/// The longest name, in characters.
pub const MAX_NAME_CHARS: usize = 40;
const EXT: &str = "workspace";

/// A layout as a workspace keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Ids of the hidden panels, in registry order.
    pub hidden: Vec<String>,
    /// As `Layout::encode` writes the tab order.
    pub tabs: String,
    /// As `Layout::encode` writes the zone sizes.
    pub sizes: String,
}

impl Snapshot {
    pub fn capture(set: &PanelSet, layout: &Layout) -> Snapshot {
        let (tabs, sizes) = layout.encode();
        Snapshot { hidden: set.hidden_ids(), tabs, sizes }
    }

    /// The layout of [`DEFAULT`].
    pub fn builtin() -> Snapshot {
        builtin_snapshot(DEFAULT)
    }

    /// The panels and layout this snapshot stands for (what makes no sense is ignored).
    pub fn restore(&self) -> (PanelSet, Layout) {
        (PanelSet::from_hidden(&self.hidden), Layout::decode(&self.tabs, &self.sizes))
    }

    fn to_text(&self, name: &str) -> String {
        format!(
            "# A WavCraft workspace: the panel layout under a name.\nname = {name}\nhidden_panels = {}\npanel_layout = {}\nzone_sizes = {}\n",
            self.hidden.join(","),
            self.tabs,
            self.sizes
        )
    }

    fn parse(text: &str) -> Option<(String, Snapshot)> {
        let (mut name, mut hidden, mut tabs, mut sizes) = (None, String::new(), String::new(), String::new());
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "name" => name = Some(v.to_string()),
                "hidden_panels" => hidden = v.to_string(),
                "panel_layout" => tabs = v.to_string(),
                "zone_sizes" => sizes = v.to_string(),
                _ => {}
            }
        }
        let hidden: Vec<String> = hidden.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
        // Round through the decoders so a hand-edited file settles to something valid.
        let (set, layout) = Snapshot { hidden, tabs, sizes }.restore();
        Some((name?, Snapshot::capture(&set, &layout)))
    }
}

/// A name as it will be kept: spaces tidied; letters, digits, space, `-` and `_` only; not empty,
/// at most [`MAX_NAME_CHARS`] long.
pub fn clean_name(raw: &str) -> Result<String, String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("give the workspace a name".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("a workspace name is at most {MAX_NAME_CHARS} characters"));
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_') {
        return Err("a workspace name may hold letters, digits, spaces, - and _ only".into());
    }
    Ok(name)
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// The saved workspaces and which one is in use.
#[derive(Debug, Clone)]
pub struct Workspaces {
    dir: Option<PathBuf>,
    current: String,
}

impl Workspaces {
    /// `dir` is the folder the files live in (`None`: this system has no config folder, so nothing
    /// can be saved). `current` falls back to [`DEFAULT`] if no such workspace exists.
    pub fn new(dir: Option<PathBuf>, current: &str) -> Workspaces {
        let mut w = Workspaces { dir, current: DEFAULT.to_string() };
        if let Some(name) = w.find(current) {
            w.current = name;
        }
        w
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    fn file(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{}.{EXT}", name.to_lowercase().replace(' ', "_"))))
    }

    fn read(&self, name: &str) -> Option<(String, Snapshot)> {
        let text = std::fs::read_to_string(self.file(name)?).ok()?;
        Snapshot::parse(&text).filter(|(n, _)| same(n, name))
    }

    /// The exact spelling of a workspace by name (any case), if it exists.
    pub fn find(&self, name: &str) -> Option<String> {
        let name = name.trim();
        if let Some(b) = builtin_name(name) {
            return Some(b.to_string());
        }
        self.read(name).map(|(n, _)| n)
    }

    /// The built-in ones first, then the saved ones in alphabetical order.
    pub fn names(&self) -> Vec<String> {
        let mut saved: Vec<String> = match self.dir.as_ref().and_then(|d| std::fs::read_dir(d).ok()) {
            Some(rd) => rd
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == EXT))
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| Snapshot::parse(&t).map(|(n, _)| n))
                .filter(|n| !is_builtin(n))
                .collect(),
            None => Vec::new(),
        };
        saved.sort_by_key(|n| n.to_lowercase());
        let mut all: Vec<String> = BUILTIN.iter().map(|b| b.to_string()).collect();
        all.extend(saved);
        all
    }

    /// The `n`th workspace (1 is `Default`) in the order [`Workspaces::names`] lists them: what the
    /// keys `workspace.switch.<n>` switch to.
    pub fn nth(&self, n: usize) -> Option<String> {
        n.checked_sub(1).and_then(|i| self.names().into_iter().nth(i))
    }

    /// What the named workspace holds.
    pub fn snapshot_of(&self, name: &str) -> Result<Snapshot, String> {
        if let Some(b) = builtin_name(name) {
            return Ok(builtin_snapshot(b));
        }
        self.read(name.trim()).map(|(_, s)| s).ok_or_else(|| format!("no workspace named {:?}", name.trim()))
    }

    /// Store `snap` under `name` (replacing a saved workspace of that name) and make it the one in
    /// use. Returns the name as kept.
    pub fn save(&mut self, name: &str, snap: &Snapshot) -> Result<String, String> {
        let name = clean_name(name)?;
        if let Some(b) = builtin_name(&name) {
            return Err(format!("{b} is built in and can't be replaced; pick another name"));
        }
        let Some(file) = self.file(&name) else { return Err("this system has no settings folder, so a workspace can't be saved".into()) };
        // A different spelling of an existing name replaces it under the new spelling.
        std::fs::create_dir_all(file.parent().unwrap_or(&PathBuf::new())).map_err(|e| format!("could not make the workspaces folder: {e}"))?;
        let tmp = file.with_extension("workspace.new");
        std::fs::write(&tmp, snap.to_text(&name))
            .and_then(|()| std::fs::rename(&tmp, &file))
            .map_err(|e| format!("could not save the workspace: {e}"))?;
        self.current = name.clone();
        Ok(name)
    }

    /// Make a workspace the one in use and return its layout.
    pub fn switch(&mut self, name: &str) -> Result<Snapshot, String> {
        let found = self.find(name).ok_or_else(|| format!("no workspace named {:?}", name.trim()))?;
        let snap = self.snapshot_of(&found)?;
        self.current = found;
        Ok(snap)
    }

    /// The saved layout of `name`, or of the one in use when `None`; that workspace becomes the one in use.
    pub fn reset(&mut self, name: Option<&str>) -> Result<Snapshot, String> {
        match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => self.switch(n),
            None => {
                let cur = self.current.clone();
                self.switch(&cur)
            }
        }
    }

    /// Remove a saved workspace: not [`DEFAULT`], not the one in use.
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if let Some(b) = builtin_name(name) {
            return Err(format!("{b} is built in and can't be deleted"));
        }
        let found = self.find(name).ok_or_else(|| format!("no workspace named {name:?}"))?;
        if same(&found, &self.current) {
            return Err(format!("{found:?} is the workspace in use; switch to another one first"));
        }
        let file = self.file(&found).ok_or("no settings folder")?;
        std::fs::remove_file(file).map_err(|e| format!("could not delete the workspace: {e}"))
    }

    /// Whether the layout now differs from the saved state of the workspace in use.
    pub fn is_modified(&self, now: &Snapshot) -> bool {
        self.snapshot_of(&self.current).map_or(true, |saved| saved != *now)
    }

    /// For the control channel: `current=<name> modified=0|1 workspaces=<a>,<b>` (names are listed
    /// with `|` between them, since a name may hold spaces).
    pub fn report(&self, now: &Snapshot) -> String {
        format!(
            "current={} modified={} workspaces={}",
            self.current.replace(' ', "_"),
            u8::from(self.is_modified(now)),
            self.names().join("|").replace(' ', "_")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::DockSide;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wavcraft-ws-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn moved(id: &str, zone: DockSide) -> Snapshot {
        let (mut set, mut layout) = (PanelSet::default(), Layout::default());
        layout.move_to(id, zone, None).unwrap();
        set.set("selection", false);
        Snapshot::capture(&set, &layout)
    }

    #[test]
    fn default_is_built_in_and_protected() {
        let dir = tmp("default");
        let mut w = Workspaces::new(Some(dir.clone()), "nope");
        assert_eq!(w.current(), DEFAULT);
        assert_eq!(w.names(), BUILTIN.to_vec());
        assert!(w.save("default", &Snapshot::builtin()).is_err());
        assert!(w.save(" DEFAULT ", &Snapshot::builtin()).is_err());
        assert!(w.delete("Default").is_err());
        assert!(w.delete("default").is_err());
        assert!(!w.is_modified(&Snapshot::builtin()));
        assert_eq!(w.snapshot_of("DEFAULT").unwrap(), Snapshot::builtin());
        assert!(!dir.exists(), "the built-in workspace writes nothing");
    }

    #[test]
    fn the_built_in_workspaces_differ_and_cannot_be_changed() {
        let dir = tmp("builtin");
        let mut w = Workspaces::new(Some(dir.clone()), "recording");
        assert_eq!(w.current(), "Recording", "a built-in is remembered between runs");
        let snaps: Vec<Snapshot> = BUILTIN.iter().map(|b| w.snapshot_of(b).unwrap()).collect();
        for (i, a) in snaps.iter().enumerate() {
            assert!(snaps[i + 1..].iter().all(|b| a != b), "built-ins {} must differ", BUILTIN[i]);
        }
        assert_eq!(snaps[0], Snapshot::capture(&PanelSet::default(), &Layout::default()));
        // Different visibility as well as different arrangement.
        let shown = |n: &str, id: &str| w.snapshot_of(n).unwrap().restore().0.is_shown(id);
        assert!(shown("Default", "spectral") && !shown("Recording", "spectral") && !shown("Repair", "waveform"));
        let zone = |n: &str, id: &str| w.snapshot_of(n).unwrap().restore().1.zone_of(id);
        assert_eq!(
            (zone("Default", "history"), zone("Editing", "history"), zone("Repair", "history")),
            (Some(DockSide::Right), Some(DockSide::Left), Some(DockSide::Bottom))
        );
        assert_eq!(zone("Default", "selection"), Some(DockSide::Bottom));
        for b in BUILTIN {
            assert!(is_builtin(&b.to_uppercase()));
            assert!(w.save(&b.to_lowercase(), &snaps[0]).is_err(), "{b} can't be replaced");
            assert!(w.delete(b).is_err(), "{b} can't be deleted");
            // Reset gives back the built-in state; a change shows as modified.
            assert_eq!(w.reset(Some(b)).unwrap(), w.snapshot_of(b).unwrap());
            assert!(!w.is_modified(&w.snapshot_of(b).unwrap()));
            assert_eq!(w.current(), b);
        }
        assert!(w.is_modified(&moved("history", DockSide::Left)));
        // Every built-in keeps the Editor shown.
        assert!(snaps.iter().all(|s| s.restore().0.is_shown("editor")));
        assert!(!dir.exists(), "built-in workspaces write nothing");
    }

    #[test]
    fn names_are_checked_and_tidied() {
        assert_eq!(clean_name("  Mix   down ").unwrap(), "Mix down");
        assert!(clean_name("").is_err() && clean_name("   ").is_err());
        assert!(clean_name("a/b").is_err() && clean_name("..").is_err() && clean_name("x=y").is_err());
        assert!(clean_name(&"x".repeat(41)).is_err());
        assert!(clean_name(&"x".repeat(40)).is_ok());
    }

    #[test]
    fn save_switch_reset_delete_and_persistence() {
        let dir = tmp("cycle");
        let mut w = Workspaces::new(Some(dir.clone()), "");
        let a = moved("history", DockSide::Left);
        let b = moved("history", DockSide::Bottom);
        assert_eq!(w.save("Edit", &a).unwrap(), "Edit");
        assert_eq!(w.save("Mix down", &b).unwrap(), "Mix down");
        assert_eq!(w.current(), "Mix down");
        assert_eq!(w.names(), vec!["Default", "Editing", "Recording", "Repair", "Edit", "Mix down"]);
        assert!(!w.is_modified(&b) && w.is_modified(&a));

        assert_eq!(w.switch("edit").unwrap(), a);
        assert_eq!(w.current(), "Edit");
        let (set, layout) = a.restore();
        assert_eq!(layout.zone_of("history"), Some(DockSide::Left));
        assert!(!set.is_shown("selection"));
        // A change, then reset: back to the saved state.
        assert!(w.is_modified(&Snapshot::builtin()));
        assert_eq!(w.reset(None).unwrap(), a);
        assert_eq!(w.reset(Some("Mix down")).unwrap(), b);
        assert_eq!(w.current(), "Mix down");
        // Delete: not the current one, not an unknown one.
        assert!(w.delete("Mix down").is_err());
        assert!(w.delete("nothing").is_err());
        w.delete("edit").unwrap();
        assert_eq!(w.names(), vec!["Default", "Editing", "Recording", "Repair", "Mix down"]);
        assert!(w.switch("edit").is_err());
        // A fresh process (a restart) sees the same.
        let w2 = Workspaces::new(Some(dir.clone()), "Mix down");
        assert_eq!(w2.current(), "Mix down");
        assert_eq!(w2.snapshot_of("mix down").unwrap(), b);
        assert_eq!(Workspaces::new(Some(dir.clone()), "edit").current(), DEFAULT);
        // Saving again under the same name replaces it.
        let mut w3 = w2.clone();
        w3.save("MIX DOWN", &a).unwrap();
        assert_eq!(w3.names(), vec!["Default", "Editing", "Recording", "Repair", "MIX DOWN"]);
        assert_eq!(w3.snapshot_of("mix down").unwrap(), a);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn without_a_folder_nothing_is_saved() {
        let mut w = Workspaces::new(None, "x");
        assert!(w.save("A", &Snapshot::builtin()).unwrap_err().contains("no settings folder"));
        assert_eq!(w.names(), BUILTIN.to_vec());
    }

    #[test]
    fn a_damaged_file_is_not_a_workspace() {
        let dir = tmp("damaged");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("junk.workspace"), "\u{0}\u{1} not a workspace").unwrap();
        std::fs::write(
            dir.join("odd.workspace"),
            "name = Odd\npanel_layout = left:history,moon:x,history\nzone_sizes = ,,\nhidden_panels = ghost,selection\n",
        )
        .unwrap();
        let w = Workspaces::new(Some(dir.clone()), "odd");
        assert_eq!(w.names(), vec!["Default", "Editing", "Recording", "Repair", "Odd"]);
        let (set, layout) = w.snapshot_of("odd").unwrap().restore();
        assert!(!set.is_shown("selection") && layout.zone_of("history") == Some(DockSide::Left));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn report_has_current_modified_and_list() {
        let dir = tmp("report");
        let mut w = Workspaces::new(Some(dir.clone()), "");
        w.save("Two words", &moved("history", DockSide::Left)).unwrap();
        assert_eq!(w.report(&moved("history", DockSide::Left)), "current=Two_words modified=0 workspaces=Default|Editing|Recording|Repair|Two_words");
        assert!(w.report(&Snapshot::builtin()).contains("modified=1"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
