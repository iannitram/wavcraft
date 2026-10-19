//! Level filters: env_logger-style directives, and the `CRAFT_LOG` switch.

use log::LevelFilter;

/// Where the active filter came from (for the build line and the diagnostic report).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    CraftLog,
    RustLog,
    Default,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::CraftLog => "CRAFT_LOG",
            Source::RustLog => "RUST_LOG",
            Source::Default => "default",
        }
    }
}

/// Parse a level word (`off`, `error` … `trace`), case-insensitive.
pub fn level(word: &str) -> Option<LevelFilter> {
    match word.trim().to_ascii_lowercase().as_str() {
        "off" => Some(LevelFilter::Off),
        "error" => Some(LevelFilter::Error),
        "warn" | "warning" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

/// The directive string to use, and where it came from.
///
/// - `craft_log` (the value of `CRAFT_LOG`) set to a level: `warn,<ours>=<level>`, so our own
///   crates log at that level and every third-party crate stays at warn. (A bare `RUST_LOG=debug`
///   turns on every crate: that is how EffectCraft 0.8.0 wrote 624,000 lines in nine seconds.)
/// - else `rust_log` (the value of `RUST_LOG`), unchanged, when not empty;
/// - else `default`.
///
/// `ours` is the app's own directive for its crates in the syntax its filter understands, for
/// example `filmcraft*` or `effectcraft`.
pub fn spec(craft_log: Option<&str>, rust_log: Option<&str>, ours: &str, default: &str) -> (String, Source) {
    if let Some(l) = craft_log.and_then(level) {
        let word = l.as_str().to_ascii_lowercase();
        return (format!("warn,{ours}={word}"), Source::CraftLog);
    }
    if let Some(r) = rust_log.map(str::trim).filter(|r| !r.is_empty()) {
        return (r.to_owned(), Source::RustLog);
    }
    (default.to_owned(), Source::Default)
}

/// [`spec`] from this process's environment.
pub fn spec_from_env(ours: &str, default: &str) -> (String, Source) {
    let craft = std::env::var("CRAFT_LOG").ok();
    let rust = std::env::var("RUST_LOG").ok();
    spec(craft.as_deref(), rust.as_deref(), ours, default)
}

/// `CRAFT_LOG_JSON=1` (or `true`/`yes`): one JSON object per line where craft-log writes the file.
pub fn json_from_env() -> bool {
    matches!(std::env::var("CRAFT_LOG_JSON").ok().as_deref().map(str::trim), Some("1" | "true" | "yes"))
}

/// A per-target filter: a default level and `(target, level)` directives. A directive applies to
/// its target and every target that starts with it (a trailing `*` means the same); the longest
/// matching directive wins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    default: LevelFilter,
    directives: Vec<(String, LevelFilter)>,
}

impl Filter {
    /// Parse `spec` (`warn,filmcraft*=info`, `debug`, `wgpu_core=info`). Unknown levels and empty
    /// parts are skipped. Without a bare level, targets no directive names log errors only.
    pub fn parse(spec: &str) -> Filter {
        let mut f = Filter { default: LevelFilter::Error, directives: Vec::new() };
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once('=') {
                Some((name, lvl)) => {
                    let name = name.trim().trim_end_matches('*');
                    if let (false, Some(l)) = (name.is_empty(), level(lvl)) {
                        f.directives.push((name.to_owned(), l));
                    }
                }
                None => match level(part) {
                    Some(l) => f.default = l,
                    // a bare target name: everything from it (as env_logger)
                    None => f.directives.push((part.trim_end_matches('*').to_owned(), LevelFilter::Trace)),
                },
            }
        }
        f
    }

    /// The level for `target`.
    pub fn level_for(&self, target: &str) -> LevelFilter {
        self.directives.iter().filter(|(name, _)| target.starts_with(name.as_str())).max_by_key(|(name, _)| name.len()).map_or(self.default, |(_, l)| *l)
    }

    /// The most verbose level any target can reach (for `log::set_max_level`).
    pub fn max(&self) -> LevelFilter {
        self.directives.iter().map(|(_, l)| *l).fold(self.default, Ord::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn craft_log_keeps_third_party_crates_at_warn() {
        let (s, src) = spec(Some("debug"), Some("trace"), "filmcraft*", "warn,filmcraft*=info");
        assert_eq!((s.as_str(), src), ("warn,filmcraft*=debug", Source::CraftLog));
        let f = Filter::parse(&s);
        assert_eq!(f.level_for("filmcraft_platform::hybrid"), LevelFilter::Debug);
        assert_eq!(f.level_for("wgpu_core::device"), LevelFilter::Warn);
    }

    #[test]
    fn rust_log_is_honoured_as_it_is_and_the_default_fills_in() {
        assert_eq!(spec(None, Some(" debug "), "x", "warn"), ("debug".into(), Source::RustLog));
        assert_eq!(spec(Some("nonsense"), Some(""), "x", "warn,x=info"), ("warn,x=info".into(), Source::Default));
    }

    #[test]
    fn longest_directive_wins_and_star_is_a_prefix() {
        let f = Filter::parse("warn,filmcraft*=info,filmcraft_codecs=trace,bogus=loud,,");
        assert_eq!(f.level_for("filmcraft_codecs::h264"), LevelFilter::Trace);
        assert_eq!(f.level_for("filmcraft_engine"), LevelFilter::Info);
        assert_eq!(f.level_for("naga"), LevelFilter::Warn);
        assert_eq!(f.max(), LevelFilter::Trace);
        assert_eq!(Filter::parse("").level_for("any"), LevelFilter::Error);
    }
}
