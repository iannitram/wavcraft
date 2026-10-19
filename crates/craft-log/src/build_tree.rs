//! Whether the program is built from uncommitted work, for the build line. Shared by `build.rs`
//! (which includes this file by path) and this crate's tests; it uses nothing from the crate and
//! no dependency.
//!
//! Builds run in containers and sandboxes without a `git` program, so this does not ask git. It
//! approximates: git rewrites its index at every commit, checkout, reset and stash, so a source
//! file modified after the index was last written is (most likely) work not in the commit. The
//! build line says so with a question mark (`+modified?`), because it is an approximation: a file
//! saved without changes counts, and changes staged with `git add` but not committed do not.

use std::path::Path;
use std::time::SystemTime;

/// What the build line adds after the commit when the tree looks modified.
pub const MODIFIED: &str = "+modified?";

/// The folders and files of the repository whose changes make a different program (those that
/// exist are watched). Folders named `target` and hidden ones (`.git`, `.cargo`) inside them are
/// skipped.
pub const WATCHED: &[&str] = &["crates", "apps", "assets", "Cargo.toml", "Cargo.lock"];

/// [`MODIFIED`] when a watched file under `repo` was modified after `index` (git's index) was
/// written, else "" (also when there is no index to compare with).
pub fn suffix(repo: &Path, index: &Path) -> &'static str {
    let Ok(written) = std::fs::metadata(index).and_then(|m| m.modified()) else { return "" };
    let newest = WATCHED.iter().filter_map(|w| newest(&repo.join(w))).max();
    if newest.is_some_and(|t| t > written) { MODIFIED } else { "" }
}

/// The newest modification time of the files at or under `path` (folders' own times are not
/// counted: an editor's swap file would change them).
fn newest(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_dir() {
        return meta.modified().ok();
    }
    let mut best = None;
    for entry in std::fs::read_dir(path).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name.starts_with('.') {
            continue;
        }
        best = best.max(newest(&entry.path()));
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn touch(path: &Path, at: SystemTime) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let f = std::fs::File::create(path).unwrap();
        f.set_modified(at).unwrap();
    }

    #[test]
    fn a_source_file_newer_than_the_index_marks_the_build_modified() {
        let repo = std::env::temp_dir().join(format!("craft-log-build-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        let index = repo.join(".git/index");
        let t = SystemTime::now() - Duration::from_secs(3600);
        assert_eq!(suffix(&repo, &index), "", "no index: nothing to say");
        touch(&index, t);
        touch(&repo.join("crates/a/src/lib.rs"), t - Duration::from_secs(60));
        touch(&repo.join("Cargo.toml"), t - Duration::from_secs(60));
        assert_eq!(suffix(&repo, &index), "", "everything older than the index: as committed");
        // build output and hidden folders do not count
        touch(&repo.join("crates/a/target/debug/x"), t + Duration::from_secs(60));
        touch(&repo.join("crates/a/.cache/y"), t + Duration::from_secs(60));
        touch(&repo.join("docs/notes.md"), t + Duration::from_secs(60));
        assert_eq!(suffix(&repo, &index), "");
        // a source edited after the last commit does
        touch(&repo.join("apps/b/src/main.rs"), t + Duration::from_secs(60));
        assert_eq!(suffix(&repo, &index), MODIFIED);
        let _ = std::fs::remove_dir_all(&repo);
    }
}
