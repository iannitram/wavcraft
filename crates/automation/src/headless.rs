//! The headless backend: `wavcraft-core` in this process, with no window. Each request is
//! answered by the same core calls the window makes (`io::open`, `Editor::run`,
//! `Editor::run_effect`, and the window's save calls: `save_wav_with`, `save_aiff_kind`, `save_vorbis`), so a script's edits come out the same either way.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use wavcraft_core::DocState;
use wavcraft_core::actions::{self, Editor};
use wavcraft_core::doc::edit::Crossfades;
use wavcraft_core::doc::history::History;
use wavcraft_core::effects;
use wavcraft_core::io::{self, OpenOptions};
use wavcraft_core::save::{self, AiffKind, Container, SampleType, SaveFormat, SaveOptions, VORBIS_DEFAULT_QUALITY};

use crate::{Backend, Request, SaveArgs};

/// The open document and its editing state.
struct Session {
    editor: Editor,
    selection: Range<u64>,
    /// The file a `SAVE` with no path writes: the one opened, then the last one saved to.
    path: PathBuf,
    /// "Include markers and other metadata" for this document (`SAVE meta=`, sticky as in the
    /// window; each document starts with it on, the settings' default).
    include_meta: bool,
}

/// Runs requests on `wavcraft-core` directly. `sessions_root` is where the document's session
/// store is made (the window uses the system cache; a script or test passes its own folder).
pub struct Headless {
    sessions_root: PathBuf,
    crossfades: Crossfades,
    session: Option<Session>,
    /// The Ogg Vorbis quality a save uses (`SAVE quality=` sets it for later saves too, as the
    /// window's does).
    vorbis_quality: f32,
}

impl Headless {
    /// With the window's default edit crossfades (what Preferences > Data starts with).
    pub fn new(sessions_root: impl Into<PathBuf>) -> Headless {
        Headless::with_crossfades(sessions_root, Crossfades::default())
    }

    /// With other edit crossfades; `Crossfades::NONE` makes every splice exact.
    pub fn with_crossfades(sessions_root: impl Into<PathBuf>, crossfades: Crossfades) -> Headless {
        Headless { sessions_root: sessions_root.into(), crossfades, session: None, vorbis_quality: VORBIS_DEFAULT_QUALITY }
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
        self.session = Some(Session { editor, selection: 0..0, path: path.to_path_buf(), include_meta: true });
        Ok(())
    }

    /// `SAVE`, as the window's `save_with` does it with no dialog: the format from the
    /// extension (Wave64, CAF and AU too; none is a WAV), the source's sample type unless `bits=`/`float=` name one (a format that cannot
    /// hold a named type refuses; otherwise the nearest it holds), the AIFF kind for this save
    /// only, the Vorbis quality and the metadata switch kept for later saves. With no path, the
    /// file that was opened (or last saved to), when WavCraft can write its format.
    fn save(&mut self, to: Option<&Path>, args: &SaveArgs) -> Result<(), String> {
        // Kept before the save is tried, as the window keeps them.
        if let Some(q) = args.quality {
            self.vorbis_quality = q;
        }
        let quality = self.vorbis_quality;
        let s = self.session.as_mut().ok_or_else(|| "no file is open".to_string())?;
        if let Some(m) = args.meta {
            s.include_meta = m;
        }
        let doc = s.editor.history.current().clone();
        let path = match to {
            Some(p) => p.to_path_buf(),
            None => {
                let own = SaveFormat::from_path(&s.path).filter(|f| SaveFormat::from_file_format(doc.source().format) == Some(*f));
                if own.is_none() {
                    return Err(format!("{} is not a format WavCraft saves in place: SAVE <path>", s.path.display()));
                }
                s.path.clone()
            }
        };
        // As in the window: the extension picks the format; Wave64, CAF and AU are a container of
        // their own (`format` is WAV there and unused); a name without an extension is a WAV.
        let (path, format, container) = if let Some(f) = SaveFormat::from_path(&path) {
            (path, f, None)
        } else if let Some(c) = Container::from_path(&path).filter(|c| *c != Container::Wav) {
            (path, SaveFormat::Wav, Some(c))
        } else if path.extension().is_none() {
            (path.with_extension("wav"), SaveFormat::Wav, None)
        } else {
            return Err(format!("no save format for {}", path.display()));
        };
        let wanted = args.ty.unwrap_or_else(|| SampleType::for_source(doc.source()));
        if !format.accepts(wanted) && args.ty.is_some() {
            return Err(format!("{} cannot hold {} samples: use 16 or 24 bit", format.name(), wanted.describe()));
        }
        let ty = format.fit(wanted);
        let done = match (container, format) {
            (Some(c), _) => save::save_as(&doc, &path, c, ty, &mut |_| {}),
            (None, SaveFormat::Vorbis) => save::save_vorbis(&doc, &path, quality, &mut |_| {}),
            (None, SaveFormat::Wav) => {
                let opts = SaveOptions { include_metadata: s.include_meta, ..SaveOptions::default() };
                save::save_wav_with(&doc, &path, ty, &opts, &mut |_| {})
            }
            (None, SaveFormat::Aiff | SaveFormat::Flac) => {
                save::save_aiff_kind(&doc, &path, format, ty, args.aiff.unwrap_or(AiffKind::Aiff), &mut |_| {})
            }
        };
        done.map_err(|e| e.to_string())?;
        s.path = path;
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

/// The one-shot effect (`actions::ONE_SHOT`: reverse, the fades) named `name` without its
/// `effect.` prefix, as the window's `EFFECT` verb finds it.
fn one_shot_effect(name: &str) -> Option<actions::ActionId> {
    actions::ONE_SHOT.iter().find(|a| a.name.strip_prefix("effect.") == Some(name)).map(|a| a.id)
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
                let Some(index) = effects::all().iter().position(|e| e.id() == name) else {
                    // A one-shot (reverse, the fades) runs as `ACTION effect.<name>` does, as the
                    // window's `EFFECT` does; its settings come from elsewhere (`PARAM`), not here.
                    let id = one_shot_effect(name).ok_or_else(|| format!("no effect named {name:?}"))?;
                    if !args.trim().is_empty() {
                        return Err(format!("{name} takes no settings here: set them with PARAM"));
                    }
                    let s = self.session()?;
                    let sel = target(s, true);
                    let outcome = s.editor.run(id, sel).map_err(|e| e.to_string())?;
                    s.selection = outcome.selection.clone();
                    return Ok(format!("selection={} changed={}", span(&s.selection), u8::from(outcome.doc_changed)));
                };
                let effect = effects::all()[index];
                let params = effects::parse_params(effect, args)?;
                let s = self.session()?;
                let sel = target(s, true);
                let outcome = s.editor.run_effect(index as u16, params, sel).map_err(|e| e.to_string())?;
                s.selection = outcome.selection.clone();
                Ok(format!("selection={} changed={}", span(&s.selection), u8::from(outcome.doc_changed)))
            }
            Request::Save { path, args } => {
                self.save(path.as_deref(), args)?;
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
