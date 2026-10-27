//! Machine-specific audio hardware defaults (f97): the audio hardware preferences (host, devices,
//! buffer size, channel maps) are kept per *device setup*, keyed by the set of device names the
//! system offers. Moving between setups (with and without an external interface) restores the
//! settings last used on that setup. A stored device that is not on offer falls back to the
//! system default. Device lists are passed in, so tests never touch real hardware.

use std::io;
use std::path::{Path, PathBuf};

use crate::chanmap::ChannelMap;
use crate::settings::Settings;

/// The audio hardware part of the settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hardware {
    pub audio_host: Option<String>,
    pub output_device: Option<String>,
    pub input_device: Option<String>,
    pub buffer_frames: Option<u32>,
    pub output_map: ChannelMap,
    pub input_map: ChannelMap,
}

const HW_KEYS: [&str; 6] = ["audio_host", "output_device", "input_device", "buffer_frames", "output_map", "input_map"];

impl Hardware {
    /// The hardware settings now in `s`.
    pub fn of(s: &Settings) -> Hardware {
        Hardware {
            audio_host: s.audio_host.clone(),
            output_device: s.output_device.clone(),
            input_device: s.input_device.clone(),
            buffer_frames: s.buffer_frames,
            output_map: s.output_map.clone(),
            input_map: s.input_map.clone(),
        }
    }

    /// Put these into `s`.
    pub fn apply(&self, s: &mut Settings) {
        s.audio_host = self.audio_host.clone();
        s.output_device = self.output_device.clone();
        s.input_device = self.input_device.clone();
        s.buffer_frames = self.buffer_frames;
        s.output_map = self.output_map.clone();
        s.input_map = self.input_map.clone();
    }

    /// Drop a device that the setup does not offer (the system default is used instead).
    pub fn with_fallback(mut self, outputs: &[String], inputs: &[String]) -> (Hardware, bool) {
        let mut fell_back = false;
        if self.output_device.as_ref().is_some_and(|d| !outputs.contains(d)) {
            self.output_device = None;
            fell_back = true;
        }
        if self.input_device.as_ref().is_some_and(|d| !inputs.contains(d)) {
            self.input_device = None;
            fell_back = true;
        }
        (self, fell_back)
    }
}

/// The key of a device setup: the sorted, de-duplicated device names (outputs and inputs), as a
/// short hex hash (FNV-1a 64), so the order the system lists them in does not matter.
pub fn setup_key(outputs: &[String], inputs: &[String]) -> String {
    let mut names: Vec<&str> = outputs.iter().chain(inputs.iter()).map(String::as_str).collect();
    names.sort_unstable();
    names.dedup();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for n in names {
        for b in n.bytes().chain(std::iter::once(0)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// A readable label for a setup (for the file and `AUDIOPROFILE list`).
pub fn setup_label(outputs: &[String], inputs: &[String]) -> String {
    let mut names: Vec<String> = outputs.iter().chain(inputs.iter()).map(|n| n.replace(['\n', '\r', '|'], " ")).collect();
    names.sort();
    names.dedup();
    names.join(" | ")
}

/// One stored setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub key: String,
    pub label: String,
    pub hw: Hardware,
}

/// What [`ProfileStore::activate`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The setup was already the active one: nothing changed.
    Unchanged,
    /// No profile was stored yet at all: the current settings became this setup's profile.
    Adopted,
    /// A stored profile was restored.
    Restored,
    /// A stored profile was restored but a device it names is not on offer: system default used.
    RestoredWithFallback,
    /// A setup not seen before (others exist): it starts from the system defaults.
    NewSetup,
}

/// All stored setups, and which one is active.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileStore {
    pub profiles: Vec<Profile>,
    pub active: Option<String>,
}

impl ProfileStore {
    pub fn parse(text: &str) -> ProfileStore {
        let mut store = ProfileStore::default();
        let mut cur: Option<(Profile, Settings)> = None;
        let finish = |cur: &mut Option<(Profile, Settings)>, store: &mut ProfileStore| {
            if let Some((mut p, s)) = cur.take() {
                p.hw = Hardware::of(&s);
                store.profiles.push(p);
            }
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(key) = line.strip_prefix("[setup ").and_then(|l| l.strip_suffix(']')) {
                finish(&mut cur, &mut store);
                cur = Some((Profile { key: key.trim().to_string(), label: String::new(), hw: Hardware::default() }, Settings::default()));
            } else if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                match (&mut cur, k) {
                    (None, "active") => store.active = (!v.is_empty()).then(|| v.to_string()),
                    (Some((p, _)), "devices") => p.label = v.to_string(),
                    (Some((_, s)), k) if HW_KEYS.contains(&k) => {
                        let _ = s.set(k, v);
                    }
                    _ => {}
                }
            }
        }
        finish(&mut cur, &mut store);
        store
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# WavCraft audio hardware, kept per device setup (the set of devices on offer). One section per setup.\n");
        out.push_str(&format!("active = {}\n", self.active.as_deref().unwrap_or_default()));
        for p in &self.profiles {
            let mut s = Settings::default();
            p.hw.apply(&mut s);
            out.push_str(&format!("\n[setup {}]\ndevices = {}\n", p.key, p.label));
            for k in HW_KEYS {
                out.push_str(&format!(
                    "{k} = {}\n",
                    if s.get(k).is_ok_and(|v| v == "System default" || v == "Device default") { String::new() } else { s.get(k).unwrap_or_default() }
                ));
            }
        }
        out
    }

    pub fn default_path(settings_path: &Path) -> PathBuf {
        settings_path.with_file_name("audio-profiles.txt")
    }

    pub fn load(path: &Path) -> ProfileStore {
        std::fs::read_to_string(path).map(|t| ProfileStore::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("txt.new");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, path)
    }

    /// Remember `hw` as the settings of the active setup (call after a hardware preference changes).
    pub fn remember(&mut self, hw: &Hardware) {
        if let Some(key) = self.active.clone()
            && let Some(p) = self.profiles.iter_mut().find(|p| p.key == key)
        {
            p.hw = hw.clone();
        }
    }

    /// The device setup is now `outputs`/`inputs`; `current` is what the settings hold. Saves
    /// `current` into the setup being left, then returns the hardware settings to use now.
    pub fn activate(&mut self, outputs: &[String], inputs: &[String], current: &Hardware) -> (Outcome, Hardware) {
        let key = setup_key(outputs, inputs);
        if self.active.as_deref() == Some(&key) {
            return (Outcome::Unchanged, current.clone());
        }
        self.remember(current);
        let had_any = !self.profiles.is_empty();
        self.active = Some(key.clone());
        match self.profiles.iter().find(|p| p.key == key) {
            Some(p) => {
                let (hw, fell) = p.hw.clone().with_fallback(outputs, inputs);
                (if fell { Outcome::RestoredWithFallback } else { Outcome::Restored }, hw)
            }
            None => {
                let (outcome, hw) = if had_any { (Outcome::NewSetup, Hardware::default()) } else { (Outcome::Adopted, current.clone()) };
                self.profiles.push(Profile { key, label: setup_label(outputs, inputs), hw: hw.clone() });
                (outcome, hw)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn hw(out: Option<&str>, buf: Option<u32>, map: &str) -> Hardware {
        Hardware { output_device: out.map(String::from), buffer_frames: buf, output_map: ChannelMap::parse(map).unwrap(), ..Hardware::default() }
    }

    #[test]
    fn the_key_ignores_order_and_duplicates_but_not_membership() {
        let a = setup_key(&v(&["Speakers", "USB"]), &v(&["Mic"]));
        assert_eq!(a, setup_key(&v(&["Mic", "USB"]), &v(&["Speakers", "Mic"])));
        assert_ne!(a, setup_key(&v(&["Speakers"]), &v(&["Mic"])));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn the_first_setup_adopts_the_current_settings() {
        let mut s = ProfileStore::default();
        let cur = hw(Some("Speakers"), Some(256), "1:2");
        let (o, got) = s.activate(&v(&["Speakers"]), &[], &cur);
        assert_eq!(o, Outcome::Adopted);
        assert_eq!(got, cur);
        assert_eq!(s.profiles.len(), 1);
    }

    #[test]
    fn moving_between_setups_restores_each_ones_settings() {
        let mut s = ProfileStore::default();
        let (internal, both) = (v(&["Speakers"]), v(&["Speakers", "USB"]));
        let a = hw(Some("Speakers"), Some(512), "");
        s.activate(&internal, &[], &a);
        // Plug in the interface: a new setup starts from the defaults; the user configures it.
        let (o, got) = s.activate(&both, &[], &a);
        assert_eq!((o, got), (Outcome::NewSetup, Hardware::default()));
        let b = hw(Some("USB"), Some(64), "1:3,2:4");
        // Unplug: the internal settings come back, the interface's were saved.
        let (o, got) = s.activate(&internal, &[], &b);
        assert_eq!((o, got), (Outcome::Restored, a.clone()));
        let (o, got) = s.activate(&both, &[], &a);
        assert_eq!((o, got), (Outcome::Restored, b));
        assert_eq!(s.activate(&both, &[], &hw(Some("USB"), Some(64), "1:3,2:4")).0, Outcome::Unchanged);
    }

    #[test]
    fn a_stored_device_that_is_missing_falls_back_to_the_system_default() {
        let mut s = ProfileStore::default();
        let key_set = v(&["Speakers"]);
        s.activate(&key_set, &[], &Hardware::default());
        s.remember(&hw(Some("Ghost"), Some(128), ""));
        s.active = None;
        let (o, got) = s.activate(&key_set, &[], &Hardware::default());
        assert_eq!(o, Outcome::RestoredWithFallback);
        assert_eq!((got.output_device, got.buffer_frames), (None, Some(128)));
    }

    #[test]
    fn the_file_round_trips() {
        let mut s = ProfileStore::default();
        s.activate(&v(&["A"]), &v(&["M"]), &hw(Some("A"), Some(256), "1:2"));
        s.activate(&v(&["A", "B"]), &[], &hw(Some("A"), Some(256), "1:2"));
        s.remember(&Hardware { input_device: Some("M".into()), ..hw(Some("B"), None, "1:3") });
        let back = ProfileStore::parse(&s.to_text());
        assert_eq!(back, s);
    }
}
