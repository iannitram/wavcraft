//! The third-party notices shown under Help › About (A346). The text is
//! `THIRD-PARTY-NOTICES.txt` at the repository root, made by `tools/notices.py --write` from
//! Cargo.lock, and compiled into the program so it travels with every binary.

/// The notices: every library with its licence, and the MPL-2.0 and font statements.
pub const TEXT: &str = include_str!("../../../THIRD-PARTY-NOTICES.txt");

#[cfg(test)]
mod tests {
    use super::TEXT;

    #[test]
    fn the_notices_carry_the_mpl_and_font_statements() {
        assert!(TEXT.contains("Mozilla Public License"), "MPL statement");
        assert!(TEXT.contains("https://github.com/pdeljanov/Symphonia"), "where the symphonia source is");
        assert!(TEXT.contains("symphonia-core 0.5.5"), "a symphonia entry");
        assert!(TEXT.contains("Ubuntu Font Licence"), "font statement");
        assert!(TEXT.contains("SIL OPEN FONT LICENSE") || TEXT.contains("SIL Open Font License"), "OFL text or statement");
        assert!(TEXT.contains("epaint_default_fonts"), "the fonts' crate");
        assert!(TEXT.len() > 100_000, "the full texts are in, not a stub");
    }

    /// The lock file's libraries that have no `name version` line in `notices`. Windows checks
    /// the notices out with CRLF endings, so they are normalised to LF before matching.
    fn libraries_missing_from(notices: &str) -> Vec<String> {
        let text = notices.replace("\r\n", "\n");
        let lock = include_str!("../../../Cargo.lock");
        let mut name = "";
        let mut missing = Vec::new();
        for line in lock.lines() {
            if let Some(n) = line.strip_prefix("name = \"") {
                name = n.trim_end_matches('"');
            } else if let Some(v) = line.strip_prefix("version = \"") {
                let entry = format!("\n{name} {}\n", v.trim_end_matches('"'));
                if !name.starts_with("wavcraft") && !name.starts_with("craft-") && !text.contains(&entry) {
                    missing.push(entry.trim().to_string());
                }
            }
        }
        missing
    }

    #[test]
    fn every_library_in_the_lock_file_is_listed() {
        let missing = libraries_missing_from(TEXT);
        assert!(missing.is_empty(), "not in THIRD-PARTY-NOTICES.txt (run tools/notices.py --write): {missing:?}");
    }

    #[test]
    fn the_lock_check_also_passes_with_crlf_line_endings() {
        let crlf = TEXT.replace('\n', "\r\n");
        assert!(crlf.contains("\r\n"), "the copy really has CRLF endings");
        let missing = libraries_missing_from(&crlf);
        assert!(missing.is_empty(), "CRLF notices should match too: {missing:?}");
    }
}
