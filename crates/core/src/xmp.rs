//! XMP in a WAV (roadmap A268): the `_PMX` chunk holds an XMP packet (RDF/XML). The packet is read
//! into a small property model (namespace URI + property name -> value), the known fields are
//! shown and edited, and a save writes the packet back.
//!
//! The packet's text is kept as it was read. An edit is a splice of the one property's bytes, so
//! every other property, comment, namespace declaration, padding and the `xpacket` wrapper come
//! back byte for byte; a property this module has no field for is "unknown" and simply stays.
//! After each edit the new text is parsed again and every other property is checked to be
//! unchanged; if not, nothing changes.
//!
//! The parser is our own and covers the subset XMP uses: elements, attributes, text, comments,
//! processing instructions, CDATA (kept, not interpreted). A DOCTYPE or any entity beyond the
//! five predefined ones and numeric references is refused (no entity expansion), as is deep or
//! huge input. A packet that cannot be read is reported and kept verbatim by the save.

use std::ops::Range;

/// A packet bigger than this is left alone (copied through, not parsed).
pub const MAX_XMP_BYTES: u64 = 4 << 20;
const MAX_DEPTH: usize = 48;
const MAX_ELEMENTS: usize = 20_000;

pub const NS_RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
pub const NS_XMP: &str = "http://ns.adobe.com/xap/1.0/";
pub const NS_XMPDM: &str = "http://ns.adobe.com/xmp/1.0/DynamicMedia/";
const NS_XML: &str = "http://www.w3.org/XML/1998/namespace";

/// A field the panel and `META` know: its name, label, namespace and property.
pub struct Field {
    pub name: &'static str,
    pub label: &'static str,
    pub ns: &'static str,
    pub prop: &'static str,
}

/// The known fields, in the panel's order.
pub const FIELDS: [Field; 8] = [
    Field { name: "xmp_title", label: "XMP title (Dublin Core)", ns: NS_DC, prop: "title" },
    Field { name: "xmp_creator", label: "XMP creator (Dublin Core)", ns: NS_DC, prop: "creator" },
    Field { name: "xmp_description", label: "XMP description (Dublin Core)", ns: NS_DC, prop: "description" },
    Field { name: "xmp_rights", label: "XMP rights (Dublin Core)", ns: NS_DC, prop: "rights" },
    Field { name: "xmp_artist", label: "XMP artist (xmpDM)", ns: NS_XMPDM, prop: "artist" },
    Field { name: "xmp_album", label: "XMP album (xmpDM)", ns: NS_XMPDM, prop: "album" },
    Field { name: "xmp_genre", label: "XMP genre (xmpDM)", ns: NS_XMPDM, prop: "genre" },
    Field { name: "xmp_track", label: "XMP track number (xmpDM)", ns: NS_XMPDM, prop: "trackNumber" },
];

/// The namespace a well-known prefix stands for.
fn well_known_prefix(p: &str) -> Option<&'static str> {
    match p {
        "dc" => Some(NS_DC),
        "xmp" => Some(NS_XMP),
        "xmpDM" => Some(NS_XMPDM),
        _ => None,
    }
}

fn preferred_prefix(uri: &str) -> Option<&'static str> {
    match uri {
        NS_DC => Some("dc"),
        NS_XMP => Some("xmp"),
        NS_XMPDM => Some("xmpDM"),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ArrKind {
    Alt,
    Seq,
    Bag,
}

/// How a property is written when this module adds one.
fn kind_for(ns: &str, name: &str) -> Option<ArrKind> {
    if ns == NS_DC {
        return match name {
            "title" | "description" | "rights" => Some(ArrKind::Alt),
            "creator" | "date" => Some(ArrKind::Seq),
            "contributor" | "language" | "publisher" | "relation" | "subject" | "type" => Some(ArrKind::Bag),
            _ => None,
        };
    }
    None
}

/// One property as the model shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub ns: String,
    pub name: String,
    /// Text of a simple property; the `x-default` (or first) item of a language alternative;
    /// the items of a sequence or bag joined by `"; "`. Empty for a structured property.
    pub value: String,
    /// Dublin Core, XMP basic or xmpDM: the panel knows these. Others are kept verbatim.
    pub known: bool,
    /// False for a structured value (nested elements, `rdf:parseType`, `rdf:resource`).
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    lang: Option<String>,
    inner: Range<usize>,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Form {
    /// An attribute of `rdf:Description`: the whole attribute with its leading space, the
    /// text between the quotes, the quote character.
    Attr {
        full: Range<usize>,
        inner: Range<usize>,
        quote: u8,
    },
    /// An element holding text: the element, and the text span (`None` for `<p:x/>`).
    Text {
        el: Range<usize>,
        inner: Option<Range<usize>>,
    },
    /// An element holding `rdf:Alt`, `rdf:Seq` or `rdf:Bag` of plain `rdf:li` items.
    Array {
        el: Range<usize>,
        kind: ArrKind,
        cont: Range<usize>,
        cont_inner: Option<Range<usize>>,
        cont_name: String,
        items: Vec<Item>,
    },
    Opaque {
        el: Range<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Prop {
    ns: String,
    name: String,
    value: String,
    form: Form,
}

/// The first `rdf:Description`, where a new property goes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    open: Range<usize>,
    close: Option<Range<usize>>,
    qname: String,
    scope: Vec<(String, String)>,
}

/// An XMP packet: its text and the properties found in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xmp {
    raw: String,
    props: Vec<Prop>,
    target: Option<Target>,
    read_only: bool,
}

// ---------------------------------------------------------------- the XML subset

struct Attr {
    qname: String,
    uri: String,
    local: String,
    full: Range<usize>,
    inner: Range<usize>,
    quote: u8,
    value: String,
}

struct El {
    qname: String,
    uri: String,
    local: String,
    attrs: Vec<Attr>,
    open: Range<usize>,
    close: Option<Range<usize>>,
    kids: Vec<Kid>,
    scope: Vec<(String, String)>,
}

enum Kid {
    El(El),
    Text(Range<usize>),
    Other,
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
    count: usize,
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

fn decode(t: &str) -> Result<String, String> {
    if !t.contains('&') {
        return Ok(t.to_string());
    }
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after.find(';').filter(|e| *e <= 10).ok_or("an & that is not a character reference (entities are not expanded)")?;
        let name = &after[..end];
        match name {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            n if n.starts_with('#') => {
                let num = n[1..].strip_prefix(['x', 'X']).map_or_else(|| n[1..].parse::<u32>().ok(), |h| u32::from_str_radix(h, 16).ok());
                let c = num.and_then(char::from_u32).filter(|c| *c != '\0').ok_or("a bad numeric character reference")?;
                out.push(c);
            }
            other => return Err(format!("the entity &{other}; is not expanded")),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn lookup<'s>(scope: &'s [(String, String)], prefix: &str) -> Option<&'s str> {
    scope.iter().rev().find(|(p, _)| p == prefix).map(|(_, u)| u.as_str())
}

impl<'a> Parser<'a> {
    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} (at byte {})", self.i))
    }

    fn skip_ws(&mut self) {
        while self.b.get(self.i).is_some_and(|c| is_ws(*c)) {
            self.i += 1;
        }
    }

    fn starts(&self, p: &str) -> bool {
        self.b.get(self.i..).is_some_and(|r| r.starts_with(p.as_bytes()))
    }

    fn skip_until(&mut self, end: &str, what: &str) -> Result<(), String> {
        match self.s.get(self.i..).and_then(|r| r.find(end)) {
            Some(n) => {
                self.i += n + end.len();
                Ok(())
            }
            None => self.err(&format!("unterminated {what}")),
        }
    }

    /// Comments, processing instructions and white space outside the root.
    fn misc(&mut self) -> Result<(), String> {
        loop {
            while self.b.get(self.i).is_some_and(|c| is_ws(*c) || *c == 0) {
                self.i += 1;
            }
            if self.starts("<!--") {
                self.skip_until("-->", "comment")?;
            } else if self.starts("<?") {
                self.skip_until("?>", "processing instruction")?;
            } else if self.starts("<!") {
                return self.err("a DOCTYPE or declaration is refused");
            } else {
                return Ok(());
            }
        }
    }

    fn name(&mut self) -> Result<String, String> {
        let start = self.i;
        while self.b.get(self.i).is_some_and(|c| !is_ws(*c) && !matches!(c, b'/' | b'>' | b'=' | b'<' | b'"' | b'\'' | b'&')) {
            self.i += 1;
        }
        if self.i == start {
            return self.err("a name is expected");
        }
        Ok(self.s[start..self.i].to_string())
    }

    fn element(&mut self, depth: usize, scope: &mut Vec<(String, String)>) -> Result<El, String> {
        if depth > MAX_DEPTH {
            return self.err("elements nested too deeply");
        }
        self.count += 1;
        if self.count > MAX_ELEMENTS {
            return self.err("too many elements");
        }
        let start = self.i;
        self.i += 1; // '<'
        let qname = self.name()?;
        let mut attrs: Vec<Attr> = Vec::new();
        let (open_end, self_closing);
        loop {
            let ws_start = self.i;
            self.skip_ws();
            match self.b.get(self.i) {
                None => return self.err("unterminated tag"),
                Some(b'>') => {
                    self.i += 1;
                    (open_end, self_closing) = (self.i, false);
                    break;
                }
                Some(b'/') if self.b.get(self.i + 1) == Some(&b'>') => {
                    self.i += 2;
                    (open_end, self_closing) = (self.i, true);
                    break;
                }
                _ => {}
            }
            if self.i == ws_start {
                return self.err("a space is expected between attributes");
            }
            let aname = self.name()?;
            self.skip_ws();
            if self.b.get(self.i) != Some(&b'=') {
                return self.err("an attribute needs =");
            }
            self.i += 1;
            self.skip_ws();
            let quote = match self.b.get(self.i) {
                Some(q @ (b'"' | b'\'')) => *q,
                _ => return self.err("an attribute value needs quotes"),
            };
            self.i += 1;
            let vstart = self.i;
            while self.b.get(self.i).is_some_and(|c| *c != quote) {
                if self.b[self.i] == b'<' {
                    return self.err("< in an attribute value");
                }
                self.i += 1;
            }
            if self.i >= self.b.len() {
                return self.err("unterminated attribute value");
            }
            let inner = vstart..self.i;
            self.i += 1;
            if attrs.iter().any(|a| a.qname == aname) {
                return self.err("a repeated attribute");
            }
            let value = decode(&self.s[inner.clone()]).map_err(|e| format!("{e} (at byte {vstart})"))?;
            attrs.push(Attr { qname: aname, uri: String::new(), local: String::new(), full: ws_start..self.i, inner, quote, value });
        }
        let pushed = scope.len();
        for a in &attrs {
            if a.qname == "xmlns" {
                scope.push((String::new(), a.value.clone()));
            } else if let Some(p) = a.qname.strip_prefix("xmlns:") {
                scope.push((p.to_string(), a.value.clone()));
            }
        }
        let (prefix, local) = qname.split_once(':').map_or(("", qname.as_str()), |(p, l)| (p, l));
        let uri = match lookup(scope, prefix) {
            Some(u) => u.to_string(),
            None if prefix.is_empty() => String::new(),
            None => return self.err(&format!("the prefix {prefix} is not declared")),
        };
        let local = local.to_string();
        for a in &mut attrs {
            if a.qname == "xmlns" || a.qname.starts_with("xmlns:") {
                continue;
            }
            match a.qname.split_once(':') {
                Some(("xml", l)) => (a.uri, a.local) = (NS_XML.to_string(), l.to_string()),
                Some((p, l)) => match lookup(scope, p) {
                    Some(u) => (a.uri, a.local) = (u.to_string(), l.to_string()),
                    None => return Err(format!("the prefix {p} is not declared (at byte {start})")),
                },
                None => a.local = a.qname.clone(),
            }
        }
        let mut el = El { qname, uri, local, attrs, open: start..open_end, close: None, kids: Vec::new(), scope: scope.clone() };
        if !self_closing {
            self.content(&mut el, depth, scope)?;
        }
        scope.truncate(pushed);
        Ok(el)
    }

    fn content(&mut self, el: &mut El, depth: usize, scope: &mut Vec<(String, String)>) -> Result<(), String> {
        loop {
            if self.i >= self.b.len() {
                return self.err(&format!("<{}> is not closed", el.qname));
            }
            if self.starts("</") {
                let cstart = self.i;
                self.i += 2;
                let n = self.name()?;
                if n != el.qname {
                    return self.err(&format!("</{n}> does not close <{}>", el.qname));
                }
                self.skip_ws();
                if self.b.get(self.i) != Some(&b'>') {
                    return self.err("an end tag needs >");
                }
                self.i += 1;
                el.close = Some(cstart..self.i);
                return Ok(());
            } else if self.starts("<!--") {
                self.skip_until("-->", "comment")?;
                el.kids.push(Kid::Other);
            } else if self.starts("<![CDATA[") {
                self.skip_until("]]>", "CDATA section")?;
                el.kids.push(Kid::Other);
            } else if self.starts("<?") {
                self.skip_until("?>", "processing instruction")?;
                el.kids.push(Kid::Other);
            } else if self.starts("<!") {
                return self.err("a DOCTYPE or declaration is refused");
            } else if self.b[self.i] == b'<' {
                let child = self.element(depth + 1, scope)?;
                el.kids.push(Kid::El(child));
            } else {
                let start = self.i;
                while self.b.get(self.i).is_some_and(|c| *c != b'<') {
                    self.i += 1;
                }
                decode(&self.s[start..self.i]).map_err(|e| format!("{e} (at byte {start})"))?;
                el.kids.push(Kid::Text(start..self.i));
            }
        }
    }
}

fn parse_document(s: &str) -> Result<El, String> {
    let mut p = Parser { s, b: s.as_bytes(), i: 0, count: 0 };
    if s.starts_with('\u{feff}') {
        p.i = '\u{feff}'.len_utf8();
    }
    p.misc()?;
    if p.b.get(p.i) != Some(&b'<') {
        return p.err("no XML element found");
    }
    let root = p.element(0, &mut Vec::new())?;
    p.misc()?;
    if p.i < p.b.len() {
        return p.err("content after the root element");
    }
    Ok(root)
}

// ---------------------------------------------------------------- the property model

fn only_ws(s: &str, r: &Range<usize>) -> bool {
    s.get(r.clone()).is_some_and(|t| t.trim().is_empty())
}

fn has_rdf_or_odd_attr(el: &El) -> bool {
    el.attrs.iter().any(|a| !(a.qname == "xmlns" || a.qname.starts_with("xmlns:") || a.uri == NS_XML))
}

fn text_of(s: &str, el: &El) -> Option<(String, Option<Range<usize>>)> {
    if el.kids.iter().any(|k| !matches!(k, Kid::Text(_))) {
        return None;
    }
    let close = el.close.as_ref()?;
    let inner = el.open.end..close.start;
    decode(&s[inner.clone()]).ok().map(|v| (v, Some(inner)))
}

fn classify(s: &str, c: &El) -> (String, Form) {
    let whole = c.open.start..c.close.as_ref().map_or(c.open.end, |x| x.end);
    let opaque = (String::new(), Form::Opaque { el: whole.clone() });
    if has_rdf_or_odd_attr(c) {
        return opaque;
    }
    if c.close.is_none() {
        return (String::new(), Form::Text { el: whole, inner: None });
    }
    if let Some((value, inner)) = text_of(s, c) {
        return (value, Form::Text { el: whole, inner });
    }
    // One container element, white space around it.
    let mut els = c.kids.iter().filter_map(|k| if let Kid::El(e) = k { Some(e) } else { None });
    let (Some(cont), None) = (els.next(), els.next()) else { return opaque };
    if c.kids.iter().any(|k| matches!(k, Kid::Other)) || c.kids.iter().any(|k| matches!(k, Kid::Text(r) if !only_ws(s, r))) {
        return opaque;
    }
    let kind = match (cont.uri.as_str(), cont.local.as_str()) {
        (NS_RDF, "Alt") => ArrKind::Alt,
        (NS_RDF, "Seq") => ArrKind::Seq,
        (NS_RDF, "Bag") => ArrKind::Bag,
        _ => return opaque,
    };
    if has_rdf_or_odd_attr(cont) || cont.kids.iter().any(|k| matches!(k, Kid::Other)) {
        return opaque;
    }
    let mut items = Vec::new();
    for k in &cont.kids {
        match k {
            Kid::Text(r) if only_ws(s, r) => {}
            Kid::El(li) if li.uri == NS_RDF && li.local == "li" && !has_rdf_or_odd_attr(li) && li.close.is_some() => {
                let Some((value, Some(inner))) = text_of(s, li) else { return opaque };
                let lang = li.attrs.iter().find(|a| a.uri == NS_XML && a.local == "lang").map(|a| a.value.clone());
                items.push(Item { lang, inner, value });
            }
            _ => return opaque,
        }
    }
    let value = match kind {
        ArrKind::Alt => items.iter().find(|i| i.lang.as_deref() == Some("x-default")).or(items.first()).map(|i| i.value.clone()).unwrap_or_default(),
        _ => items.iter().map(|i| i.value.as_str()).collect::<Vec<_>>().join("; "),
    };
    let cont_range = cont.open.start..cont.close.as_ref().map_or(cont.open.end, |x| x.end);
    let cont_inner = cont.close.as_ref().map(|x| cont.open.end..x.start);
    (value, Form::Array { el: whole, kind, cont: cont_range, cont_inner, cont_name: cont.qname.clone(), items })
}

fn find_rdf(el: &El, depth: usize) -> Option<&El> {
    if el.uri == NS_RDF && el.local == "RDF" {
        return Some(el);
    }
    if depth >= 4 {
        return None;
    }
    el.kids.iter().find_map(|k| if let Kid::El(e) = k { find_rdf(e, depth + 1) } else { None })
}

fn collect(s: &str, root: &El) -> (Vec<Prop>, Option<Target>) {
    let mut props = Vec::new();
    let mut target = None;
    let Some(rdf) = find_rdf(root, 0) else { return (props, target) };
    for k in &rdf.kids {
        let Kid::El(d) = k else { continue };
        if !(d.uri == NS_RDF && d.local == "Description") {
            continue;
        }
        if target.is_none() {
            target = Some(Target { open: d.open.clone(), close: d.close.clone(), qname: d.qname.clone(), scope: d.scope.clone() });
        }
        for a in &d.attrs {
            if a.qname == "xmlns" || a.qname.starts_with("xmlns:") || a.uri.is_empty() || a.uri == NS_RDF || a.uri == NS_XML {
                continue;
            }
            props.push(Prop {
                ns: a.uri.clone(),
                name: a.local.clone(),
                value: a.value.clone(),
                form: Form::Attr { full: a.full.clone(), inner: a.inner.clone(), quote: a.quote },
            });
        }
        for k in &d.kids {
            let Kid::El(c) = k else { continue };
            if c.uri == NS_RDF || c.uri.is_empty() {
                continue;
            }
            let (value, form) = classify(s, c);
            props.push(Prop { ns: c.uri.clone(), name: c.local.clone(), value, form });
        }
    }
    (props, target)
}

fn escape_text(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attr(v: &str, quote: u8) -> String {
    let e = escape_text(v);
    if quote == b'"' { e.replace('"', "&quot;") } else { e.replace('\'', "&apos;") }
}

fn check_value(v: &str) -> Result<(), String> {
    match v.chars().find(|c| (*c < ' ' && !matches!(c, '\t' | '\n' | '\r')) || matches!(c, '\u{fffe}' | '\u{ffff}')) {
        Some(c) => Err(format!("a value cannot hold the control character U+{:04X}", c as u32)),
        None => Ok(()),
    }
}

fn check_name(name: &str) -> Result<(), String> {
    let mut ch = name.chars();
    let ok = ch.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && ch.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if ok { Ok(()) } else { Err(format!("{name:?} is not a property name")) }
}

fn check_uri(uri: &str) -> Result<(), String> {
    if uri.is_empty() || uri.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '\'' | '&')) {
        return Err(format!("{uri:?} is not a namespace URI or a prefix of this packet"));
    }
    Ok(())
}

/// Splices applied from the back so that the earlier ranges stay valid.
fn apply(raw: &str, mut splices: Vec<(Range<usize>, String)>) -> String {
    splices.sort_by_key(|a| std::cmp::Reverse(a.0.start));
    let mut out = raw.to_string();
    for (r, text) in splices {
        out.replace_range(r, &text);
    }
    out
}

impl Xmp {
    /// Read a `_PMX` chunk body. An error says why the packet cannot be read (the caller keeps
    /// the bytes as they are).
    pub fn parse(bytes: &[u8]) -> Result<Xmp, String> {
        if bytes.len() as u64 > MAX_XMP_BYTES {
            return Err("the packet is larger than 4 MiB".to_string());
        }
        let raw = std::str::from_utf8(bytes).map_err(|_| "the packet is not UTF-8 text".to_string())?.to_string();
        let root = parse_document(&raw)?;
        if find_rdf(&root, 0).is_none() {
            return Err("the packet has no rdf:RDF element".to_string());
        }
        let (props, target) = collect(&raw, &root);
        let tail = raw.trim_end_matches(['\0', ' ', '\r', '\n', '\t']);
        let read_only =
            tail.rfind("<?xpacket end=").is_some_and(|i| tail[i..].starts_with("<?xpacket end=\"r\"") || tail[i..].starts_with("<?xpacket end='r'"));
        Ok(Xmp { raw, props, target, read_only })
    }

    /// A packet with no properties, for a file that has none.
    pub fn empty() -> Xmp {
        let raw = "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  <rdf:Description rdf:about=\"\">\n  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>";
        match Xmp::parse(raw.as_bytes()) {
            Ok(x) => x,
            Err(_) => Xmp { raw: raw.to_string(), props: Vec::new(), target: None, read_only: false },
        }
    }

    /// The chunk body.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.raw.as_bytes().to_vec()
    }

    /// The packet's text.
    pub fn text(&self) -> &str {
        &self.raw
    }

    /// Every property, in packet order.
    pub fn properties(&self) -> Vec<Property> {
        self.props
            .iter()
            .map(|p| Property {
                ns: p.ns.clone(),
                name: p.name.clone(),
                value: p.value.clone(),
                known: matches!(p.ns.as_str(), NS_DC | NS_XMP | NS_XMPDM),
                editable: !matches!(p.form, Form::Opaque { .. }),
            })
            .collect()
    }

    /// The value of a property; `None` when the packet has none by that name.
    pub fn get(&self, ns: &str, name: &str) -> Option<String> {
        self.props.iter().find(|p| p.ns == ns && p.name == name).map(|p| p.value.clone())
    }

    /// The namespace a prefix stands for in this packet, or one of the well-known ones.
    pub fn resolve_prefix(&self, prefix: &str) -> Option<String> {
        if let Some(t) = &self.target
            && let Some(u) = lookup(&t.scope, prefix)
        {
            return Some(u.to_string());
        }
        well_known_prefix(prefix).map(str::to_string)
    }

    /// Split `<ns>:<prop>` where `<ns>` is a namespace URI or a prefix.
    pub fn split_key(&self, key: &str) -> Result<(String, String), String> {
        let (ns, name) = key.rsplit_once(':').ok_or("a property is written <namespace>:<name>, for example dc:title")?;
        check_name(name)?;
        let uri = self.resolve_prefix(ns).unwrap_or_else(|| ns.to_string());
        if !uri.contains(':') && !uri.contains('/') {
            return Err(format!("no namespace prefix {ns:?} in this packet"));
        }
        check_uri(&uri)?;
        Ok((uri, name.to_string()))
    }

    /// Set a property to `value` (empty removes it). Returns whether the packet changed. Only
    /// that property's bytes change, and the result is parsed again and checked. Errors leave
    /// the packet as it was.
    pub fn set(&mut self, ns: &str, name: &str, value: &str) -> Result<bool, String> {
        check_name(name)?;
        check_uri(ns)?;
        check_value(value)?;
        let current = self.get(ns, name);
        if (!value.is_empty() && current.as_deref() == Some(value)) || (value.is_empty() && current.is_none()) {
            return Ok(false);
        }
        if self.read_only {
            return Err("the packet is marked read-only (xpacket end=\"r\")".to_string());
        }
        let splices = match self.props.iter().find(|p| p.ns == ns && p.name == name) {
            Some(p) => self.splices_existing(p, value)?,
            None => self.splices_new(ns, name, value)?,
        };
        let raw = apply(&self.raw, splices);
        let mut next = Xmp::parse(raw.as_bytes()).map_err(|e| format!("the edit would leave a packet that cannot be read ({e}); nothing changed"))?;
        let others = |x: &Xmp| -> Vec<(String, String, String)> {
            x.props.iter().filter(|p| !(p.ns == ns && p.name == name)).map(|p| (p.ns.clone(), p.name.clone(), p.value.clone())).collect()
        };
        if others(&next) != others(self) {
            return Err("the edit would change another property; nothing changed".to_string());
        }
        if next.get(ns, name).unwrap_or_default() != value
            && !(matches!(kind_for(ns, name), Some(ArrKind::Seq | ArrKind::Bag)) || self.is_list(ns, name))
        {
            return Err("the edit did not take; nothing changed".to_string());
        }
        next.read_only = self.read_only;
        *self = next;
        Ok(true)
    }

    fn is_list(&self, ns: &str, name: &str) -> bool {
        self.props.iter().any(|p| p.ns == ns && p.name == name && matches!(&p.form, Form::Array { kind, .. } if *kind != ArrKind::Alt))
    }

    fn removal(&self, el: &Range<usize>) -> Range<usize> {
        let b = self.raw.as_bytes();
        let mut s = el.start;
        while s > 0 && matches!(b[s - 1], b' ' | b'\t') {
            s -= 1;
        }
        if s > 0 && b[s - 1] == b'\n' {
            s -= 1;
            if s > 0 && b[s - 1] == b'\r' {
                s -= 1;
            }
        }
        s..el.end
    }

    fn splices_existing(&self, p: &Prop, value: &str) -> Result<Vec<(Range<usize>, String)>, String> {
        let removing = value.is_empty();
        Ok(match &p.form {
            Form::Opaque { .. } => return Err(format!("{} has a structured value; it is kept as it is", p.name)),
            Form::Attr { full, inner, quote } => {
                if removing {
                    vec![(full.clone(), String::new())]
                } else {
                    vec![(inner.clone(), escape_attr(value, *quote))]
                }
            }
            Form::Text { el, inner } => {
                if removing {
                    vec![(self.removal(el), String::new())]
                } else if let Some(inner) = inner {
                    vec![(inner.clone(), escape_text(value))]
                } else {
                    let q = self.raw.get(el.start + 1..el.end - 2).unwrap_or_default().trim_end().to_string();
                    let qname = q.split(|c: char| c.is_whitespace()).next().unwrap_or_default().to_string();
                    let open = format!("<{q}>");
                    vec![(el.clone(), format!("{open}{}</{qname}>", escape_text(value)))]
                }
            }
            Form::Array { el, kind, cont, cont_inner, cont_name, items } => {
                if removing {
                    return Ok(vec![(self.removal(el), String::new())]);
                }
                let li = li_name(cont_name);
                match kind {
                    ArrKind::Alt => {
                        let at = items.iter().position(|i| i.lang.as_deref() == Some("x-default")).or(if items.is_empty() { None } else { Some(0) });
                        match at {
                            Some(n) => vec![(items[n].inner.clone(), escape_text(value))],
                            None => {
                                replace_container(cont, cont_inner, cont_name, &format!("<{li} xml:lang=\"x-default\">{}</{li}>", escape_text(value)))
                            }
                        }
                    }
                    ArrKind::Seq | ArrKind::Bag => replace_container(cont, cont_inner, cont_name, &list_items(&li, value)),
                }
            }
        })
    }

    fn splices_new(&self, ns: &str, name: &str, value: &str) -> Result<Vec<(Range<usize>, String)>, String> {
        if value.is_empty() {
            return Ok(Vec::new());
        }
        let t = self.target.as_ref().ok_or("the packet has no rdf:Description to add a property to")?;
        // The prefix: one the packet already binds to this namespace, else a conventional one.
        let bound = t.scope.iter().rev().find(|(p, u)| !p.is_empty() && u == ns).map(|(p, _)| p.clone());
        let (prefix, declare) = match bound {
            Some(p) => (p, false),
            None => {
                let mut want = preferred_prefix(ns).map_or_else(|| "ns1".to_string(), str::to_string);
                let mut n = 1;
                while lookup(&t.scope, &want).is_some_and(|u| u != ns) {
                    n += 1;
                    want = format!("ns{n}");
                }
                (want, true)
            }
        };
        let rdf_prefix = t.qname.split_once(':').map_or("", |(p, _)| p);
        let li = if rdf_prefix.is_empty() { "li".to_string() } else { format!("{rdf_prefix}:li") };
        let cont = |k: &str| if rdf_prefix.is_empty() { k.to_string() } else { format!("{rdf_prefix}:{k}") };
        let q = format!("{prefix}:{name}");
        let el = match kind_for(ns, name) {
            Some(ArrKind::Alt) => format!("<{q}><{c}><{li} xml:lang=\"x-default\">{v}</{li}></{c}></{q}>", c = cont("Alt"), v = escape_text(value)),
            Some(k) => {
                let c = cont(if k == ArrKind::Seq { "Seq" } else { "Bag" });
                format!("<{q}><{c}>{}</{c}></{q}>", list_items(&li, value))
            }
            None => format!("<{q}>{}</{q}>", escape_text(value)),
        };
        let decl = if declare { format!(" xmlns:{prefix}=\"{ns}\"") } else { String::new() };
        let mut out = Vec::new();
        match &t.close {
            Some(close) => {
                if declare {
                    out.push((t.open.end - 1..t.open.end - 1, decl));
                }
                let b = self.raw.as_bytes();
                let mut s = close.start;
                while s > 0 && matches!(b[s - 1], b' ' | b'\t') {
                    s -= 1;
                }
                if s > 0 && b[s - 1] == b'\n' {
                    let indent = &self.raw[s..close.start];
                    out.push((s..s, format!("{indent}  {el}\n")));
                } else {
                    out.push((close.start..close.start, el));
                }
            }
            None => {
                let open = &self.raw[t.open.clone()];
                let head = open.strip_suffix("/>").ok_or("a malformed rdf:Description")?.trim_end();
                out.push((t.open.clone(), format!("{head}{decl}>{el}</{}>", t.qname)));
            }
        }
        Ok(out)
    }
}

fn li_name(cont_name: &str) -> String {
    cont_name.split_once(':').map_or("li".to_string(), |(p, _)| format!("{p}:li"))
}

fn list_items(li: &str, value: &str) -> String {
    value.split(';').map(str::trim).filter(|v| !v.is_empty()).map(|v| format!("<{li}>{}</{li}>", escape_text(v))).collect()
}

fn replace_container(cont: &Range<usize>, inner: &Option<Range<usize>>, name: &str, items: &str) -> Vec<(Range<usize>, String)> {
    match inner {
        Some(i) => vec![(i.clone(), items.to_string())],
        None => vec![(cont.clone(), format!("<{name}>{items}</{name}>"))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKET: &str = concat!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n",
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Some Toolkit\">\n",
        " <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n",
        "  <!-- a comment that must stay -->\n",
        "  <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n",
        "    xmlns:xmpDM=\"http://ns.adobe.com/xmp/1.0/DynamicMedia/\" xmlns:acme=\"http://example.com/acme/1.0/\"\n",
        "    xmpDM:artist=\"The Band\" acme:take=\"7\">\n",
        "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Old &amp; title</rdf:li></rdf:Alt></dc:title>\n",
        "   <dc:creator><rdf:Seq><rdf:li>Ann</rdf:li><rdf:li>Bo</rdf:li></rdf:Seq></dc:creator>\n",
        "   <acme:mood>calm</acme:mood>\n",
        "   <acme:shape rdf:parseType=\"Resource\"><acme:w>3</acme:w></acme:shape>\n",
        "   <xmpDM:album>First</xmpDM:album>\n",
        "  </rdf:Description>\n",
        " </rdf:RDF>\n",
        "</x:xmpmeta>\n",
        "                    \n",
        "<?xpacket end=\"w\"?>"
    );
    const ACME: &str = "http://example.com/acme/1.0/";

    fn packet() -> Xmp {
        Xmp::parse(PACKET.as_bytes()).unwrap()
    }

    #[test]
    fn reads_known_and_unknown_properties() {
        let x = packet();
        assert_eq!(x.get(NS_DC, "title").as_deref(), Some("Old & title"));
        assert_eq!(x.get(NS_DC, "creator").as_deref(), Some("Ann; Bo"));
        assert_eq!(x.get(NS_XMPDM, "artist").as_deref(), Some("The Band"));
        assert_eq!(x.get(NS_XMPDM, "album").as_deref(), Some("First"));
        assert_eq!(x.get(ACME, "take").as_deref(), Some("7"));
        assert_eq!(x.get(ACME, "mood").as_deref(), Some("calm"));
        let shape = x.properties().into_iter().find(|p| p.name == "shape").unwrap();
        assert!(!shape.editable && !shape.known);
        assert!(x.get(NS_DC, "rights").is_none());
        assert_eq!(x.to_bytes(), PACKET.as_bytes(), "an unedited packet writes back byte for byte");
    }

    #[test]
    fn an_edit_changes_only_that_property() {
        let mut x = packet();
        assert!(x.set(NS_DC, "title", "New <title>").unwrap());
        let expect = PACKET.replace("Old &amp; title", "New &lt;title&gt;");
        assert_eq!(x.text(), expect);
        let mut y = packet();
        y.set(NS_XMPDM, "artist", "Other \"Band\"").unwrap();
        assert_eq!(y.text(), PACKET.replace("xmpDM:artist=\"The Band\"", "xmpDM:artist=\"Other &quot;Band&quot;\""));
        let mut z = packet();
        z.set(ACME, "mood", "tense").unwrap();
        assert_eq!(z.text(), PACKET.replace(">calm<", ">tense<"));
        let mut c = packet();
        c.set(NS_DC, "creator", "Cy; Di; Ed").unwrap();
        assert_eq!(c.get(NS_DC, "creator").as_deref(), Some("Cy; Di; Ed"));
        assert!(c.text().contains("<rdf:Seq><rdf:li>Cy</rdf:li><rdf:li>Di</rdf:li><rdf:li>Ed</rdf:li></rdf:Seq>"));
        assert!(!x.set(NS_DC, "title", "New <title>").unwrap(), "the same value changes nothing");
    }

    #[test]
    fn adds_and_removes_properties() {
        let mut x = packet();
        x.set(NS_DC, "rights", "(c) me").unwrap();
        assert_eq!(x.get(NS_DC, "rights").as_deref(), Some("(c) me"));
        assert!(x.text().contains("<dc:rights><rdf:Alt><rdf:li xml:lang=\"x-default\">(c) me</rdf:li></rdf:Alt></dc:rights>"));
        x.set(NS_XMPDM, "genre", "Rock").unwrap();
        assert!(x.text().contains("<xmpDM:genre>Rock</xmpDM:genre>"));
        x.set("http://example.com/new/", "thing", "v").unwrap();
        assert!(x.text().contains("xmlns:ns1=\"http://example.com/new/\"") && x.text().contains("<ns1:thing>v</ns1:thing>"));
        x.set(ACME, "mood", "").unwrap();
        x.set(ACME, "take", "").unwrap();
        assert!(x.get(ACME, "mood").is_none() && x.get(ACME, "take").is_none());
        assert!(!x.text().contains("acme:mood") && !x.text().contains("acme:take"));
        assert_eq!(x.get(NS_DC, "title").as_deref(), Some("Old & title"));
        assert!(x.set(ACME, "shape", "x").is_err(), "a structured value is not edited");
        assert!(!x.set(NS_DC, "format", "").unwrap(), "removing what is not there is no change");
    }

    #[test]
    fn a_new_packet_takes_properties() {
        let mut x = Xmp::empty();
        assert!(x.properties().is_empty());
        x.set(NS_DC, "title", "Hello").unwrap();
        x.set(NS_DC, "creator", "A; B").unwrap();
        x.set(NS_XMPDM, "artist", "Z").unwrap();
        let y = Xmp::parse(&x.to_bytes()).unwrap();
        assert_eq!(y.get(NS_DC, "title").as_deref(), Some("Hello"));
        assert_eq!(y.get(NS_DC, "creator").as_deref(), Some("A; B"));
        assert_eq!(y.get(NS_XMPDM, "artist").as_deref(), Some("Z"));
    }

    #[test]
    fn a_self_closing_description_takes_a_property() {
        let p = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\"/></rdf:RDF></x:xmpmeta>";
        let mut x = Xmp::parse(p.as_bytes()).unwrap();
        x.set(NS_XMP, "CreatorTool", "WavCraft").unwrap();
        assert_eq!(Xmp::parse(&x.to_bytes()).unwrap().get(NS_XMP, "CreatorTool").as_deref(), Some("WavCraft"));
    }

    #[test]
    fn malformed_and_hostile_packets_are_refused_not_crashed() {
        let bad: [&[u8]; 9] = [
            b"",
            b"not xml at all",
            b"<a><b></a>",
            b"<?xml version=\"1.0\"?><!DOCTYPE x [<!ENTITY a \"b\">]><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>",
            b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description a=\"&boom;\"/></rdf:RDF></x:xmpmeta>",
            b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><y:z/></x:xmpmeta>",
            b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"></x:xmpmeta>",
            b"\xff\xfe<\0x\0>\0",
            b"<a x=\"1\" x=\"2\"/>",
        ];
        for b in bad {
            assert!(Xmp::parse(b).is_err(), "{:?}", String::from_utf8_lossy(b));
        }
        let deep = "<a>".repeat(200);
        assert!(Xmp::parse(deep.as_bytes()).is_err());
        // Chopped anywhere, a good packet never panics.
        for n in 0..PACKET.len() {
            if PACKET.is_char_boundary(n) {
                let _ = Xmp::parse(&PACKET.as_bytes()[..n]);
            }
        }
    }

    #[test]
    fn keys_resolve_prefixes_and_uris() {
        let x = packet();
        assert_eq!(x.split_key("dc:title").unwrap(), (NS_DC.to_string(), "title".to_string()));
        assert_eq!(x.split_key("acme:mood").unwrap(), (ACME.to_string(), "mood".to_string()));
        assert_eq!(x.split_key(&format!("{ACME}:mood")).unwrap().0, ACME);
        assert!(x.split_key("nope:mood").is_err());
        assert!(x.split_key("title").is_err());
        assert!(x.split_key("dc:bad name").is_err());
    }

    #[test]
    fn a_read_only_packet_is_not_edited() {
        let mut x = Xmp::parse(PACKET.replace("end=\"w\"", "end=\"r\"").as_bytes()).unwrap();
        assert!(x.set(NS_DC, "title", "x").is_err());
    }
}
