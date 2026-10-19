//! Random identifiers from the operating system's CSPRNG (Codex review CX-W2/W4, 2026-10-19):
//! temporary file names, the control channel's token, this machine's id. There is no fallback to
//! time or process ids: when the system will not give random bytes, the caller fails closed.

use std::io;

/// `bytes` random bytes from the operating system, as lowercase hex (twice as many characters).
pub fn hex(bytes: usize) -> io::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| io::Error::other(format!("the system gave no random bytes: {e}")))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_has_the_length_asked_for_and_differs_each_time() {
        let (a, b) = (super::hex(32).unwrap(), super::hex(32).unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, b);
    }
}
