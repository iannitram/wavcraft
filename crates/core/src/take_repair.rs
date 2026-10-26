//! J3 (a crash keeps the take): repairing the header of a take whose app was killed mid-take.
//!
//! A take is a plain 44-byte-header WAV (`RIFF`, a 16-byte `fmt ` chunk, `data`) whose two size
//! fields are rewritten about once a second while recording. If the app is killed, the sizes on
//! disk are older than the samples after them. [`repair_header`] works out, from the header and
//! the file's length alone, the sizes that cover every whole frame in the file. It reads and
//! writes nothing; `take_recover` does the file side.

// Stub for wav-c-116-a: the builder replaces the body and deletes these two lines.
#![allow(clippy::todo)]

/// Bytes in a take's header: `RIFF` (12), `fmt ` of 16 bytes (24), `data` id and size (8).
pub const HEADER_BYTES: usize = 44;

/// What [`repair_header`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderRepair {
    /// Not a 44-byte PCM or float WAV header as a take has: leave the file alone.
    NotATake,
    /// The two sizes already cover every whole frame in the file.
    Intact,
    /// The header with its RIFF size (bytes 4..8) and data size (bytes 40..44) corrected;
    /// every other byte as it was.
    Fixed([u8; HEADER_BYTES]),
}

/// The repaired header for a take whose first bytes are `header` and whose file is `file_len`
/// bytes long.
pub fn repair_header(header: &[u8], file_len: u64) -> HeaderRepair {
    let _ = (header, file_len);
    todo!("wav-c-116-a: repair_header")
}
