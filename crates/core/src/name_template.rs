//! The output-name template used by Save and Export: `{name}-{n:3}` becomes a safe file name.

/// The values a template can use.
pub struct NameFields<'a> {
    pub name: &'a str,
    pub index: u32,
    /// Year, month, day.
    pub date: (u16, u8, u8),
    pub format: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TemplateError {
    Empty,
    UnknownToken(String),
    UnclosedBrace,
    BadWidth(String),
}

/// Expand `template` with `fields` and make the result a safe file name.
pub fn expand(template: &str, fields: &NameFields) -> Result<String, TemplateError> {
    if template.is_empty() {
        return Err(TemplateError::Empty);
    }
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    out.push('{');
                    continue;
                }
                let mut token = String::new();
                let mut closed = false;
                for t in chars.by_ref() {
                    if t == '}' {
                        closed = true;
                        break;
                    }
                    token.push(t);
                }
                if !closed {
                    return Err(TemplateError::UnclosedBrace);
                }
                out.push_str(&expand_token(&token, fields)?);
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                }
                out.push('}');
            }
            other => out.push(other),
        }
    }
    let clean = sanitize_file_name(&out);
    if clean.is_empty() { Err(TemplateError::Empty) } else { Ok(clean) }
}

fn expand_token(token: &str, fields: &NameFields) -> Result<String, TemplateError> {
    match token {
        "name" => Ok(fields.name.to_string()),
        "n" => Ok(fields.index.to_string()),
        "date" => {
            let (y, m, d) = fields.date;
            Ok(format!("{y:04}-{m:02}-{d:02}"))
        }
        "format" => Ok(fields.format.to_string()),
        _ => match token.strip_prefix("n:") {
            Some(w) => {
                let width = if !w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()) {
                    w.parse::<usize>().ok().filter(|v| (1..=9).contains(v))
                } else {
                    None
                };
                match width {
                    Some(width) => Ok(format!("{:0>width$}", fields.index)),
                    None => Err(TemplateError::BadWidth(w.to_string())),
                }
            }
            None => Err(TemplateError::UnknownToken(token.to_string())),
        },
    }
}

fn is_reserved(stem: &str) -> bool {
    let up = stem.to_ascii_uppercase();
    if matches!(up.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(d) = up.strip_prefix(prefix)
            && matches!(d.as_bytes(), [b'1'..=b'9'])
        {
            return true;
        }
    }
    false
}

/// Make `raw` safe as a file name on Windows, Mac and Linux.
pub fn sanitize_file_name(raw: &str) -> String {
    let replaced: String =
        raw.chars().map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c < ' ' { '_' } else { c }).collect();
    let trimmed = replaced.trim_matches(|c| c == ' ' || c == '.');
    if trimmed.is_empty() {
        return String::new();
    }
    let stem = trimmed.split('.').next().unwrap_or("");
    let named = if is_reserved(stem) { format!("_{trimmed}") } else { trimmed.to_string() };
    named.chars().take(120).collect()
}
