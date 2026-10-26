use wavcraft_core::name_template::{NameFields, TemplateError, expand};

fn fields(name: &str) -> NameFields<'_> {
    NameFields { name, index: 7, date: (2026, 3, 9), format: "wav" }
}

#[test]
fn width_pads() {
    assert_eq!(expand("{name}-{n:3}", &fields("Take 1")), Ok("Take 1-007".to_string()));
}

#[test]
fn plain_index() {
    assert_eq!(expand("{n}", &fields("Take 1")), Ok("7".to_string()));
}

#[test]
fn date_and_format() {
    assert_eq!(expand("{date}_{format}", &fields("Take 1")), Ok("2026-03-09_wav".to_string()));
}

#[test]
fn doubled_braces() {
    assert_eq!(expand("{{x}}", &fields("Take 1")), Ok("{x}".to_string()));
}

#[test]
fn errors() {
    let f = fields("Take 1");
    assert_eq!(expand("{bogus}", &f), Err(TemplateError::UnknownToken("bogus".to_string())));
    assert_eq!(expand("{name", &f), Err(TemplateError::UnclosedBrace));
    assert_eq!(expand("{n:0}", &f), Err(TemplateError::BadWidth("0".to_string())));
    assert_eq!(expand("{n:10}", &f), Err(TemplateError::BadWidth("10".to_string())));
    assert_eq!(expand("", &f), Err(TemplateError::Empty));
}

#[test]
fn illegal_characters() {
    assert_eq!(expand("{name}", &fields("a/b:c")), Ok("a_b_c".to_string()));
}

#[test]
fn reserved_names() {
    assert_eq!(expand("{name}", &fields("CON")), Ok("_CON".to_string()));
    assert_eq!(expand("{name}", &fields("nul.txt")), Ok("_nul.txt".to_string()));
}

#[test]
fn only_spaces_and_dots_is_empty() {
    assert_eq!(expand("{name}", &fields("  .. ")), Err(TemplateError::Empty));
}

#[test]
fn long_name_is_cut() {
    let long = "a".repeat(200);
    assert_eq!(expand("{name}", &fields(&long)), Ok("a".repeat(120)));
}

#[test]
fn cut_counts_chars_not_bytes() {
    let long = "é".repeat(130);
    assert_eq!(expand("{name}", &fields(&long)), Ok("é".repeat(120)));
}
