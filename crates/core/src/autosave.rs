//! Auto Save (v1 item I4, #86): the journal an open session writes into its session folder so
//! that, after a crash, the next start can offer to recover the work.
//!
//! This module holds the journal's file names and its framing: the journal is plain text that
//! ends in one checksum line, `sum <16 lowercase hex digits>`, the 64-bit FNV-1a of every byte
//! before that line. A journal cut short or changed on disk fails the check and is not used.

/// The journal in a session folder.
pub const JOURNAL_FILE: &str = "journal.wcj";
/// Where a journal is written before it is renamed over [`JOURNAL_FILE`]. A crash between the
/// two can leave this file behind.
pub const JOURNAL_TMP: &str = "journal.wcj.tmp";

/// The 64-bit FNV-1a hash of `bytes`.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    // Stub: card wav-c-86-c replaces this body.
    let _ = bytes;
    0
}

/// `body` with its checksum line appended: `body`, then `sum `, then [`fnv1a`] of `body`'s bytes
/// as 16 lowercase hex digits, then `\n`.
pub fn seal(body: &str) -> String {
    // Stub: card wav-c-86-c replaces this body.
    let _ = body;
    String::new()
}

/// The body of a sealed journal text, if its checksum line is there and matches.
pub fn unseal(text: &str) -> Result<&str, String> {
    // Stub: card wav-c-86-c replaces this body.
    let _ = text;
    Err(String::new())
}
