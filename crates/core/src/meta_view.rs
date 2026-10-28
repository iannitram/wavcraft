//! How the Metadata panel looks at the metadata (roadmap A269, A270, A271): a text filter over
//! the properties, named display sets (which fields the panel shows, saved to a small text file),
//! and the AIFF `AUTH` chunk <-> Dublin Core creator mapping.
//!
//! None of this changes the file's metadata; it only chooses what is listed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::meta::Metadata;
use crate::xmp;

/// One line of the panel: a field the panel knows or an XMP property of the packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The name `META get` takes: a panel name, or `xmp:<ns>:<prop>` for another property.
    pub key: String,
    pub label: String,
    pub value: String,
}

/// Every row: the file fields, the known XMP fields, then each other XMP property of the packet.
pub fn rows(meta: &Metadata) -> Vec<Row> {
    let mut out = Vec::new();
    for key in Metadata::panel_fields() {
        let label = xmp::FIELDS.iter().find(|f| f.name == key).map_or_else(|| key.replace('_', " "), |f| f.label.to_string());
        out.push(Row { key: key.to_string(), label, value: meta.get(key).unwrap_or_default() });
    }
    if let Some(x) = &meta.xmp {
        for p in x.properties() {
            if xmp::FIELDS.iter().any(|f| f.ns == p.ns && f.prop == p.name) {
                continue;
            }
            out.push(Row { key: format!("xmp:{}:{}", p.ns, p.name), label: format!("{} ({})", p.name, p.ns), value: p.value });
        }
    }
    out
}

/// The rows whose name, label or value contains `needle` (case does not matter). An empty
/// `needle` keeps every row.
pub fn filter_rows(rows: Vec<Row>, needle: &str) -> Vec<Row> {
    let n = needle.trim().to_lowercase();
    if n.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|r| r.key.to_lowercase().contains(&n) || r.label.to_lowercase().contains(&n) || r.value.to_lowercase().contains(&n))
        .collect()
}

/// Named sets of field keys, the keys now shown, and the set last loaded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisplaySets {
    pub sets: BTreeMap<String, Vec<String>>,
    /// `None`: every field is shown.
    pub shown: Option<Vec<String>>,
    pub active: Option<String>,
}

fn check_name(name: &str) -> Result<&str, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("a set needs a name".to_string());
    }
    if n.contains(['\t', '\n', '\r']) {
        return Err("a set name cannot hold a tab or a line break".to_string());
    }
    Ok(n)
}

impl DisplaySets {
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("wavcraft").join("metadata-sets.txt"))
    }

    /// Whether `key` is shown now.
    pub fn is_shown(&self, key: &str) -> bool {
        self.shown.as_ref().is_none_or(|s| s.iter().any(|k| k == key))
    }

    /// Show exactly `keys` (`None`: all).
    pub fn show_only(&mut self, keys: Option<Vec<String>>) {
        self.shown = keys;
        self.active = None;
    }

    /// Save the fields shown now (all of `all_keys` when none were chosen) under `name`.
    pub fn save(&mut self, name: &str, all_keys: &[String]) -> Result<(), String> {
        let n = check_name(name)?.to_string();
        let keys = self.shown.clone().unwrap_or_else(|| all_keys.to_vec());
        self.sets.insert(n.clone(), keys);
        self.active = Some(n);
        Ok(())
    }

    pub fn load(&mut self, name: &str) -> Result<(), String> {
        let n = check_name(name)?;
        let keys = self.sets.get(n).ok_or_else(|| format!("no set named {n:?}"))?.clone();
        self.shown = Some(keys);
        self.active = Some(n.to_string());
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let n = check_name(name)?;
        self.sets.remove(n).ok_or_else(|| format!("no set named {n:?}"))?;
        if self.active.as_deref() == Some(n) {
            self.active = None;
        }
        Ok(())
    }

    /// The file's text: `set<TAB>name<TAB>key,key`, `shown<TAB>key,key`, `active<TAB>name`.
    pub fn to_text(&self) -> String {
        let mut s = String::from("# WavCraft metadata display sets\n");
        for (n, k) in &self.sets {
            s.push_str(&format!("set\t{n}\t{}\n", k.join(",")));
        }
        if let Some(k) = &self.shown {
            s.push_str(&format!("shown\t{}\n", k.join(",")));
        }
        if let Some(a) = &self.active {
            s.push_str(&format!("active\t{a}\n"));
        }
        s
    }

    pub fn parse(text: &str) -> DisplaySets {
        let mut d = DisplaySets::default();
        let keys = |s: &str| s.split(',').filter(|k| !k.is_empty()).map(str::to_string).collect::<Vec<_>>();
        for line in text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            match parts.as_slice() {
                ["set", n, k] if !n.is_empty() => {
                    d.sets.insert((*n).to_string(), keys(k));
                }
                ["shown", k] => d.shown = Some(keys(k)),
                ["active", n] if !n.is_empty() => d.active = Some((*n).to_string()),
                _ => {}
            }
        }
        d
    }

    /// Read `path`; a missing or unreadable file gives the empty state.
    pub fn load_file(path: &Path) -> DisplaySets {
        match std::fs::read_to_string(path) {
            Ok(t) => DisplaySets::parse(&t),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("could not read the metadata sets {}: {e}", path.display());
                }
                DisplaySets::default()
            }
        }
    }

    pub fn save_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }
}

/// An AIFF `AUTH` chunk (id, big-endian size, text, a pad byte when the size is odd) for `creator`.
pub fn auth_chunk(creator: &str) -> Vec<u8> {
    let body = creator.as_bytes();
    let mut v = b"AUTH".to_vec();
    v.extend_from_slice(&(body.len() as u32).to_be_bytes());
    v.extend_from_slice(body);
    if body.len() % 2 == 1 {
        v.push(0);
    }
    v
}

/// The author text in the body of an `AUTH` chunk (up to the first zero byte).
pub fn creator_from_auth(body: &[u8]) -> String {
    let end = body.iter().position(|&c| c == 0).unwrap_or(body.len());
    String::from_utf8_lossy(&body[..end]).trim().to_string()
}

/// The text an AIFF `AUTH` chunk should hold for this metadata: the Dublin Core creator, if any.
pub fn auth_for(meta: &Metadata) -> Option<String> {
    meta.get("xmp_creator").filter(|c| !c.is_empty())
}

/// Read an `AUTH` body into the Dublin Core creator, unless the packet already has one (the
/// packet wins). Returns whether the creator was set.
pub fn creator_from_auth_into(meta: &mut Metadata, auth_body: &[u8]) -> Result<bool, String> {
    let a = creator_from_auth(auth_body);
    if a.is_empty() || auth_for(meta).is_some() {
        return Ok(false);
    }
    meta.set("xmp_creator", &a)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> Metadata {
        let mut m = Metadata::default();
        m.set("title", "Rain on glass").unwrap();
        m.set("xmp_creator", "A. Writer").unwrap();
        m.set("xmp_genre", "ambient").unwrap();
        m
    }

    #[test]
    fn filter_matches_name_or_value_ignoring_case() {
        let all = rows(&meta());
        assert!(all.len() >= 18);
        let by_value = filter_rows(all.clone(), "RAIN");
        assert_eq!(by_value.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["title"]);
        let by_name = filter_rows(all.clone(), "genre");
        assert!(by_name.iter().any(|r| r.key == "xmp_genre"), "{:?}", by_name.iter().map(|r| &r.key).collect::<Vec<_>>());
        assert_eq!(filter_rows(all.clone(), "").len(), all.len());
        assert!(filter_rows(all, "zzz-none").is_empty());
    }

    #[test]
    fn filter_reaches_properties_the_panel_does_not_know() {
        let mut m = Metadata::default();
        m.set("xmp:http://example.com/acme/1.0/:mood", "calm").unwrap();
        let hits = filter_rows(rows(&m), "calm");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].key.ends_with(":mood"));
    }

    #[test]
    fn sets_save_load_delete_and_persist() {
        let keys: Vec<String> = Metadata::panel_fields().iter().map(|s| s.to_string()).collect();
        let mut d = DisplaySets::default();
        assert!(d.is_shown("title"));
        d.show_only(Some(vec!["title".into(), "xmp_creator".into()]));
        assert!(!d.is_shown("comment"));
        d.save("tags only", &keys).unwrap();
        d.show_only(None);
        d.save("everything", &keys).unwrap();
        d.load("tags only").unwrap();
        assert_eq!(d.shown.as_deref().unwrap(), ["title", "xmp_creator"]);
        let dir = std::env::temp_dir().join(format!("wavcraft-metaset-{}", std::process::id()));
        let path = dir.join("sub").join("sets.txt");
        d.save_file(&path).unwrap();
        let back = DisplaySets::load_file(&path);
        assert_eq!(back, d);
        assert_eq!(back.sets.len(), 2);
        let mut back = back;
        back.delete("tags only").unwrap();
        assert!(back.active.is_none());
        assert!(back.load("tags only").is_err());
        assert!(back.save("  ", &keys).is_err());
        assert_eq!(DisplaySets::load_file(&dir.join("missing.txt")), DisplaySets::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creator_maps_to_the_aiff_author_chunk_and_back() {
        let m = meta();
        let text = auth_for(&m).unwrap();
        assert_eq!(text, "A. Writer");
        let c = auth_chunk(&text);
        assert_eq!(&c[..4], b"AUTH");
        assert_eq!(u32::from_be_bytes(c[4..8].try_into().unwrap()), 9);
        assert_eq!(c.len(), 8 + 10, "odd size is padded");
        let mut back = Metadata::default();
        assert!(creator_from_auth_into(&mut back, &c[8..8 + 9]).unwrap());
        assert_eq!(back.get("xmp_creator").unwrap(), "A. Writer");
        assert!(!creator_from_auth_into(&mut back, b"Someone Else").unwrap(), "the packet wins");
        assert_eq!(back.get("xmp_creator").unwrap(), "A. Writer");
        assert_eq!(auth_for(&Metadata::default()), None);
    }
}
