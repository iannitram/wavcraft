//! Export presets (f87): a named choice of file format, sample type and AIFF kind, kept in
//! `export-presets.txt` next to the settings file (`<config dir>/wavcraft`). One preset a line,
//! tab-separated: `name <TAB> format <TAB> type <TAB> aiff`, `#` for comments. The file is
//! replaced whole through a temporary file, so a crash never leaves half of it.

use std::io;
use std::path::{Path, PathBuf};

use crate::save::{AiffKind, SampleType, SaveFormat};

/// The longest preset name, in characters.
pub const MAX_NAME: usize = 64;

/// What an export is asked to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportChoice {
    pub format: SaveFormat,
    pub ty: SampleType,
    pub aiff: AiffKind,
}

impl Default for ExportChoice {
    fn default() -> Self {
        ExportChoice { format: SaveFormat::Wav, ty: SampleType::Int24, aiff: AiffKind::Aiff }
    }
}

impl ExportChoice {
    /// The same choice with a sample type the format can hold.
    pub fn fitted(self) -> ExportChoice {
        ExportChoice { ty: self.format.fit(self.ty), ..self }
    }

    /// `format=aiff type=24 aiff=sowt`, for the control channel.
    pub fn describe(&self) -> String {
        format!("format={} type={} aiff={}", self.format.key(), self.ty.key(), self.aiff.key())
    }
}

/// A named choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPreset {
    pub name: String,
    pub choice: ExportChoice,
}

/// The saved presets, in the order they were first made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportPresets {
    pub presets: Vec<ExportPreset>,
}

/// A name a preset can have: trimmed, 1 to [`MAX_NAME`] characters, no control characters.
pub fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a preset needs a name".to_string());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("a preset name is at most {MAX_NAME} characters"));
    }
    if name.chars().any(char::is_control) {
        return Err("a preset name cannot hold tabs or line breaks".to_string());
    }
    Ok(name.to_string())
}

impl ExportPresets {
    /// `<config dir>/wavcraft/export-presets.txt`, when the system has a config dir.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("wavcraft").join("export-presets.txt"))
    }

    /// Read `path`; a missing or unreadable file gives no presets.
    pub fn load(path: &Path) -> ExportPresets {
        match std::fs::read_to_string(path) {
            Ok(text) => ExportPresets::parse(&text),
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!("could not read the export presets {}: {e}", path.display());
                }
                ExportPresets::default()
            }
        }
    }

    /// Presets from the file's text; lines that don't parse are skipped.
    pub fn parse(text: &str) -> ExportPresets {
        let mut out = ExportPresets::default();
        for line in text.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if let [name, format, ty, aiff] = f[..]
                && let (Ok(name), Some(format), Some(ty), Some(aiff)) =
                    (clean_name(name), SaveFormat::parse(format), SampleType::parse(ty), AiffKind::parse(aiff))
            {
                let _ = out.set(&name, ExportChoice { format, ty, aiff });
            }
        }
        out
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# WavCraft export presets: name, format, sample type, AIFF kind (tab-separated).\n");
        for p in &self.presets {
            out.push_str(&format!("{}\t{}\t{}\t{}\n", p.name, p.choice.format.key(), p.choice.ty.key(), p.choice.aiff.key()));
        }
        out
    }

    /// Write to `path` (its folder made if missing), replacing the file whole.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("txt.new");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, path)
    }

    /// Add a preset, or replace the one with this name.
    pub fn set(&mut self, name: &str, choice: ExportChoice) -> Result<(), String> {
        let name = clean_name(name)?;
        let choice = choice.fitted();
        match self.presets.iter_mut().find(|p| p.name == name) {
            Some(p) => p.choice = choice,
            None => self.presets.push(ExportPreset { name, choice }),
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<ExportChoice> {
        let name = name.trim();
        self.presets.iter().find(|p| p.name == name).map(|p| p.choice)
    }

    /// Remove a preset; whether there was one.
    pub fn remove(&mut self, name: &str) -> bool {
        let name = name.trim();
        let before = self.presets.len();
        self.presets.retain(|p| p.name != name);
        self.presets.len() != before
    }
}
