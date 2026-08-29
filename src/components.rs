//! Components: one HTML file each in src/components.
//!
//!     src/components/card.html       ->  <x-card>
//!     src/components/blog/hero.html  ->  <x-blog-hero>
//!
//! A file holds a <template>, an optional <style> and an optional <script>.
//! A component whose template contains <html> is a document layout: it is
//! rendered without a wrapper element.

use crate::errors::{MageError, Result};
use crate::htmltree::{parse, Node};
use indexmap::IndexMap;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Component {
    pub tag: String,
    /// Site-relative path with forward slashes, e.g. src/components/card.html
    pub file: String,
    pub template: Vec<Node>,
    pub style: String,
    pub script: String,
    pub is_document: bool,
    pub slots: Vec<String>,
}

/// Short content hash used in generated asset file names for cache busting.
pub fn digest(text: &str) -> String {
    digest_bytes(text.as_bytes())
}

pub fn digest_bytes(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    hash.iter().map(|b| format!("{b:02x}")).collect::<String>()[..10].to_string()
}

/// Every file under `dir`, recursively, sorted, as (absolute path, site-relative string).
pub fn walk_files(root: &Path, dir: &Path) -> Vec<(std::path::PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                out.push((p, rel));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

pub fn load_components(root: &Path) -> Result<IndexMap<String, Component>> {
    let base = root.join("src").join("components");
    let mut comps = IndexMap::new();
    if !base.is_dir() {
        return Ok(comps);
    }
    for (path, rel) in walk_files(root, &base) {
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let inner = path.strip_prefix(&base).unwrap().with_extension("");
        let parts: Vec<String> = inner.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
        let tag = format!("x-{}", parts.join("-"));
        let valid = tag.split('-').skip(1).all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
        if !valid {
            return Err(MageError::in_file("component file names use lowercase letters, digits and dashes", &rel)
                .fix(format!("rename it, for example src/components/{}.html", parts.join("-").replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "-"))));
        }
        let text = std::fs::read_to_string(&path)?;
        comps.insert(tag.clone(), load(&text, tag, rel)?);
    }
    Ok(comps)
}

fn load(text: &str, tag: String, file: String) -> Result<Component> {
    let nodes = parse(text);
    let mut styles = Vec::new();
    let mut scripts = Vec::new();
    let mut template: Option<Vec<Node>> = None;
    let mut stray = None;
    for n in nodes {
        match n {
            Node::Element(e) if e.tag == "style" => styles.push(e.text_content()),
            Node::Element(e) if e.tag == "script" => scripts.push(e.text_content()),
            Node::Element(e) if e.tag == "template" && template.is_none() => template = Some(e.children),
            Node::Element(e) => { stray.get_or_insert((e.tag.clone(), e.line)); }
            Node::Raw { .. } | Node::Text { .. } => {}
        };
    }
    let Some(template) = template else {
        let line = stray.as_ref().map(|s| s.1).unwrap_or(1);
        return Err(MageError::at(format!("component <{tag}> has no <template>"), &file, line)
            .fix("a component file is <template>markup</template>, then optional <style> and <script> outside it"));
    };
    if let Some((stray_tag, line)) = stray {
        return Err(MageError::at(format!("<{stray_tag}> outside the <template> of component <{tag}>"), &file, line)
            .fix("only <template>, <style> and <script> may appear at the top level of a component file; move the markup inside <template>"));
    }
    let is_document = template.iter().any(|n| matches!(n, Node::Element(e) if e.tag == "html"));
    let mut slots = Vec::new();
    slot_names(&template, &mut slots);
    Ok(Component {
        tag,
        file,
        template,
        style: styles.join("\n").trim().to_string(),
        script: scripts.join("\n").trim().to_string(),
        is_document,
        slots,
    })
}

fn slot_names(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        if let Node::Element(e) = n {
            if e.tag == "slot" {
                let name = e.attr("name").unwrap_or("").to_string();
                if !out.contains(&name) {
                    out.push(name);
                }
            }
            slot_names(&e.children, out);
        }
    }
}

/// Wrap a component's CSS in native @scope so it applies to the component's
/// own markup and stops at the boundary of any nested component. The
/// component element itself defaults to display: contents so it does not
/// disturb layout; a component can override that with `:scope { ... }`.
pub fn scoped_css(comp: &Component, all_tags: &[String]) -> String {
    let mut tags: Vec<&str> = all_tags.iter().map(String::as_str).collect();
    tags.sort_unstable();
    let limit = format!(":scope :is({}) > *", tags.join(", "));
    if comp.is_document {
        format!("@scope (html) to ({limit}) {{\n{}\n}}\n", comp.style)
    } else {
        format!("{} {{ display: contents; }}\n@scope ({}) to ({limit}) {{\n{}\n}}\n", comp.tag, comp.tag, comp.style)
    }
}

/// Visit every text node and every element (for expression discovery).
pub fn visit(nodes: &[Node], f: &mut dyn FnMut(&Node)) {
    for n in nodes {
        f(n);
        if let Node::Element(e) = n {
            if !crate::htmltree::RAW.contains(&e.tag.as_str()) {
                visit(&e.children, f);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_a_template() {
        let e = load("<div>{{ x }}</div>", "x-a".into(), "src/components/a.html".into()).unwrap_err();
        assert!(e.message.contains("no <template>"));
        assert!(e.fix.is_some());
    }

    #[test]
    fn finds_slots_and_document_layouts() {
        let c = load("<template><html><body><slot></slot><slot name=\"side\"></slot></body></html></template><style>a{}</style>", "x-b".into(), "f".into()).unwrap();
        assert!(c.is_document);
        assert_eq!(c.slots, vec!["", "side"]);
        assert_eq!(c.style, "a{}");
        assert_eq!(digest("a{}").len(), 10);
    }
}
