//! Collections: folders of items with the same shape, under src/content.
//!
//!     src/content/blog/hello.md           item "hello", default language
//!     src/content/blog/hello.pt-BR.md     the same item in Portuguese
//!     src/content/blog/hello.html         an HTML item
//!
//! Markdown items carry metadata in a leading --- block; HTML items start with
//! <title> and <meta name=... content=...> elements. `draft: true` hides an item.
//! Items are ordered by `date` descending, then by id.

use crate::components::walk_files;
use crate::config::Config;
use crate::errors::{MageError, Result};
use crate::frontmatter::{parse_scalar, split_frontmatter};
use crate::htmltree::{parse, serialize, Node};
use crate::markdown::render_markdown;
use crate::values::{to_text, Map, Value};
use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub struct Item {
    pub collection: String,
    pub id: String,
    pub lang: String,
    pub file: String,
    pub meta: Map,
    /// HTML text, still containing component tags to expand.
    pub body_html: String,
}

impl Item {
    pub fn slug(&self) -> String {
        match self.meta.get("slug") {
            Some(v) if !to_text(v).is_empty() => to_text(v),
            _ => self.id.clone(),
        }
    }
}

/// collection -> language -> ordered items
pub type Collections = IndexMap<String, IndexMap<String, Vec<Item>>>;

pub fn load_collections(cfg: &Config) -> Result<Collections> {
    let base = cfg.src().join("content");
    let mut out = Collections::new();
    if !base.is_dir() {
        return Ok(out);
    }
    let mut dirs: Vec<_> = std::fs::read_dir(&base)?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for coll_dir in dirs {
        let name = coll_dir.file_name().unwrap().to_string_lossy().to_string();
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(MageError::in_file(format!("collection folder name must be letters, digits, - or _: {name}"), &format!("src/content/{name}")));
        }
        let mut by_lang: IndexMap<String, Vec<Item>> = cfg.languages.iter().map(|l| (l.clone(), Vec::new())).collect();
        for (path, rel) in walk_files(&cfg.root, &coll_dir) {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "md" && ext != "html" {
                continue;
            }
            let inner = path.strip_prefix(&coll_dir).unwrap();
            if inner.components().count() != 1 {
                let stem = inner.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
                let folder = inner.components().next().unwrap().as_os_str().to_string_lossy();
                return Err(MageError::in_file("content files do not go in sub-folders", &rel).fix(format!(
                    "rename src/content/{name}/{folder}/{stem}.{ext} to src/content/{name}/{folder}.{ext} for the default language, or {folder}.{stem}.{ext} if {stem} is a language code"
                )));
            }
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            let (id, lang) = match stem.rsplit_once('.') {
                Some((id, lang)) => {
                    if !cfg.languages.contains(&lang.to_string()) {
                        return Err(MageError::in_file(format!("{lang:?} is not one of the site languages"), &rel)
                            .fix(format!("languages in site.toml are {:?}; add {lang:?} there or fix the file name", cfg.languages)));
                    }
                    (id.to_string(), lang.to_string())
                }
                None => (stem.clone(), cfg.default_language().to_string()),
            };
            let item = load_item(&path, &rel, &name, id, lang)?;
            if matches!(item.meta.get("draft"), Some(Value::Bool(true))) {
                continue;
            }
            by_lang.get_mut(&item.lang).unwrap().push(item);
        }
        for items in by_lang.values_mut() {
            check_duplicates(items)?;
            items.sort_by(|a, b| a.id.cmp(&b.id));
            items.sort_by(|a, b| date_key(b).cmp(&date_key(a)));
            items.sort_by(|a, b| order_key(a).partial_cmp(&order_key(b)).unwrap_or(std::cmp::Ordering::Equal));
        }
        out.insert(name, by_lang);
    }
    Ok(out)
}

fn date_key(item: &Item) -> String {
    item.meta.get("date").map(to_text).unwrap_or_default()
}

/// An explicit `order` puts an item ahead of the dated ones, lowest first;
/// it is for collections with no dates (a FAQ, a team, a feature list).
fn order_key(item: &Item) -> (bool, f64) {
    match item.meta.get("order") {
        Some(Value::Int(n)) => (false, *n as f64),
        Some(Value::Float(n)) => (false, *n),
        Some(v) => match to_text(v).trim().parse::<f64>() {
            Ok(n) => (false, n),
            Err(_) => (true, 0.0),
        },
        None => (true, 0.0),
    }
}

fn load_item(path: &std::path::Path, rel: &str, collection: &str, id: String, lang: String) -> Result<Item> {
    let text = std::fs::read_to_string(path)?;
    if rel.ends_with(".md") {
        let (meta, body, _) = split_frontmatter(&text, rel)?;
        return Ok(Item { collection: collection.into(), id, lang, file: rel.into(), meta, body_html: render_markdown(&body) });
    }
    let nodes = parse(&text);
    let (meta, start, _) = split_html_meta(&nodes);
    Ok(Item { collection: collection.into(), id, lang, file: rel.into(), meta, body_html: serialize(&nodes[start..]) })
}

/// Take leading <title> and <meta name= content=> elements as metadata.
/// Returns (meta, index of the first body node, line of the first body node).
pub fn split_html_meta(nodes: &[Node]) -> (Map, usize, usize) {
    let mut meta = Map::new();
    let mut i = 0;
    while i < nodes.len() {
        match &nodes[i] {
            n if n.is_blank() => {}
            Node::Raw { .. } => {}
            Node::Element(e) if e.tag == "title" => {
                meta.insert("title".into(), Value::str(e.text_content().trim()));
            }
            Node::Element(e) if e.tag == "meta" && e.attr("name").is_some() && e.attr("content").is_some() => {
                meta.insert(e.attr("name").unwrap().to_string(), parse_scalar(e.attr("content").unwrap()));
            }
            _ => break,
        }
        i += 1;
    }
    // Drop the blank line left behind by the metadata block.
    while i < nodes.len() && nodes[i].is_blank() {
        i += 1;
    }
    let line = nodes.get(i).map(|n| n.line()).unwrap_or(1);
    (meta, i, line)
}

fn check_duplicates(items: &[Item]) -> Result<()> {
    let mut slugs: IndexMap<String, &Item> = IndexMap::new();
    for item in items {
        let slug = item.slug();
        if let Some(other) = slugs.get(&slug) {
            return Err(MageError::in_file(format!("slug {slug:?} in {} is also used by {}", item.lang, other.file), &item.file)
                .fix("give one of them a different `slug` in its metadata"));
        }
        slugs.insert(slug, item);
    }
    Ok(())
}
