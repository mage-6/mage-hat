//! A small HTML tree with a tokenizer of its own.
//!
//! The tree keeps the original text of every start tag so that untouched
//! markup is written back byte for byte. Only elements MageHat has to change
//! (loops, conditions, components, slots) get their tags rebuilt.

use regex::Regex;
use std::sync::LazyLock;

pub const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];
/// Content of these elements is raw text: never interpolated, never parsed.
/// The one exception is a JSON-LD script, which is a template (see jsonld.rs).
pub const RAW: &[&str] = &["script", "style"];

pub fn is_json_ld(tag: &str, attrs: &[(String, Option<String>)]) -> bool {
    tag == "script" && attrs.iter().any(|(k, v)| k == "type" && v.as_deref().map_or(false, |v| v.trim().eq_ignore_ascii_case("application/ld+json")))
}

pub fn is_raw(tag: &str, attrs: &[(String, Option<String>)]) -> bool {
    RAW.contains(&tag) && !is_json_ld(tag, attrs)
}

const BLOCK: &[&str] = &[
    "address", "article", "aside", "blockquote", "div", "dl", "fieldset", "figure", "footer",
    "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "main", "nav", "ol", "p", "pre",
    "section", "table", "ul",
];

#[derive(Debug, Clone)]
pub enum Node {
    Text { text: String, line: usize },
    /// Comments, doctype, processing instructions: copied through verbatim.
    Raw { text: String, line: usize },
    Element(Element),
}

#[derive(Debug, Clone)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<(String, Option<String>)>,
    pub start_text: String,
    pub children: Vec<Node>,
    /// Had an explicit end tag in the source.
    pub closed: bool,
    pub line: usize,
}

impl Node {
    pub fn line(&self) -> usize {
        match self {
            Node::Text { line, .. } | Node::Raw { line, .. } => *line,
            Node::Element(e) => e.line,
        }
    }

    pub fn is_blank(&self) -> bool {
        matches!(self, Node::Text { text, .. } if text.trim().is_empty())
    }
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_deref().unwrap_or(""))
    }

    /// A JSON-LD script: parsed and rendered like markup, unlike other scripts.
    pub fn is_json_ld(&self) -> bool {
        is_json_ld(&self.tag, &self.attrs)
    }

    /// Raw text content: never interpolated, never parsed.
    pub fn is_raw(&self) -> bool {
        is_raw(&self.tag, &self.attrs)
    }

    pub fn has_attr(&self, name: &str) -> bool {
        self.attrs.iter().any(|(k, _)| k == name)
    }

    pub fn text_content(&self) -> String {
        let mut out = String::new();
        for c in &self.children {
            match c {
                Node::Text { text, .. } => out.push_str(text),
                Node::Element(e) => out.push_str(&e.text_content()),
                Node::Raw { .. } => {}
            }
        }
        out
    }
}

/// Which open element a start tag implicitly closes.
fn implicitly_closes(tag: &str, open: &str) -> bool {
    match tag {
        "li" => open == "li",
        "dt" | "dd" => matches!(open, "dt" | "dd"),
        "tr" => matches!(open, "tr" | "td" | "th"),
        "td" | "th" => matches!(open, "td" | "th"),
        "option" => open == "option",
        "thead" | "tbody" | "tfoot" => matches!(open, "thead" | "tbody" | "tfoot" | "tr" | "td" | "th") && open != tag,
        _ => open == "p" && BLOCK.contains(&tag),
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    line: usize,
    /// Stack of open elements as paths into the tree being built.
    stack: Vec<Element>,
    root: Vec<Node>,
}

impl<'a> Parser<'a> {
    fn push_node(&mut self, node: Node) {
        match self.stack.last_mut() {
            Some(parent) => parent.children.push(node),
            None => self.root.push(node),
        }
    }

    fn advance(&mut self, to: usize) -> &'a str {
        let s = &self.src[self.pos..to];
        self.line += s.matches('\n').count();
        self.pos = to;
        s
    }

    fn close_top(&mut self, closed: bool) {
        if let Some(mut el) = self.stack.pop() {
            el.closed = closed;
            self.push_node(Node::Element(el));
        }
    }

    fn run(mut self) -> Vec<Node> {
        while self.pos < self.src.len() {
            let rest = &self.src[self.pos..];
            if !rest.starts_with('<') {
                // `rest` does not start with '<' here, so the next '<' can never be at
                // offset 0 and there is nothing to skip. Slicing `rest[1..]` to skip it
                // panicked whenever the text began with a multi-byte character, because
                // byte 1 is inside that character rather than on a boundary.
                let next = rest.find('<').map(|i| self.pos + i).unwrap_or(self.src.len());
                let line = self.line;
                let text = self.advance(next).to_string();
                self.push_node(Node::Text { text, line });
                continue;
            }
            let line = self.line;
            if let Some(stripped) = rest.strip_prefix("<!--") {
                let end = stripped.find("-->").map(|i| self.pos + 4 + i + 3).unwrap_or(self.src.len());
                let text = self.advance(end).to_string();
                self.push_node(Node::Raw { text, line });
            } else if rest.starts_with("<!") || rest.starts_with("<?") {
                let end = rest.find('>').map(|i| self.pos + i + 1).unwrap_or(self.src.len());
                let text = self.advance(end).to_string();
                self.push_node(Node::Raw { text, line });
            } else if let Some(stripped) = rest.strip_prefix("</") {
                let name_len = tag_name_len(stripped);
                let end = rest.find('>').map(|i| self.pos + i + 1);
                match (name_len, end) {
                    (n, Some(end)) if n > 0 => {
                        let name = stripped[..n].to_ascii_lowercase();
                        self.advance(end);
                        self.end_tag(&name, line);
                    }
                    _ => {
                        let text = self.advance(self.pos + 1).to_string();
                        self.push_node(Node::Text { text, line });
                    }
                }
            } else {
                let name_len = tag_name_len(&rest[1..]);
                if name_len == 0 {
                    let text = self.advance(self.pos + 1).to_string();
                    self.push_node(Node::Text { text, line });
                    continue;
                }
                match scan_start_tag(rest) {
                    Some((end, attrs, self_closing)) => {
                        let tag = rest[1..1 + name_len].to_ascii_lowercase();
                        let start_text = self.advance(self.pos + end).to_string();
                        self.start_tag(tag, attrs, start_text, self_closing, line);
                    }
                    None => {
                        let text = self.advance(self.src.len()).to_string();
                        self.push_node(Node::Text { text, line });
                    }
                }
            }
        }
        while !self.stack.is_empty() {
            self.close_top(false);
        }
        self.root
    }

    fn start_tag(&mut self, tag: String, attrs: Vec<(String, Option<String>)>, start_text: String,
                 self_closing: bool, line: usize) {
        if let Some(top) = self.stack.last() {
            if implicitly_closes(&tag, &top.tag) {
                self.close_top(false);
            }
        }
        let raw = is_raw(&tag, &attrs);
        let el = Element { tag: tag.clone(), attrs, start_text, children: Vec::new(), closed: false, line };
        if self_closing || VOID.contains(&tag.as_str()) {
            self.push_node(Node::Element(el));
            return;
        }
        self.stack.push(el);
        if raw {
            // Raw text until the matching end tag, case-insensitive.
            let rest = &self.src[self.pos..];
            let lower = rest.to_ascii_lowercase();
            let needle = format!("</{tag}");
            let end = lower.find(&needle).map(|i| self.pos + i).unwrap_or(self.src.len());
            let text_line = self.line;
            let text = self.advance(end).to_string();
            if !text.is_empty() {
                self.push_node(Node::Text { text, line: text_line });
            }
        }
    }

    fn end_tag(&mut self, name: &str, line: usize) {
        if let Some(idx) = self.stack.iter().rposition(|e| e.tag == name) {
            while self.stack.len() > idx + 1 {
                self.close_top(false);
            }
            self.close_top(true);
        } else {
            // Stray end tag: keep it as text so nothing is silently dropped.
            self.push_node(Node::Text { text: format!("</{name}>"), line });
        }
    }
}

fn tag_name_len(s: &str) -> usize {
    let mut n = 0;
    for (i, ch) in s.char_indices() {
        if i == 0 && !ch.is_ascii_alphabetic() {
            return 0;
        }
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == ':' || ch == '_' || ch == '.' {
            n = i + ch.len_utf8();
        } else {
            break;
        }
    }
    n
}

/// Scan a start tag beginning at `<`. Returns (byte length, attrs, self_closing).
pub fn scan_start_tag(s: &str) -> Option<(usize, Vec<(String, Option<String>)>, bool)> {
    let bytes = s.as_bytes();
    let mut i = 1 + tag_name_len(&s[1..]);
    let mut attrs = Vec::new();
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'>' {
            return Some((i + 1, attrs, false));
        }
        if bytes[i] == b'/' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'>' {
                return Some((i + 1, attrs, true));
            }
            continue;
        }
        // attribute name
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        if i == start {
            i += 1;
            continue;
        }
        let name = s[start..i].to_ascii_lowercase();
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'=' {
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= bytes.len() {
                return None;
            }
            let value = if bytes[j] == b'"' || bytes[j] == b'\'' {
                let q = bytes[j];
                let vstart = j + 1;
                let vend = s[vstart..].find(q as char).map(|k| vstart + k)?;
                i = vend + 1;
                &s[vstart..vend]
            } else {
                let vstart = j;
                while j < bytes.len() && !bytes[j].is_ascii_whitespace() && bytes[j] != b'>' {
                    j += 1;
                }
                i = j;
                &s[vstart..j]
            };
            attrs.push((name, Some(unescape(value))));
        } else {
            attrs.push((name, None));
        }
    }
}

/// Decode the entities that matter in attribute values.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    static ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);").unwrap());
    ENTITY
        .replace_all(s, |c: &regex::Captures| {
            let e = &c[1];
            let decoded = if let Some(hex) = e.strip_prefix("#x") {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map(|ch| ch.to_string())
            } else if let Some(dec) = e.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32).map(|ch| ch.to_string())
            } else {
                match e {
                    "amp" => Some("&".into()),
                    "lt" => Some("<".into()),
                    "gt" => Some(">".into()),
                    "quot" => Some("\"".into()),
                    "apos" => Some("'".into()),
                    "nbsp" => Some("\u{a0}".into()),
                    _ => None,
                }
            };
            decoded.unwrap_or_else(|| c[0].to_string())
        })
        .into_owned()
}

/// Parse an HTML document or fragment into a list of top-level nodes.
pub fn parse(text: &str) -> Vec<Node> {
    Parser { src: text, pos: 0, line: 1, stack: Vec::new(), root: Vec::new() }.run()
}

pub fn serialize(nodes: &[Node]) -> String {
    let mut out = String::new();
    serialize_into(nodes, &mut out);
    out
}

fn serialize_into(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text { text, .. } | Node::Raw { text, .. } => out.push_str(text),
            Node::Element(e) => {
                out.push_str(&e.start_text);
                serialize_into(&e.children, out);
                if e.closed {
                    out.push_str("</");
                    out.push_str(&e.tag);
                    out.push('>');
                }
            }
        }
    }
}

static ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\s+([^\s=/>"']+)(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?"#).unwrap());

/// Remove the named attributes from a raw start tag, leaving the rest as written.
pub fn strip_attrs(start_text: &str, names: &[&str]) -> String {
    if names.is_empty() {
        return start_text.to_string();
    }
    let head_len = 1 + tag_name_len(&start_text[1..]);
    let (head, rest) = start_text.split_at(head_len);
    let stripped = ATTR_RE.replace_all(rest, |c: &regex::Captures| {
        if names.contains(&c[1].to_ascii_lowercase().as_str()) {
            String::new()
        } else {
            c[0].to_string()
        }
    });
    format!("{head}{stripped}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_untouched_markup() {
        let src = "<!doctype html>\n<html lang=\"en\"><head><title>T &amp; U</title><style>a{color:red}</style></head>\n<body>\n<ul>\n<li each=\"x in xs\">{{ x }}\n<li>b</li></ul><x-card title=\"a\" /><img src=x><!-- c --><p>one<p>two</body></html>";
        assert_eq!(serialize(&parse(src)), src);
    }

    #[test]
    fn strips_attributes() {
        assert_eq!(strip_attrs("<li each=\"x in xs\" class=\"a\" if='y'>", &["each", "if"]), "<li class=\"a\">");
        assert_eq!(strip_attrs("<x-card title=\"a\" />", &["each"]), "<x-card title=\"a\" />");
    }

    #[test]
    fn attribute_values_are_unescaped_and_lines_counted() {
        let nodes = parse("<p>\n\n<a href=\"?a=1&amp;b=2\" x>t</a>");
        let Node::Element(p) = &nodes[0] else { panic!() };
        let Node::Element(a) = &p.children[1] else { panic!() };
        assert_eq!(a.attr("href"), Some("?a=1&b=2"));
        assert_eq!(a.attr("x"), Some(""));
        assert_eq!(a.line, 3);
    }

    #[test]
    fn raw_elements_keep_content() {
        let src = "<script>if (a < b) { x = '</div>' }</script><p>ok";
        assert_eq!(serialize(&parse(src)), src);
    }
}
