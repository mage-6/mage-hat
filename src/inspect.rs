//! `magehat inspect --json`: everything an agent needs to know about a site
//! without reading every file. The shape of this output is part of MageHat's
//! stable surface: keys are only ever added.

use crate::build::{build_site, BuildResult};
use crate::check::flatten_keys;
use crate::components::{visit, Component};
use crate::config::RESERVED_GLOBALS;
use crate::errors::Result;
use crate::expr::path_roots;
use crate::htmltree::Node;
use regex::Regex;
use serde_json::{json, Value as Json};
use std::path::Path;
use std::sync::LazyLock;

static EXPR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\{\{\s*(.+?)\s*\}\}").unwrap());
static EACH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)^\s*([A-Za-z_][A-Za-z0-9_]*)\s+in\s+(.+?)\s*$").unwrap());

pub fn inspect_site(root: &Path) -> Result<Json> {
    let r = build_site(root)?;
    let cfg = &r.cfg;
    let mut globals: Vec<&str> = RESERVED_GLOBALS.to_vec();
    let mut colls: Vec<&str> = r.collections.keys().map(String::as_str).collect();
    colls.sort();
    globals.extend(colls);
    Ok(json!({
        "magehat": env!("CARGO_PKG_VERSION"),
        "site": {
            "name": cfg.name,
            "url": cfg.url,
            "languages": cfg.languages,
            "default_language": cfg.default_language(),
            "config": "site.toml",
        },
        "commands": {
            "magehat check [--json]": "build in memory and report errors and warnings, each with its fix; fix everything it reports",
            "magehat build [--json]": "write the site to dist/",
            "magehat new page <name> [--lang xx]": "create a page file with the right shape",
            "magehat new component <name>": "create a component file, used as <x-name>",
            "magehat new item <collection> <id> [--lang xx]": "create a content item with metadata",
            "magehat add [name]": "list the ready-made components, or copy one into src/components",
            "magehat dev [--port N]": "serve dist/ locally with live reload",
            "magehat inspect --json": "this description of the site",
            "magehat clean": "remove dist/ and the image cache",
            "magehat": "the list of commands",
            "magehat -h": "print the manual, which is the complete reference",
        },
        "syntax": {
            "text": "{{ expr }} inserts a value, HTML-escaped; {{ t.key }} reads src/i18n/<lang>.json",
            "each": "each=\"item in list\" repeats the element per item",
            "if": "if=\"expr\" keeps the element only when expr is true",
            "components": "<x-name attr=\"value\">children</x-name>; attributes are props, children fill <slot>",
            "expressions": "dotted paths, 'strings', numbers, true, false, null, not, and, or, ==, !=",
            "globals": globals,
        },
        "pages": pages(&r),
        "components": r.components.values().map(|c| component(c, &r)).collect::<Vec<_>>(),
        "collections": collections(&r),
        "i18n": cfg.languages.iter().map(|lang| {
            let mut keys = Vec::new();
            if let Some(v) = r.i18n.get(lang) { flatten_keys(v, "", &mut keys); }
            keys.sort();
            (lang.clone(), json!({"file": format!("src/i18n/{lang}.json"), "keys": keys}))
        }).collect::<serde_json::Map<String, Json>>(),
        "data": match &r.data { crate::values::Value::Map(m) => m.keys().cloned().collect::<Vec<_>>(), _ => Vec::new() },
        "errors": r.errors.iter().map(|e| json!({"file": e.file, "line": e.line, "message": e.message, "fix": e.fix})).collect::<Vec<_>>(),
        "warnings": r.warnings.iter().map(|w| json!({"file": w.file, "message": w.message})).collect::<Vec<_>>(),
    }))
}

fn pages(r: &BuildResult) -> Vec<Json> {
    let mut out: indexmap::IndexMap<String, Json> = indexmap::IndexMap::new();
    for s in &r.sources {
        let entry = out.entry(s.identity.clone()).or_insert_with(|| {
            let mut e = json!({
                "id": s.identity,
                "kind": if s.item_var.is_some() { "items" } else { "page" },
                "files": [],
                "urls": {},
            });
            if let Some(v) = &s.item_var {
                e["collection"] = json!(s.collection);
                e["item_var"] = json!(v);
            }
            e
        });
        entry["files"].as_array_mut().unwrap().push(json!(s.file));
    }
    for p in &r.pages {
        let Some(entry) = out.get_mut(&p.identity) else { continue };
        let urls = entry["urls"].as_object_mut().unwrap();
        if p.item_id.is_none() {
            urls.insert(p.lang.clone(), json!(p.url));
        } else {
            urls.entry(p.lang.clone()).or_insert_with(|| json!([])).as_array_mut().unwrap().push(json!(p.url));
        }
    }
    out.into_values().collect()
}

fn component(c: &Component, r: &BuildResult) -> Json {
    let mut roots: Vec<String> = Vec::new();
    let mut loop_vars: Vec<String> = Vec::new();
    let add = |src: &str, roots: &mut Vec<String>| {
        for root in path_roots(src) {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    };
    visit(&c.template, &mut |n: &Node| match n {
        Node::Text { text, .. } => {
            for m in EXPR.captures_iter(text) {
                add(&m[1], &mut roots);
            }
        }
        Node::Element(e) => {
            for (name, value) in &e.attrs {
                let Some(value) = value else { continue };
                match name.as_str() {
                    "each" => {
                        if let Some(m) = EACH.captures(value) {
                            loop_vars.push(m[1].to_string());
                            add(&m[2], &mut roots);
                        }
                    }
                    "if" => add(value, &mut roots),
                    _ => {
                        for m in EXPR.captures_iter(value) {
                            add(&m[1], &mut roots);
                        }
                    }
                }
            }
        }
        Node::Raw { .. } => {}
    });
    let mut props: Vec<&String> = roots
        .iter()
        .filter(|n| !RESERVED_GLOBALS.contains(&n.as_str()) && !r.collections.contains_key(*n) && !loop_vars.contains(n))
        .collect();
    props.sort();
    let attrs: String = props.iter().map(|p| format!(" {p}=\"...\"")).collect();
    let usage = if c.is_document {
        format!("<{}{attrs}>page content</{}>", c.tag, c.tag)
    } else if c.slots.is_empty() {
        format!("<{}{attrs}></{}>", c.tag, c.tag)
    } else {
        let inner: String = c
            .slots
            .iter()
            .map(|s| if s.is_empty() { "children".to_string() } else { format!("<div slot=\"{s}\">...</div>") })
            .collect::<Vec<_>>()
            .join("");
        format!("<{}{attrs}>{inner}</{}>", c.tag, c.tag)
    };
    json!({
        "tag": c.tag,
        "file": c.file,
        "usage": usage,
        "props": props,
        "slots": c.slots,
        "layout": c.is_document,
        "style": !c.style.is_empty(),
        "script": !c.script.is_empty(),
    })
}

fn collections(r: &BuildResult) -> Json {
    let mut out = serde_json::Map::new();
    for (coll, by_lang) in &r.collections {
        let page = r.sources.iter().find(|s| s.item_var.is_some() && s.collection.as_deref() == Some(coll));
        let mut ids: indexmap::IndexMap<String, (Vec<String>, Vec<String>)> = indexmap::IndexMap::new();
        for lang in &r.cfg.languages {
            for item in &by_lang[lang] {
                let e = ids.entry(item.id.clone()).or_default();
                e.0.push(lang.clone());
                for k in item.meta.keys() {
                    if !e.1.contains(k) {
                        e.1.push(k.clone());
                    }
                }
            }
        }
        let items: Vec<Json> = ids
            .into_iter()
            .map(|(id, (languages, mut fields))| {
                fields.sort();
                json!({"id": id, "languages": languages, "fields": fields})
            })
            .collect();
        out.insert(
            coll.clone(),
            json!({
                "folder": format!("src/content/{coll}"),
                "item_page": page.map(|p| p.file.clone()),
                "item_var": page.and_then(|p| p.item_var.clone()),
                "feed": r.cfg.collection_option(coll, "feed").and_then(|v| v.as_bool()).unwrap_or(false),
                "items": items,
            }),
        );
    }
    Json::Object(out)
}
