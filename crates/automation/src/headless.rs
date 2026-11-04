//! The headless backend: `wavcraft-core` in this process, with no window. Each request is
//! answered by the same core calls the window makes (`io::open`, `Editor::run`,
//! `Editor::run_effect`, `save::save`), so a script's edits come out the same either way.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::DocState;
use wavcraft_core::actions::{self, Editor};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::effects;
use wavcraft_core::io::{self, OpenOptions};
use wavcraft_core::save::{self, SampleType, SaveFormat};

use crate::{Backend, Request};

/// The open document and its editing state.
struct Session {
    editor: Editor,
    selection: Range<u64>,
}

/// Runs requests on `wavcraft-core` directly. `sessions_root` is where the document's session
/// store is made (the window uses the system cache; a script or test passes its own folder).
pub struct Headless {
    sessions_root: PathBuf,
    crossfades: Crossfades,
    session: Option<Session>,
}

impl Headless {
    /// With the window's default edit crossfades (what Preferences > Data starts with).
    pub fn new(sessions_root: impl Into<PathBuf>) -> Headless {
        Headless::with_crossfades(sessions_root, Crossfades::default())
    }

    /// With other edit crossfades; `Crossfades::NONE` makes every splice exact.
    pub fn with_crossfades(sessions_root: impl Into<PathBuf>, crossfades: Crossfades) -> Headless {
        Headless { sessions_root: sessions_root.into(), crossfades, session: None }
    }

    /// The open document's state, if one is open.
    pub fn document(&self) -> Option<&DocState> {
        self.session.as_ref().map(|s| &**s.editor.history.current())
    }

    fn open(&mut self, path: &Path) -> Result<(), String> {
        let opts = OpenOptions { sessions_root: Some(self.sessions_root.clone()), ..OpenOptions::default() };
        let doc = io::open(path, &opts, &mut |_| {}, &AtomicBool::new(false)).map_err(|e| e.to_string())?;
        let cap = History::default_cap(&self.sessions_root);
        let mut editor = Editor::new(History::new(doc, cap));
        editor.crossfades = self.crossfades;
        self.session = Some(Session { editor, selection: 0..0 });
        Ok(())
    }

    fn session(&mut self) -> Result<&mut Session, String> {
        self.session.as_mut().ok_or_else(|| "no file is open".to_string())
    }
}

/// `a-b`, as `STATE` and the edit replies write a selection.
fn span(r: &Range<u64>) -> String {
    format!("{}-{}", r.start, r.end)
}

/// The range a request acts on: the selection, or for an effect with no selection the whole
/// file (the window does the same: `EFFECT` runs "on the selection, the whole file when none").
fn target(s: &Session, whole_when_empty: bool) -> Range<u64> {
    if whole_when_empty && s.selection.is_empty() { 0..s.editor.history.current().len_frames() } else { s.selection.clone() }
}

impl Backend for Headless {
    fn execute(&mut self, req: &Request) -> Result<String, String> {
        match req {
            Request::Open(path) => {
                self.open(path)?;
                Ok(String::new())
            }
            Request::Select { start, end } => {
                let s = self.session()?;
                let len = s.editor.history.current().len_frames();
                let (a, b) = ((*start).min(len), (*end).min(len));
                s.selection = a.min(b)..a.max(b);
                Ok(format!("selection={}", span(&s.selection)))
            }
            Request::Action(name) => {
                let id = actions::id_by_name(name).ok_or_else(|| format!("no action named {name:?}"))?;
                let whole = name.starts_with("effect.");
                let s = self.session()?;
                let sel = target(s, whole);
                let outcome = s.editor.run(id, sel).map_err(|e| e.to_string())?;
                s.selection = outcome.selection.clone();
                Ok(format!("selection={} changed={}", span(&s.selection), u8::from(outcome.doc_changed)))
            }
            Request::Effect { name, args } => {
                let index = effects::all().iter().position(|e| e.id() == name).ok_or_else(|| format!("no effect named {name:?}"))?;
                let effect = effects::all()[index];
                let params = effects::parse_params(effect, args)?;
                let s = self.session()?;
                let sel = target(s, true);
                let outcome = s.editor.run_effect(index as u16, params, sel).map_err(|e| e.to_string())?;
                s.selection = outcome.selection.clone();
                Ok(format!("selection={} changed={}", span(&s.selection), u8::from(outcome.doc_changed)))
            }
            Request::Save(path) => {
                let format = SaveFormat::from_path(path).ok_or_else(|| format!("no save format for {}", path.display()))?;
                let s = self.session()?;
                let doc = s.editor.history.current().clone();
                let ty = format.fit(SampleType::for_source(doc.source()));
                save::save(&doc, path, format, ty, &mut |_| {}).map_err(|e| e.to_string())?;
                Ok(String::new())
            }
            Request::State => {
                let s = self.session()?;
                let doc = s.editor.history.current();
                Ok(format!(
                    "frames={} channels={} rate={} selection={} busy=0",
                    doc.len_frames(),
                    doc.channel_count(),
                    doc.sample_rate(),
                    span(&s.selection)
                ))
            }
        }
    }
}
