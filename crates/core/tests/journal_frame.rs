//! Card wav-c-86-c (#86, v1 item I4): the autosave journal's checksum framing,
//! `autosave::fnv1a`, `autosave::seal` and `autosave::unseal`. Locked tests: every expected value
//! is written out here.

use wavcraft_core::autosave::{fnv1a, seal, unseal};

const DAMAGED: &str = "the checksum does not match (the journal is damaged)";
const CUT_SHORT: &str = "no checksum line (the journal is cut short)";

#[test]
fn c86c_fnv1a_matches_the_published_vectors() {
    assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
}

#[test]
fn c86c_seal_appends_one_sum_line() {
    assert_eq!(seal(""), "sum cbf29ce484222325\n");
    assert_eq!(seal("a"), "asum af63dc4c8601ec8c\n");
    assert_eq!(seal("foobar"), "foobarsum 85944171f73967e8\n");
}

#[test]
fn c86c_unseal_gives_back_the_body() {
    assert_eq!(unseal("sum cbf29ce484222325\n"), Ok(""));
    assert_eq!(unseal("foobarsum 85944171f73967e8\n"), Ok("foobar"));
    // The final newline may be missing (a file trimmed by an editor): the sum still checks.
    assert_eq!(unseal("foobarsum 85944171f73967e8"), Ok("foobar"));
    // A body that itself holds "sum " lines: only the last line is the checksum.
    let body = "wavcraft-journal 2\nstep 0 Open\nsum of steps 1\ncursor 0\n";
    let sealed = seal(body);
    assert!(sealed.starts_with(body), "{sealed:?}");
    assert_eq!(sealed.len(), body.len() + 21, "{sealed:?}");
    assert_eq!(unseal(&sealed), Ok(body));
}

#[test]
fn c86c_unseal_refuses_a_damaged_or_cut_journal() {
    // One byte of the body changed.
    assert_eq!(unseal("foobaRsum 85944171f73967e8\n"), Err(DAMAGED.to_string()));
    // The sum itself cut short, or written in capitals.
    assert_eq!(unseal("foobarsum 85944171f73967e\n"), Err(DAMAGED.to_string()));
    assert_eq!(unseal("foobarsum 85944171F73967E8\n"), Err(DAMAGED.to_string()));
    // Something after the sum that is not white space.
    assert_eq!(unseal("foobarsum 85944171f73967e8\nmore\n"), Err(DAMAGED.to_string()));
    // No sum line at all.
    assert_eq!(unseal("foobar\n"), Err(CUT_SHORT.to_string()));
    assert_eq!(unseal(""), Err(CUT_SHORT.to_string()));
}
