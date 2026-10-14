//! Heading anchors and the table of contents of a content item.
//!
//! Every h2 and h3 in an item's body gets an id, so a page can link to its
//! sections, and the item exposes them in order as `toc`:
//!
//!     ## 1. Só para um adulto      ->  <h2 id="1-so-para-um-adulto">
//!     toc: [{ id: "1-so-para-um-adulto", text: "1. Só para um adulto", level: 2 }]
//!
//! The id is the heading's plain text slugged by the same rule as archive
//! terms (`term_slug`: accents folded, lowercase, anything else a single
//! hyphen), so there is one slug rule in MageHat. Leading numbering stays in
//! the slug. A repeat within the item gets -2, -3, skipping any id already
//! in the body; a heading the author gave an id keeps it. Only h2 and h3:
//! an h1 is the page title and deeper levels are too fine for a contents box.

use crate::htmltree::{unescape, Element, Node};
use crate::pages::term_slug;
use crate::values::{Map, Value};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct TocEntry {
    pub id: String,
    pub text: String,
    pub level: u8,
}

impl TocEntry {
    pub fn value(&self) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), Value::str(&self.id));
        m.insert("text".into(), Value::str(&self.text));
        m.insert("level".into(), Value::Int(self.level as i64));
        Value::map(m)
    }
}

/// Give every h2 and h3 without an id one, in place, and list them in order.
pub fn anchor_headings(nodes: &mut [Node]) -> Vec<TocEntry> {
    let mut taken = HashSet::new();
    collect_ids(nodes, &mut taken);
    let mut toc = Vec::new();
    walk(nodes, &mut taken, &mut toc);
    toc
}

fn collect_ids(nodes: &[Node], out: &mut HashSet<String>) {
    for n in nodes {
        if let Node::Element(e) = n {
            if let Some(id) = e.attr("id") {
                out.insert(id.to_string());
            }
            collect_ids(&e.children, out);
        }
    }
}

fn walk(nodes: &mut [Node], taken: &mut HashSet<String>, toc: &mut Vec<TocEntry>) {
    for n in nodes {
        let Node::Element(e) = n else { continue };
        let level = match e.tag.as_str() {
            "h2" => 2,
            "h3" => 3,
            _ => {
                if !e.is_raw() {
                    walk(&mut e.children, taken, toc);
                }
                continue;
            }
        };
        let text = plain_text(e);
        let id = match e.attr("id") {
            Some(id) => id.to_string(),
            None => {
                let id = unique(&text, taken);
                set_id(e, &id);
                id
            }
        };
        toc.push(TocEntry { id, text, level });
    }
}

/// The heading as a reader sees it: tags dropped, entities decoded,
/// whitespace runs collapsed.
fn plain_text(e: &Element) -> String {
    unescape(&e.text_content()).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unique(text: &str, taken: &mut HashSet<String>) -> String {
    let mut base = term_slug(text);
    if base.is_empty() {
        base = "section".into(); // a heading of symbols only
    }
    let mut id = base.clone();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    taken.insert(id.clone());
    id
}

/// Write the id into the start tag right after the tag name, leaving the
/// rest of the tag as written.
fn set_id(e: &mut Element, id: &str) {
    let at = 1 + e.tag.len();
    e.start_text.insert_str(at, &format!(" id=\"{id}\""));
    e.attrs.push(("id".into(), Some(id.into())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::htmltree::{parse, serialize};

    fn run(html: &str) -> (String, Vec<TocEntry>) {
        let mut nodes = parse(html);
        let toc = anchor_headings(&mut nodes);
        (serialize(&nodes), toc)
    }

    fn entry(id: &str, text: &str, level: u8) -> TocEntry {
        TocEntry { id: id.into(), text: text.into(), level }
    }

    #[test]
    fn slugs_fold_portuguese_accents_and_punctuation() {
        let (html, toc) = run("<h2>1. Só para um adulto</h2>\n<h2>Cães &amp; gatos: o que muda?</h2>\n<h3>Vacinação, vermífugo e castração!</h3>");
        assert_eq!(
            html,
            "<h2 id=\"1-so-para-um-adulto\">1. Só para um adulto</h2>\n<h2 id=\"caes-gatos-o-que-muda\">Cães &amp; gatos: o que muda?</h2>\n<h3 id=\"vacinacao-vermifugo-e-castracao\">Vacinação, vermífugo e castração!</h3>"
        );
        assert_eq!(toc[1], entry("caes-gatos-o-que-muda", "Cães & gatos: o que muda?", 2));
        assert_eq!(toc[2].level, 3);
    }

    #[test]
    fn repeats_get_a_number_and_author_ids_are_kept() {
        let (html, toc) = run("<p id=\"dicas-2\">x</p><h2>Dicas</h2><h3>Dicas</h3><h2 id=\"Minha Âncora\">Dicas</h2><h2>Dicas</h2><h2>?!</h2>");
        assert_eq!(
            html,
            "<p id=\"dicas-2\">x</p><h2 id=\"dicas\">Dicas</h2><h3 id=\"dicas-3\">Dicas</h3><h2 id=\"Minha Âncora\">Dicas</h2><h2 id=\"dicas-4\">Dicas</h2><h2 id=\"section\">?!</h2>"
        );
        let ids: Vec<&str> = toc.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["dicas", "dicas-3", "Minha Âncora", "dicas-4", "section"]);
    }

    #[test]
    fn text_is_plain_and_order_is_the_document_order() {
        let (_, toc) = run("<h1>Title</h1><h2 class=\"x\">Use <code>magehat\n  build</code> <em>now</em></h2><div><h3>Inner</h3></div><h4>Deep</h4><h2>Last</h2>");
        assert_eq!(toc, [entry("use-magehat-build-now", "Use magehat build now", 2), entry("inner", "Inner", 3), entry("last", "Last", 2)]);
    }

    #[test]
    fn no_headings_no_entries_and_no_change() {
        let src = "<p>Só texto.</p>\n<ul><li>a</li></ul>";
        let (html, toc) = run(src);
        assert_eq!(html, src);
        assert!(toc.is_empty());
    }
}
