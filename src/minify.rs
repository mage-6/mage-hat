//! Deterministic, conservative minification.
//!
//! HTML: comments go, runs of whitespace shrink to one character, and
//! nothing inside <pre>, <textarea>, <script>, <style> or a tag's attributes
//! is touched. A whitespace run is never removed outright, only shortened,
//! so inline rendering is identical.
//! CSS: comments go, whitespace shrinks, and the spaces that are safe to drop
//! around punctuation are dropped. Strings and url() stay verbatim.

const HTML_RAW: &[&str] = &["pre", "textarea", "script", "style"];

pub fn html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut raw_until: Option<String> = None; // closing tag we are waiting for
    let mut pending_ws: Option<char> = None; // ' ' or '\n' waiting to be written

    while i < bytes.len() {
        if let Some(close) = &raw_until {
            let rest = &input[i..];
            let end = find_ci(rest, close).unwrap_or(rest.len());
            out.push_str(&rest[..end]);
            i += end;
            raw_until = None;
            continue;
        }
        let c = bytes[i];
        if c == b'<' {
            let rest = &input[i..];
            if rest.starts_with("<!--") && !rest.starts_with("<!--[") {
                let end = rest.find("-->").map(|e| e + 3).unwrap_or(rest.len());
                i += end;
                continue;
            }
            flush_ws(&mut out, &mut pending_ws);
            let end = tag_end(rest);
            let tag = &rest[..end];
            out.push_str(tag);
            i += end;
            let name = tag_name(tag);
            if HTML_RAW.contains(&name.as_str()) {
                raw_until = Some(format!("</{name}"));
            }
            continue;
        }
        if c.is_ascii_whitespace() {
            if c == b'\n' || pending_ws.is_none() {
                pending_ws = Some(if c == b'\n' || pending_ws == Some('\n') { '\n' } else { ' ' });
            }
            i += 1;
            continue;
        }
        flush_ws(&mut out, &mut pending_ws);
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'<' {
            i += 1;
        }
        out.push_str(&input[start..i]);
    }
    flush_ws(&mut out, &mut pending_ws);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn flush_ws(out: &mut String, pending: &mut Option<char>) {
    if let Some(ws) = pending.take() {
        if !out.is_empty() {
            out.push(ws);
        }
    }
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Length of the tag starting at `<`, respecting quoted attribute values.
fn tag_end(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 1;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        match (quote, b[i]) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, b'"') | (None, b'\'') => quote = Some(b[i]),
            (None, b'>') => return i + 1,
            _ => {}
        }
        i += 1;
    }
    b.len()
}

fn tag_name(tag: &str) -> String {
    tag.trim_start_matches('<')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_ascii_lowercase()
}

pub fn css(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let b = input.as_bytes();
    let mut i = 0;
    let mut pending_space = false;
    while i < b.len() {
        let c = b[i];
        // comments
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let end = input[i + 2..].find("*/").map(|e| i + 2 + e + 2).unwrap_or(b.len());
            i = end;
            continue;
        }
        // strings and url(...) verbatim
        if c == b'"' || c == b'\'' {
            let end = input[i + 1..].find(c as char).map(|e| i + 1 + e + 1).unwrap_or(b.len());
            space_if_needed(&mut out, &mut pending_space);
            out.push_str(&input[i..end]);
            i = end;
            continue;
        }
        if input[i..].starts_with("url(") {
            let end = input[i..].find(')').map(|e| i + e + 1).unwrap_or(b.len());
            space_if_needed(&mut out, &mut pending_space);
            out.push_str(&input[i..end]);
            i = end;
            continue;
        }
        if c.is_ascii_whitespace() {
            pending_space = true;
            i += 1;
            continue;
        }
        if matches!(c, b'{' | b'}' | b';' | b',' | b'>') {
            pending_space = false;
        }
        space_if_needed(&mut out, &mut pending_space);
        if c == b'}' {
            while out.ends_with(';') {
                out.pop();
            }
        }
        out.push(c as char);
        i += 1;
    }
    out.trim().to_string()
}

/// Emit one space for a pending whitespace run, unless the previous
/// character makes it redundant.
fn space_if_needed(out: &mut String, pending: &mut bool) {
    if *pending && !out.is_empty() && !matches!(out.as_bytes().last(), Some(b'{' | b'}' | b';' | b',' | b':' | b'>')) {
        out.push(' ');
    }
    *pending = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_collapses_but_never_removes_whitespace() {
        let src = "<!doctype html>\n<html>\n  <!-- gone -->\n  <body>\n    <a>x</a>\n    <a>y</a>   <b>z</b>\n    <pre>  keep\n   this </pre>\n    <p title=\"a  b\">t</p>\n  </body>\n</html>\n";
        assert_eq!(html(src), "<!doctype html>\n<html>\n<body>\n<a>x</a>\n<a>y</a> <b>z</b>\n<pre>  keep\n   this </pre>\n<p title=\"a  b\">t</p>\n</body>\n</html>\n");
    }

    #[test]
    fn css_is_compact_and_safe() {
        let src = "/* c */\nx-card { display: contents; }\n@scope (x-card) to (:scope :is(a, b) > *) {\n  .card > a:hover , .x { padding: 1rem ; background: url( /a b.png ); content: \"a  b\"; }\n}\n";
        assert_eq!(css(src), "x-card{display:contents}@scope (x-card) to (:scope :is(a,b)>*){.card>a:hover,.x{padding:1rem;background:url( /a b.png );content:\"a  b\"}}");
    }
}
