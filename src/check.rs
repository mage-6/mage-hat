//! `magehat check`: build in memory, then look for what a build alone would
//! let through. Errors fail the check; warnings are printed but do not.
//! Every finding says how to fix it.

use crate::build::{build_site, BuildResult};
use crate::errors::Result;
use crate::htmltree::parse;
use crate::lint::{lint_output, lint_source};
use crate::values::Value;
use regex::Regex;
use serde_json::json;
use std::path::Path;
use std::sync::LazyLock;

static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\s(?:href|src)\s*=\s*["']([^"']*)["']"#).unwrap());
static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*:").unwrap());
static TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>\s*([^<]*?)\s*</title>").unwrap());
static DESCRIPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<meta\s[^>]*name\s*=\s*["']description["'][^>]*content\s*=\s*["']([^"']*)["']"#).unwrap());
static DESCRIPTION2: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<meta\s[^>]*content\s*=\s*["']([^"']*)["'][^>]*name\s*=\s*["']description["']"#).unwrap());
static HTML_LANG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<html\s[^>]*\blang\s*=").unwrap());

pub fn run_check(root: &Path) -> Result<BuildResult> {
    let mut r = build_site(root)?;
    check_markup(&mut r);
    check_translations(&mut r);
    check_links(&mut r);
    check_seo(&mut r);
    check_i18n_parity(&mut r);
    check_external_fonts(&mut r);
    Ok(r)
}

/// A Google Fonts <link> is served locally by the build; anything else that
/// still reaches Google (an @import, an inline style) is reported.
fn check_external_fonts(r: &mut BuildResult) {
    let fix = "load the font with <link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family=...\"> in the layout; MageHat then saves the files under src/assets/fonts and serves them from the site. An @import or inline style is not converted";
    let mut warnings = Vec::new();
    for p in &r.pages {
        let html = String::from_utf8_lossy(&r.outputs[&p.out]);
        if html.contains("fonts.googleapis.com") || html.contains("fonts.gstatic.com") {
            warnings.push((format!("page still loads fonts from Google on {}", p.url), p.file.clone()));
        }
    }
    for (key, bytes) in &r.outputs {
        // Original stylesheets only (site.css, not site.<hash>.css).
        if key.ends_with(".css") && !key.starts_with("_mh/") && key.matches('.').count() == 1 {
            if String::from_utf8_lossy(bytes).contains("fonts.googleapis.com") {
                warnings.push(("stylesheet imports fonts from Google".to_string(), format!("src/assets/{key}")));
            }
        }
    }
    for (m, f) in warnings {
        r.warn_at(m, &f, None, fix);
    }
}

fn check_markup(r: &mut BuildResult) {
    let mut findings = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for s in &r.sources {
        if !seen.insert(s.file.clone()) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&s.path) {
            for f in lint_source(&parse(&text)) {
                findings.push((f.message, s.file.clone(), f.line, f.fix));
            }
        }
    }
    for c in r.components.values() {
        for f in lint_source(&c.template) {
            findings.push((f.message, c.file.clone(), f.line, f.fix));
        }
    }
    for p in &r.pages {
        let html = String::from_utf8_lossy(&r.outputs[&p.out]).to_string();
        for f in lint_output(&html) {
            findings.push((format!("{} on {}", f.message, p.url), p.file.clone(), None, f.fix));
        }
    }
    for (message, file, line, fix) in findings {
        r.warn_at(message, &file, line, &fix);
    }
}

fn check_translations(r: &mut BuildResult) {
    if r.cfg.languages.len() < 2 {
        return;
    }
    let mut warnings = Vec::new();
    let mut by_identity: indexmap::IndexMap<String, Vec<String>> = indexmap::IndexMap::new();
    for p in &r.pages {
        if p.item_id.is_none() {
            by_identity.entry(p.identity.clone()).or_default().push(p.lang.clone());
        }
    }
    by_identity.sort_keys();
    for (identity, langs) in &by_identity {
        let missing: Vec<&str> = r.cfg.languages.iter().filter(|l| !langs.contains(l)).map(String::as_str).collect();
        if !missing.is_empty() {
            let file = r.pages.iter().find(|p| &p.identity == identity).map(|p| p.file.clone()).unwrap_or_default();
            let fix = format!("add src/pages/{identity}.{}.html, or accept that this page exists only in {}", missing[0], langs.join(", "));
            warnings.push((format!("page {identity:?} has no translation for: {}", missing.join(", ")), file, fix));
        }
    }
    for (coll, by_lang) in &r.collections {
        let mut ids: indexmap::IndexMap<String, (Vec<String>, String)> = indexmap::IndexMap::new();
        for (lang, items) in by_lang {
            for item in items {
                let e = ids.entry(item.id.clone()).or_insert_with(|| (Vec::new(), item.file.clone()));
                e.0.push(lang.clone());
            }
        }
        ids.sort_keys();
        for (id, (langs, file)) in &ids {
            let missing: Vec<&str> = r.cfg.languages.iter().filter(|l| !langs.contains(l)).map(String::as_str).collect();
            if !missing.is_empty() {
                let ext = file.rsplit_once('.').map(|(_, e)| e).unwrap_or("md");
                let fix = format!("add src/content/{coll}/{id}.{}.{ext}, or accept that this item exists only in {}", missing[0], langs.join(", "));
                warnings.push((format!("{coll}/{id} has no translation for: {}", missing.join(", ")), file.clone(), fix));
            }
        }
    }
    for (m, f, fix) in warnings {
        r.warn_at(m, &f, None, &fix);
    }
}

fn check_links(r: &mut BuildResult) {
    let mut warnings = Vec::new();
    for p in &r.pages {
        let html = String::from_utf8_lossy(&r.outputs[&p.out]);
        let mut seen = std::collections::HashSet::new();
        for m in LINK.captures_iter(&html) {
            let link = m[1].trim().to_string();
            if link.is_empty() || !seen.insert(link.clone()) || link.starts_with('#') || link.starts_with("//") || SCHEME.is_match(&link) {
                continue;
            }
            let cut = link.find(['?', '#']).unwrap_or(link.len());
            let mut target = link[..cut].to_string();
            if target.is_empty() {
                continue;
            }
            if !target.starts_with('/') {
                let dir = p.url.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                target = normalize(&format!("{dir}/{target}"));
                if link.ends_with('/') && !target.ends_with('/') {
                    target.push('/');
                }
            }
            if !exists(&r.outputs, &target) {
                warnings.push((format!("broken link {link:?} on {}", p.url), p.file.clone()));
            }
        }
    }
    for (m, f) in warnings {
        r.warn_at(m, &f, None, "fix the path, or create the page or asset it points to; write links as they are in the default language");
    }
}

/// Resolve `.` and `..` in an absolute URL path.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => { parts.pop(); }
            s => parts.push(s),
        }
    }
    let joined = format!("/{}", parts.join("/"));
    if path.ends_with('/') && joined != "/" { joined + "/" } else { joined }
}

fn exists(outputs: &std::collections::BTreeMap<String, Vec<u8>>, target: &str) -> bool {
    let rel = target.trim_start_matches('/');
    if rel.is_empty() {
        return outputs.contains_key("index.html");
    }
    outputs.contains_key(rel) || outputs.contains_key(&format!("{}/index.html", rel.trim_end_matches('/')))
}

fn check_seo(r: &mut BuildResult) {
    let mut warnings = Vec::new();
    for p in &r.pages {
        let html = String::from_utf8_lossy(&r.outputs[&p.out]).to_string();
        if !html.to_lowercase().contains("<head") {
            warnings.push(("page has no <head>".to_string(), p.file.clone(), "wrap the page content in a layout component, for example <x-base>...</x-base>".to_string()));
            continue;
        }
        if TITLE.captures(&html).map_or(true, |c| c[1].trim().is_empty()) {
            warnings.push((format!("missing <title> on {}", p.url), p.file.clone(), "start the page file with <title>...</title>".to_string()));
        }
        let d = DESCRIPTION.captures(&html).or_else(|| DESCRIPTION2.captures(&html));
        if d.map_or(true, |c| c[1].trim().is_empty()) {
            warnings.push((format!("missing meta description on {}", p.url), p.file.clone(),
                "add <meta name=\"description\" content=\"...\"> after the <title> at the top of the page file (or a description in the item metadata)".to_string()));
        }
        if !HTML_LANG.is_match(&html) {
            warnings.push((format!("<html> has no lang attribute on {}", p.url), p.file.clone(), "write <html lang=\"{{ lang }}\"> in the layout".to_string()));
        }
    }
    for (m, f, fix) in warnings {
        r.warn_at(m, &f, None, &fix);
    }
}

pub fn flatten_keys(v: &Value, prefix: &str, out: &mut Vec<String>) {
    if let Value::Map(m) = v {
        for (k, v) in m.iter() {
            let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            match v {
                Value::Map(_) => flatten_keys(v, &key, out),
                _ => out.push(key),
            }
        }
    }
}

fn check_i18n_parity(r: &mut BuildResult) {
    let default = r.cfg.default_language().to_string();
    let mut base = Vec::new();
    if let Some(v) = r.i18n.get(&default) {
        flatten_keys(v, "", &mut base);
    }
    let mut warnings = Vec::new();
    for lang in r.cfg.languages.iter().skip(1) {
        let mut keys = Vec::new();
        if let Some(v) = r.i18n.get(lang) {
            flatten_keys(v, "", &mut keys);
        }
        let mut missing: Vec<&String> = base.iter().filter(|k| !keys.contains(k)).collect();
        missing.sort();
        if !missing.is_empty() {
            let shown: Vec<&str> = missing.iter().take(8).map(|s| s.as_str()).collect();
            let more = if missing.len() > 8 { " ..." } else { "" };
            warnings.push((format!("missing keys present in {default}.json: {}{more}", shown.join(", ")), format!("src/i18n/{lang}.json"),
                format!("add the translated strings to src/i18n/{lang}.json; a page using a missing key fails to build")));
        }
    }
    for (m, f, fix) in warnings {
        r.warn_at(m, &f, None, &fix);
    }
}

pub fn format_report(r: &BuildResult) -> String {
    let mut lines: Vec<String> = Vec::new();
    for e in &r.errors {
        lines.push(format!("error: {}{}", e.where_(), e.message));
        if let Some(x) = e.excerpt(&r.cfg.root) {
            lines.push(x);
        }
        if let Some(fix) = &e.fix {
            lines.push(format!("  fix: {fix}"));
        }
    }
    for w in &r.warnings {
        lines.push(format!("warning: {w}"));
        if let Some(fix) = &w.fix {
            lines.push(format!("  fix: {fix}"));
        }
    }
    for n in &r.notes {
        lines.push(format!("note: {n}"));
    }
    lines.push(format!("{} pages, {} errors, {} warnings", r.pages.len(), r.errors.len(), r.warnings.len()));
    lines.join("\n")
}

pub fn report_json(r: &BuildResult) -> serde_json::Value {
    json!({
        "ok": r.errors.is_empty(),
        "pages": r.pages.len(),
        "errors": r.errors.iter().map(|e| json!({
            "file": e.file, "line": e.line, "message": e.message, "fix": e.fix, "excerpt": e.excerpt(&r.cfg.root),
        })).collect::<Vec<_>>(),
        "warnings": r.warnings.iter().map(|w| json!({
            "file": w.file, "line": w.line, "message": w.message, "fix": w.fix,
        })).collect::<Vec<_>>(),
        "notes": r.notes,
    })
}
