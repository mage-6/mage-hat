//! Content-hashed asset names, with references rewritten.
//!
//! `src/assets/site.css` is still served at `/site.css`, and a copy named
//! `/_mh/a/site.<hash>.css` is added. Every reference MageHat can see (href,
//! src, srcset, poster, content, and url() inside CSS) is rewritten to the
//! hashed copy, always as a root-absolute URL. Originals stay, so a
//! reference MageHat cannot see (inside a script, in an RSS reader) still
//! works.
//!
//! Every hashed file MageHat writes lives under `/_mh/`, so one `_headers`
//! rule (`headers`) tells the host to keep them all for a year: a file under
//! a hashed name never changes, and without the rule Cloudflare makes the
//! browser revalidate each one on every page view.

use crate::build::{BuildResult, ASSET_DIR};
use crate::components::digest_bytes;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

const HASHED: &[&str] = &[
    "css", "js", "mjs", "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "ico", "woff", "woff2", "ttf", "otf",
    "mp4", "webm", "mp3", "pdf", "json", "wasm",
];

static ATTR_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(\s(?:href|src|poster|content|data-src)\s*=\s*)(["'])([^"']*)(["'])"#).unwrap());
static SRCSET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(\ssrcset\s*=\s*)(["'])([^"']*)(["'])"#).unwrap());
static CSS_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"url\(\s*(["']?)([^"')]+)(["']?)\s*\)"#).unwrap());

/// Map from an output path to its hashed twin, e.g. "site.css" -> "site.1a2b3c4d5e.css".
pub type AssetMap = HashMap<String, String>;

fn hashed_name(key: &str, bytes: &[u8]) -> String {
    match key.rsplit_once('.') {
        Some((stem, ext)) => format!("{ASSET_DIR}/a/{stem}.{}.{ext}", digest_bytes(bytes)),
        None => format!("{ASSET_DIR}/a/{key}.{}", digest_bytes(bytes)),
    }
}

/// The rule that lets browsers and Cloudflare keep everything under `/_mh/`
/// for a year, appended to the site's own `_headers` when it ships one.
const CACHE_RULE: &str = "/_mh/*\n  Cache-Control: public, max-age=31536000, immutable\n";

/// The `_headers` file to publish: the site's own (if any) plus the cache
/// rule for hashed files, unless the site already has a rule for `/_mh/*`.
pub fn headers(own: Option<&[u8]>) -> Vec<u8> {
    let own = own.map(|b| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    if own.lines().any(|l| l.trim() == "/_mh/*") {
        return own.into_bytes();
    }
    let mut out = own.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("# Added by MageHat: files under /_mh/ have content-hashed names and never change.\n");
    out.push_str(CACHE_RULE);
    out.into_bytes()
}

fn hashable(key: &str) -> bool {
    if key.starts_with('_') || key.starts_with(".well-known/") || key.contains("/_") {
        return false;
    }
    key.rsplit_once('.').map_or(false, |(_, ext)| HASHED.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Add hashed copies of the given asset outputs. CSS is processed after
/// everything else so the url() references inside it are already hashed.
pub fn plan(r: &mut BuildResult, asset_keys: &[String]) -> AssetMap {
    let mut map = AssetMap::new();
    let (css, others): (Vec<&String>, Vec<&String>) = asset_keys.iter().filter(|k| hashable(k)).partition(|k| k.ends_with(".css"));
    for key in others {
        let bytes = r.outputs[key].clone();
        let name = hashed_name(key, &bytes);
        r.outputs.insert(name.clone(), bytes);
        map.insert(key.clone(), name);
    }
    for key in css {
        let text = String::from_utf8_lossy(&r.outputs[key]).to_string();
        let rewritten = crate::minify::css(&rewrite_css(&text, key, &map));
        let name = hashed_name(key, rewritten.as_bytes());
        r.outputs.insert(key.clone(), rewritten.clone().into_bytes());
        r.outputs.insert(name.clone(), rewritten.into_bytes());
        map.insert(key.clone(), name);
    }
    map
}

/// Resolve a reference found in `from` (an output path) to an output key.
/// Returns (key, suffix) where suffix is the ?query or #fragment to keep.
fn resolve(from: &str, value: &str) -> Option<(String, String)> {
    let v = value.trim();
    if v.is_empty() || v.starts_with('#') || v.starts_with("//") || v.starts_with("data:") || v.contains(':') {
        return None;
    }
    let cut = v.find(['?', '#']).unwrap_or(v.len());
    let (path, suffix) = v.split_at(cut);
    let key = if let Some(abs) = path.strip_prefix('/') {
        abs.to_string()
    } else {
        let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        for seg in path.split('/') {
            match seg {
                "" | "." => {}
                ".." => { parts.pop(); }
                s => parts.push(s),
            }
        }
        parts.join("/")
    };
    Some((key, suffix.to_string()))
}

/// The URL to write for a reference: root-absolute, since the hashed copy
/// lives under /_mh/a/, not beside the original.
fn replacement(from: &str, value: &str, map: &AssetMap) -> Option<String> {
    let (key, suffix) = resolve(from, value)?;
    let hashed = map.get(&key)?;
    Some(format!("/{hashed}{suffix}"))
}

pub fn rewrite_css(text: &str, from: &str, map: &AssetMap) -> String {
    CSS_URL
        .replace_all(text, |c: &regex::Captures| match replacement(from, &c[2], map) {
            Some(url) => format!("url({}{url}{})", &c[1], &c[3]),
            None => c[0].to_string(),
        })
        .into_owned()
}

pub fn rewrite_html(html: &str, page_out: &str, map: &AssetMap) -> String {
    let step = ATTR_REF.replace_all(html, |c: &regex::Captures| match replacement(page_out, &c[3], map) {
        Some(url) => format!("{}{}{url}{}", &c[1], &c[2], &c[4]),
        None => c[0].to_string(),
    });
    SRCSET
        .replace_all(&step, |c: &regex::Captures| {
            let rewritten: Vec<String> = c[3]
                .split(',')
                .map(|cand| {
                    let cand = cand.trim();
                    let (url, desc) = cand.split_once(char::is_whitespace).unwrap_or((cand, ""));
                    let url = replacement(page_out, url, map).unwrap_or_else(|| url.to_string());
                    if desc.is_empty() { url } else { format!("{url} {}", desc.trim()) }
                })
                .collect();
            format!("{}{}{}{}", &c[1], &c[2], rewritten.join(", "), &c[4])
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_absolute_and_relative_references() {
        let mut map = AssetMap::new();
        map.insert("site.css".into(), "_mh/a/site.abc.css".into());
        map.insert("img/a.png".into(), "_mh/a/img/a.def.png".into());
        let html = "<link href=\"/site.css?v=1\"><img src=\"../img/a.png\" srcset=\"/img/a.png 1x, /img/b.png 2x\"><a href=\"/about/\">x</a>";
        assert_eq!(
            rewrite_html(html, "blog/index.html", &map),
            "<link href=\"/_mh/a/site.abc.css?v=1\"><img src=\"/_mh/a/img/a.def.png\" srcset=\"/_mh/a/img/a.def.png 1x, /img/b.png 2x\"><a href=\"/about/\">x</a>"
        );
        assert_eq!(rewrite_css("a{background:url('/img/a.png')} b{background:url(http://x/y.png)}", "site.css", &map), "a{background:url('/_mh/a/img/a.def.png')} b{background:url(http://x/y.png)}");
        // A relative url() in a stylesheet resolves against the stylesheet's own folder.
        assert_eq!(rewrite_css("a{background:url(a.png)}", "img/x.css", &map), "a{background:url(/_mh/a/img/a.def.png)}");
    }

    #[test]
    fn headers_add_the_cache_rule_once() {
        let fresh = String::from_utf8(headers(None)).unwrap();
        assert!(fresh.contains("/_mh/*\n  Cache-Control: public, max-age=31536000, immutable"), "{fresh}");
        let own = "/*\n  X-Frame-Options: DENY\n";
        let merged = String::from_utf8(headers(Some(own.as_bytes()))).unwrap();
        assert!(merged.starts_with(own) && merged.contains("/_mh/*\n  Cache-Control"), "{merged}");
        let custom = "/_mh/*\n  Cache-Control: no-store\n";
        assert_eq!(headers(Some(custom.as_bytes())), custom.as_bytes(), "a site's own rule for /_mh/* stands");
    }
}
