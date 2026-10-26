//! Named presets (f31): the settings of one effect, or of the whole Effects Rack, saved as small
//! text files in WavCraft's config folder and read back later.
//!
//! Layout under the presets folder: `<effect id>/<name>.preset` for an effect (`key=value` per
//! line) and `rack/<name>.preset` for a rack. The names are the user's own; WavCraft ships none.

use std::fs;
use std::path::{Path, PathBuf};

use crate::effects;
use crate::rack::{RackConfig, SlotConfig};

/// The folder name used for rack presets (no effect has this id).
pub const RACK: &str = "rack";
const EXT: &str = "preset";

/// A presets folder.
#[derive(Debug, Clone)]
pub struct Presets {
    root: PathBuf,
}

impl Presets {
    /// At `root`; nothing is created until a preset is saved.
    pub fn at(root: impl Into<PathBuf>) -> Presets {
        Presets { root: root.into() }
    }

    /// The default place: `presets` in WavCraft's config folder (next to `settings.txt`).
    pub fn default_location() -> Option<Presets> {
        dirs::config_dir().map(|d| Presets::at(d.join("wavcraft").join("presets")))
    }

    fn folder(&self, kind: &str) -> Result<PathBuf, String> {
        if kind != RACK && effects::find(kind).is_none() {
            return Err(format!("no effect named {kind:?}"));
        }
        Ok(self.root.join(kind))
    }

    fn file(&self, kind: &str, name: &str) -> Result<PathBuf, String> {
        check_name(name)?;
        Ok(self.folder(kind)?.join(format!("{name}.{EXT}")))
    }

    /// Save an effect's settings (checked against its ranges) under `name`, replacing any.
    pub fn save_effect(&self, effect: &str, name: &str, params: &[f64]) -> Result<(), String> {
        let e = effects::find(effect).ok_or_else(|| format!("no effect named {effect:?}"))?;
        effects::validate(e, params)?;
        write(&self.file(effect, name)?, &effect_text(e.parameters().iter().map(|p| p.id), params))
    }

    /// The settings saved under `name`, checked again on the way back.
    pub fn load_effect(&self, effect: &str, name: &str) -> Result<Vec<f64>, String> {
        let e = effects::find(effect).ok_or_else(|| format!("no effect named {effect:?}"))?;
        let text = read(&self.file(effect, name)?, name)?;
        effects::parse_params(e, &text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "))
    }

    /// Save the rack's settings under `name`.
    pub fn save_rack(&self, name: &str, rack: &RackConfig) -> Result<(), String> {
        let mut text = format!(
            "power={}\ninput_db={}\noutput_db={}\nmix={}\n",
            if rack.power { "on" } else { "off" },
            rack.input_db,
            rack.output_db,
            rack.mix_pct
        );
        for s in &rack.slots {
            match s.effect.as_deref().and_then(effects::find) {
                None => text.push_str("slot=- off\n"),
                Some(e) => {
                    text.push_str(&format!("slot={} {}", e.id(), if s.enabled { "on" } else { "off" }));
                    for (p, v) in e.parameters().iter().zip(&s.params) {
                        text.push_str(&format!(" {}={v}", p.id));
                    }
                    text.push('\n');
                }
            }
        }
        write(&self.file(RACK, name)?, &text)
    }

    /// The rack saved under `name`.
    pub fn load_rack(&self, name: &str) -> Result<RackConfig, String> {
        let text = read(&self.file(RACK, name)?, name)?;
        let mut rack = RackConfig::new();
        rack.slots.clear();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (key, value) = line.split_once('=').ok_or_else(|| format!("preset {name:?}: cannot read {line:?}"))?;
            let number = |v: &str| v.trim().parse::<f64>().map_err(|_| format!("preset {name:?}: {key} = {v:?} is not a number"));
            match key {
                "power" => rack.power = value.trim() == "on",
                "input_db" => rack.set_input_db(number(value)?)?,
                "output_db" => rack.set_output_db(number(value)?)?,
                "mix" => rack.set_mix(number(value)?)?,
                "slot" => rack.slots.push(parse_slot(value).map_err(|e| format!("preset {name:?}: {e}"))?),
                _ => return Err(format!("preset {name:?}: unknown line {line:?}")),
            }
        }
        if rack.slots.is_empty() || rack.slots.len() > 16 {
            return Err(format!("preset {name:?} has {} slots", rack.slots.len()));
        }
        Ok(rack)
    }

    /// Remove a preset (an effect id or [`RACK`] for `kind`).
    pub fn delete(&self, kind: &str, name: &str) -> Result<(), String> {
        let path = self.file(kind, name)?;
        fs::remove_file(&path).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { format!("no preset named {name:?}") } else { e.to_string() })
    }

    /// The names saved for `kind` (an effect id or [`RACK`]), sorted.
    pub fn list(&self, kind: &str) -> Result<Vec<String>, String> {
        let dir = self.folder(kind)?;
        let Ok(rd) = fs::read_dir(&dir) else { return Ok(Vec::new()) };
        let mut names: Vec<String> = rd
            .filter_map(Result::ok)
            .filter_map(|f| {
                let p = f.path();
                (p.extension().is_some_and(|x| x == EXT)).then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned())).flatten()
            })
            .collect();
        names.sort();
        Ok(names)
    }
}

fn effect_text<'a>(ids: impl Iterator<Item = &'a str>, params: &[f64]) -> String {
    ids.zip(params).map(|(k, v)| format!("{k}={v}\n")).collect()
}

fn parse_slot(value: &str) -> Result<SlotConfig, String> {
    let mut words = value.split_whitespace();
    let (Some(id), Some(power)) = (words.next(), words.next()) else { return Err(format!("slot {value:?}: write <effect|-> <on|off> ...")) };
    let enabled = power == "on";
    if id == "-" {
        return Ok(SlotConfig::empty());
    }
    let e = effects::find(id).ok_or_else(|| format!("no effect named {id:?}"))?;
    let rest: Vec<&str> = words.collect();
    let params = effects::parse_params(e, &rest.join(" "))?;
    Ok(SlotConfig { effect: Some(id.to_string()), params, enabled })
}

/// A preset name: 1 to 64 letters, digits, spaces, `-`, `_` or `.`, not starting with `.` or a space.
fn check_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.chars().count() <= 64
        && !name.starts_with(['.', ' '])
        && !name.ends_with(' ')
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
    if ok { Ok(()) } else { Err(format!("{name:?} is not a usable preset name (1 to 64 letters, digits, spaces, - _ .)")) }
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot make the presets folder: {e}"))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text).and_then(|()| fs::rename(&tmp, path)).map_err(|e| format!("cannot save the preset: {e}"))
}

fn read(path: &Path, name: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { format!("no preset named {name:?}") } else { e.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wavcraft-presets-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn effect_presets_save_load_list_delete() {
        let dir = temp("effect");
        let p = Presets::at(&dir);
        assert_eq!(p.list("gain").unwrap(), Vec::<String>::new());
        p.save_effect("gain", "quiet one", &[-12.5]).unwrap();
        p.save_effect("gain", "a", &[3.0]).unwrap();
        assert_eq!(p.list("gain").unwrap(), vec!["a".to_string(), "quiet one".to_string()]);
        assert_eq!(p.load_effect("gain", "quiet one").unwrap(), vec![-12.5]);
        p.save_effect("gain", "a", &[4.0]).unwrap();
        assert_eq!(p.load_effect("gain", "a").unwrap(), vec![4.0], "saving again replaces");
        p.delete("gain", "a").unwrap();
        assert!(p.load_effect("gain", "a").unwrap_err().contains("no preset"));
        assert!(p.delete("gain", "a").is_err());
        assert!(p.save_effect("gain", "x", &[999.0]).is_err(), "out of range");
        assert!(p.save_effect("nonesuch", "x", &[]).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_cannot_escape_the_folder() {
        let dir = temp("names");
        let p = Presets::at(&dir);
        for bad in ["", "../x", "a/b", "a\\b", ".hidden", " lead", "x".repeat(65).as_str(), "a:b"] {
            assert!(p.save_effect("gain", bad, &[0.0]).is_err(), "{bad:?}");
        }
        assert!(!dir.exists(), "nothing was written");
    }

    #[test]
    fn rack_presets_round_trip() {
        let dir = temp("rack");
        let p = Presets::at(&dir);
        let mut r = RackConfig::new();
        r.set_slot(0, "invert", None).unwrap();
        r.set_slot(2, "gain", Some(vec![-6.5])).unwrap();
        r.set_slot_power(2, false).unwrap();
        r.set_input_db(-3.0).unwrap();
        r.set_mix(40.0).unwrap();
        p.save_rack("mine", &r).unwrap();
        assert_eq!(p.load_rack("mine").unwrap(), r);
        assert_eq!(p.list(RACK).unwrap(), vec!["mine".to_string()]);
        p.delete(RACK, "mine").unwrap();
        assert!(p.load_rack("mine").is_err());
        fs::create_dir_all(dir.join(RACK)).unwrap();
        fs::write(dir.join(RACK).join("broken.preset"), "slot=gain on gain_db=900\n").unwrap();
        assert!(p.load_rack("broken").is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
