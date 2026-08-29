//! Pages: one HTML file per page under src/pages, and the URL each one gets.
//!
//!     src/pages/index.html           /                  default language
//!     src/pages/about.html           /about/
//!     src/pages/about.pt-BR.html     /pt-br/about/      the same page in Portuguese
//!     src/pages/blog/index.html      /blog/
//!     src/pages/blog/[post].html     /blog/<slug>/      one page per item of the
//!                                                       "blog" collection, as `post`
//!     src/pages/404.html             /404.html
//!
//! An unsuffixed file is the default language only, so a page exists in a
//! language exactly when a file for it exists. Item page templates ([post])
//! are the exception: they are shared by every language unless a suffixed
//! variant exists, because their text comes from the item.

use crate::components::walk_files;
use crate::config::Config;
use crate::errors::{MageError, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PageSource {
    /// 'about', 'blog/index', 'blog/[post]'
    pub identity: String,
    /// None for an unsuffixed file
    pub lang: Option<String>,
    pub path: PathBuf,
    /// Site-relative path with forward slashes
    pub file: String,
    /// 'post' for blog/[post].html
    pub item_var: Option<String>,
    /// 'blog' for blog/[post].html (folder name)
    pub collection: Option<String>,
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '[' || c == ']')
}

pub fn discover_pages(cfg: &Config) -> Result<Vec<PageSource>> {
    let base = cfg.src().join("pages");
    let mut pages = Vec::new();
    if !base.is_dir() {
        return Ok(pages);
    }
    for (path, rel) in walk_files(&cfg.root, &base) {
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let inner = path.strip_prefix(&base).unwrap();
        let mut stem = inner.file_stem().unwrap().to_string_lossy().to_string();
        let mut lang = None;
        if let Some((head, tail)) = stem.rsplit_once('.') {
            if cfg.languages.contains(&tail.to_string()) {
                lang = Some(tail.to_string());
                stem = head.to_string();
            } else {
                return Err(MageError::in_file(format!("{tail:?} is not one of the site languages"), &rel)
                    .fix(format!("languages in site.toml are {:?}; add {tail:?} there or fix the file name", cfg.languages)));
            }
        }
        if !valid_name(&stem) {
            return Err(MageError::in_file("page file names use letters, digits, - and _", &rel)
                .fix("rename the file; the name becomes the URL"));
        }
        let parts: Vec<String> = inner.parent().map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect()).unwrap_or_default();
        for part in &parts {
            if !valid_name(part) || part.contains('[') {
                return Err(MageError::in_file("page folder names use letters, digits, - and _", &rel)
                    .fix("rename the folder; only the file name may be [item]"));
            }
        }
        let item_var = stem.strip_prefix('[').and_then(|s| s.strip_suffix(']')).map(String::from);
        if let Some(v) = &item_var {
            if v.is_empty() || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(MageError::in_file("the name in brackets must be a variable name", &rel).fix("for example blog/[post].html"));
            }
        }
        let collection = if item_var.is_some() { parts.last().cloned() } else { None };
        if item_var.is_some() && collection.is_none() {
            return Err(MageError::in_file("an item page must live in a folder named after its collection", &rel)
                .fix("move it to src/pages/<collection>/[item].html, for example src/pages/blog/[post].html"));
        }
        let mut id_parts = parts.clone();
        id_parts.push(stem);
        pages.push(PageSource { identity: id_parts.join("/"), lang, path, file: rel, item_var, collection });
    }
    Ok(pages)
}

/// URL for a page identity in a language. `slug` fills in an item page's [var].
pub fn page_url(cfg: &Config, identity: &str, lang: &str, slug: Option<&str>) -> String {
    let mut parts: Vec<String> = identity.split('/').map(String::from).collect();
    if let Some(s) = slug {
        *parts.last_mut().unwrap() = s.to_string();
    }
    if parts.last().map(String::as_str) == Some("index") {
        parts.pop();
    } else if parts.len() == 1 && parts[0] == "404" {
        return format!("{}/404.html", cfg.lang_prefix(lang));
    }
    let path = parts.join("/");
    if path.is_empty() {
        format!("{}/", cfg.lang_prefix(lang))
    } else {
        format!("{}/{path}/", cfg.lang_prefix(lang))
    }
}

/// dist-relative file for a URL: /about/ -> about/index.html, /404.html -> 404.html.
pub fn output_path(url: &str) -> String {
    let rel = url.trim_start_matches('/');
    if url.ends_with('/') {
        format!("{rel}index.html")
    } else {
        rel.to_string()
    }
}

/// Pick the source file for a page identity in a language, if any.
pub fn resolve<'a>(pages: &'a [PageSource], identity: &str, lang: &str, cfg: &Config) -> Option<&'a PageSource> {
    let candidates: Vec<&PageSource> = pages.iter().filter(|p| p.identity == identity).collect();
    if let Some(p) = candidates.iter().find(|p| p.lang.as_deref() == Some(lang)) {
        return Some(p);
    }
    candidates
        .into_iter()
        .find(|p| p.lang.is_none() && (lang == cfg.default_language() || p.item_var.is_some() || cfg.languages.len() == 1))
}
