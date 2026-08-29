//! Checks on markup that a build alone would let through: unclosed or stray
//! tags in source files, and duplicate ids, images without alt text and
//! anchors without href in the output.

use crate::htmltree::{Node, VOID};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Elements whose end tag HTML lets you omit.
const OPTIONAL_END: &[&str] = &[
    "html", "head", "body", "li", "dt", "dd", "p", "tr", "td", "th", "thead", "tbody", "tfoot", "option", "optgroup",
    "colgroup", "caption", "rt", "rp",
];

static STRAY_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^</([a-zA-Z][a-zA-Z0-9-]*)>$").unwrap());
static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<[a-z][^>]*\sid\s*=\s*["']([^"']+)["']"#).unwrap());
static IMG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<img\b[^>]*>").unwrap());
static ALT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\salt\s*=").unwrap());
static ANCHOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<a\b[^>]*>").unwrap());
static HREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\shref\s*=").unwrap());

pub struct Finding {
    pub line: Option<usize>,
    pub message: String,
    pub fix: String,
}

/// Problems in a parsed source file (a page or a component template).
pub fn lint_source(nodes: &[Node]) -> Vec<Finding> {
    let mut out = Vec::new();
    walk(nodes, &mut out);
    out
}

fn walk(nodes: &[Node], out: &mut Vec<Finding>) {
    for n in nodes {
        match n {
            Node::Element(e) => {
                if !e.closed && !VOID.contains(&e.tag.as_str()) && !OPTIONAL_END.contains(&e.tag.as_str()) && !e.start_text.ends_with("/>") {
                    out.push(Finding {
                        line: Some(e.line),
                        message: format!("<{}> is never closed", e.tag),
                        fix: format!("add </{}> where the element ends; unclosed tags swallow everything after them", e.tag),
                    });
                }
                walk(&e.children, out);
            }
            Node::Text { text, line } => {
                if let Some(m) = STRAY_END.captures(text.trim()) {
                    out.push(Finding {
                        line: Some(*line),
                        message: format!("stray </{}> with no open <{}>", &m[1], &m[1]),
                        fix: "remove it, or add the missing opening tag".into(),
                    });
                }
            }
            Node::Raw { .. } => {}
        }
    }
}

/// Problems in a rendered page.
pub fn lint_output(html: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut ids: HashMap<String, usize> = HashMap::new();
    for m in ID.captures_iter(html) {
        *ids.entry(m[1].to_string()).or_default() += 1;
    }
    let mut dups: Vec<(&String, &usize)> = ids.iter().filter(|(_, n)| **n > 1).collect();
    dups.sort();
    for (id, n) in dups {
        out.push(Finding {
            line: None,
            message: format!("id \"{id}\" appears {n} times"),
            fix: "ids must be unique on a page; a component used more than once with a fixed id is the usual cause".into(),
        });
    }
    let no_alt = IMG.find_iter(html).filter(|m| !ALT.is_match(m.as_str())).count();
    if no_alt > 0 {
        out.push(Finding {
            line: None,
            message: format!("{no_alt} <img> without alt"),
            fix: "add alt=\"what the image shows\", or alt=\"\" for a purely decorative image".into(),
        });
    }
    let no_href = ANCHOR.find_iter(html).filter(|m| !HREF.is_match(m.as_str())).count();
    if no_href > 0 {
        out.push(Finding {
            line: None,
            message: format!("{no_href} <a> without href"),
            fix: "add href, or use <button> for something that is not a link".into(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::htmltree::parse;

    #[test]
    fn finds_unclosed_and_stray_tags() {
        let f = lint_source(&parse("<div>\n<p>ok<ul><li>a<li>b</ul>\n<span>x</div>\n</section>"));
        let messages: Vec<&str> = f.iter().map(|x| x.message.as_str()).collect();
        assert_eq!(messages, vec!["<span> is never closed", "stray </section> with no open <section>"]);
        assert_eq!(f[0].line, Some(3));
    }

    #[test]
    fn finds_output_problems() {
        let f = lint_output("<p id=\"a\"></p><p id=\"a\"></p><img src=x><img src=y alt=\"\"><a>no</a><a href=\"/\">ok</a>");
        let messages: Vec<&str> = f.iter().map(|x| x.message.as_str()).collect();
        assert_eq!(messages, vec!["id \"a\" appears 2 times", "1 <img> without alt", "1 <a> without href"]);
    }
}
