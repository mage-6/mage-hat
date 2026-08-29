//! Web fonts: a Google Fonts <link> is served from the site itself.
//!
//! The layout keeps the snippet Google hands out:
//!
//!     <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter:wght@400;700&display=swap">
//!
//! The first build fetches that stylesheet, downloads the font files it
//! names into src/assets/fonts/<family>/, writes src/assets/fonts/<slug>.css
//! pointing at them, and rewrites the link to /fonts/<slug>.css. They are
//! source from then on: commit them and no build asks Google again, and no
//! visitor ever connects to Google, which is also a privacy requirement in
//! parts of Europe. Preconnect hints for Google's font hosts are dropped.

use crate::errors::{MageError, Result};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Google serves woff2 with unicode-range subsets only to a modern browser.
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

static LINK_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<link\b[^>]*>").unwrap());
static HREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(\shref\s*=\s*)(["'])([^"']*)(["'])"#).unwrap());
static REL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\srel\s*=\s*["']([^"']*)["']"#).unwrap());
static FACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)(?:/\*\s*([^*]*?)\s*\*/\s*)?@font-face\s*\{(.*?)\}").unwrap());
static FAMILY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"font-family:\s*['"]?([^'";]+?)['"]?\s*;"#).unwrap());
static STYLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"font-style:\s*([^;]+);").unwrap());
static WEIGHT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"font-weight:\s*([^;]+);").unwrap());
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"url\(\s*['"]?([^'")]+?)['"]?\s*\)"#).unwrap());

pub struct Fonts {
    root: PathBuf,
    /// href -> local stylesheet URL, or None once it failed (reported once).
    done: HashMap<String, Option<String>>,
    /// What this build downloaded, for the notes.
    pub fetched: Vec<String>,
}

impl Fonts {
    pub fn new(root: &Path) -> Fonts {
        Fonts { root: root.to_path_buf(), done: HashMap::new(), fetched: Vec::new() }
    }

    pub fn is_google(href: &str) -> bool {
        href.contains("fonts.googleapis.com/css")
    }

    /// Point Google Fonts links in a rendered page at local stylesheets and
    /// drop preconnects to Google's font hosts.
    pub fn localize_page(&mut self, html: &str, file: &str) -> Result<String> {
        let mut out = String::with_capacity(html.len());
        let mut last = 0;
        let mut first_error: Option<MageError> = None;
        for m in LINK_TAG.find_iter(html) {
            let tag = m.as_str();
            let href = HREF.captures(tag).map(|c| c[3].replace("&amp;", "&"));
            let rel = REL.captures(tag).map(|c| c[1].to_ascii_lowercase()).unwrap_or_default();
            let replacement = match href {
                Some(h) if rel.contains("preconnect") && (h.contains("fonts.googleapis.com") || h.contains("fonts.gstatic.com")) => Some(String::new()),
                Some(h) if Self::is_google(&h) => match self.local_css(&h, file) {
                    Ok(Some(local)) => Some(HREF.replace(tag, |c: &regex::Captures| format!("{}{}{local}{}", &c[1], &c[2], &c[4])).into_owned()),
                    Ok(None) => None,
                    Err(e) => {
                        first_error.get_or_insert(e);
                        None
                    }
                },
                _ => None,
            };
            if let Some(rep) = replacement {
                out.push_str(&html[last..m.start()]);
                out.push_str(&rep);
                last = m.end();
            }
        }
        out.push_str(&html[last..]);
        match first_error {
            Some(e) => Err(e),
            None => Ok(out),
        }
    }

    /// The local stylesheet for a Google Fonts href, fetched once. None when
    /// it already failed in this build.
    fn local_css(&mut self, href: &str, file: &str) -> Result<Option<String>> {
        if let Some(done) = self.done.get(href) {
            return Ok(done.clone());
        }
        let slug = slug_for(href);
        let rel = format!("src/assets/fonts/{slug}.css");
        let url = format!("/fonts/{slug}.css");
        if self.root.join(&rel).is_file() {
            self.done.insert(href.into(), Some(url.clone()));
            return Ok(Some(url));
        }
        match fetch_and_write(&self.root, href, &slug) {
            Ok(count) => {
                self.fetched.push(format!("{count} font files for {href} -> {rel}"));
                self.done.insert(href.into(), Some(url.clone()));
                Ok(Some(url))
            }
            Err(mut e) => {
                self.done.insert(href.into(), None);
                e.file.get_or_insert(file.to_string());
                Err(e)
            }
        }
    }
}

fn fetch_and_write(root: &Path, href: &str, slug: &str) -> Result<usize> {
    let css = String::from_utf8_lossy(&fetch(href)?).to_string();
    let (local_css, files) = localize_css(&css, &mut fetch)?;
    let dir = root.join("src/assets/fonts");
    for (name, bytes) in &files {
        write(&dir.join(name), bytes)?;
    }
    write(&dir.join(format!("{slug}.css")), local_css.as_bytes())?;
    Ok(files.len())
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MageError::new(format!("cannot create {}: {e}", parent.display())))?;
    }
    std::fs::write(path, bytes).map_err(|e| MageError::new(format!("cannot write {}: {e}", path.display())))
}

fn fetch(url: &str) -> Result<Vec<u8>> {
    let fix = "connect to the internet once so MageHat can save the fonts under src/assets/fonts, or put the font files and a stylesheet there yourself and link /fonts/<name>.css";
    let mut resp = ureq::get(url)
        .header("User-Agent", UA)
        .call()
        .map_err(|e| MageError::new(format!("could not download {url}: {e}")).fix(fix))?;
    resp.body_mut().read_to_vec().map_err(|e| MageError::new(format!("could not read {url}: {e}")).fix(fix))
}

/// Rewrite Google's stylesheet to local files. Returns the CSS and the files
/// it now refers to, as (path under src/assets/fonts, bytes). Files are
/// named after what the block says, family-weight-style-subset, so the
/// folder reads like the stylesheet.
pub fn localize_css(css: &str, fetch: &mut dyn FnMut(&str) -> Result<Vec<u8>>) -> Result<(String, Vec<(String, Vec<u8>)>)> {
    let mut out = css.to_string();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for m in FACE.captures_iter(css) {
        let subset = m.get(1).map(|s| slug(s.as_str())).filter(|s| !s.is_empty()).unwrap_or_else(|| "all".into());
        let block = &m[2];
        let family = FAMILY.captures(block).map(|c| slug(&c[1])).unwrap_or_else(|| "font".into());
        let style = STYLE.captures(block).map(|c| slug(&c[1])).unwrap_or_else(|| "normal".into());
        let weight = WEIGHT.captures(block).map(|c| slug(&c[1])).unwrap_or_else(|| "400".into());
        for u in URL.captures_iter(block) {
            let url = &u[1];
            if !url.starts_with("http") {
                continue;
            }
            let ext = url.rsplit_once('.').map(|(_, e)| e).unwrap_or("woff2");
            let name = format!("{family}/{family}-{weight}-{style}-{subset}.{ext}");
            if !files.iter().any(|(n, _)| n == &name) {
                files.push((name.clone(), fetch(url)?));
            }
            out = out.replace(url, &format!("/fonts/{name}"));
        }
    }
    if files.is_empty() {
        return Err(MageError::new("the Google Fonts stylesheet names no font files")
            .fix("check the family names in the link; open the link in a browser to see what Google returns"));
    }
    Ok((out, files))
}

/// Lowercase letters, digits and single dashes.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The stylesheet name for a Google Fonts href: its families, in order.
pub fn slug_for(href: &str) -> String {
    let query = href.split_once('?').map(|(_, q)| q).unwrap_or("");
    let families: Vec<String> = query
        .split('&')
        .filter_map(|p| p.strip_prefix("family="))
        .map(|f| slug(&f.split(':').next().unwrap_or("").replace('+', " ")))
        .filter(|s| !s.is_empty())
        .collect();
    if families.is_empty() {
        "fonts".into()
    } else {
        families.join("-")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOGLE_CSS: &str = "/* cyrillic */\n@font-face {\n  font-family: 'Inter';\n  font-style: normal;\n  font-weight: 400;\n  font-display: swap;\n  src: url(https://fonts.gstatic.com/s/inter/v18/abc.woff2) format('woff2');\n  unicode-range: U+0301, U+0400-045F;\n}\n/* latin */\n@font-face {\n  font-family: 'Inter';\n  font-style: italic;\n  font-weight: 100 900;\n  font-display: swap;\n  src: url(https://fonts.gstatic.com/s/inter/v18/def.woff2) format('woff2');\n  unicode-range: U+0000-00FF;\n}\n";

    #[test]
    fn stylesheet_is_localized_and_files_named_after_the_blocks() {
        let mut fetched = Vec::new();
        let (css, files) = localize_css(GOOGLE_CSS, &mut |url| {
            fetched.push(url.to_string());
            Ok(url.as_bytes().to_vec())
        })
        .unwrap();
        assert_eq!(fetched, ["https://fonts.gstatic.com/s/inter/v18/abc.woff2", "https://fonts.gstatic.com/s/inter/v18/def.woff2"]);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["inter/inter-400-normal-cyrillic.woff2", "inter/inter-100-900-italic-latin.woff2"]);
        assert!(css.contains("src: url(/fonts/inter/inter-400-normal-cyrillic.woff2) format('woff2');"), "{css}");
        assert!(!css.contains("gstatic"));
        assert!(localize_css("body { color: red }", &mut |_| Ok(Vec::new())).is_err());
    }

    #[test]
    fn hrefs_name_their_stylesheet() {
        assert_eq!(slug_for("https://fonts.googleapis.com/css2?family=Inter:wght@400;700&family=Playfair+Display:ital@1&display=swap"), "inter-playfair-display");
        assert_eq!(slug_for("https://fonts.googleapis.com/css?family=Lora"), "lora");
        assert_eq!(slug_for("https://fonts.googleapis.com/css2"), "fonts");
    }

    #[test]
    fn links_are_rewritten_and_preconnects_dropped() {
        let dir = std::env::temp_dir().join(format!("magehat-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/assets/fonts")).unwrap();
        std::fs::write(dir.join("src/assets/fonts/inter.css"), "/* local */").unwrap();
        let mut fonts = Fonts::new(&dir);
        let html = "<head>\n<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\n<link href=\"https://fonts.googleapis.com/css2?family=Inter:wght@400;700&amp;display=swap\" rel=\"stylesheet\">\n<link rel=\"stylesheet\" href=\"/site.css\">\n</head>";
        let out = fonts.localize_page(html, "src/pages/index.html").unwrap();
        assert_eq!(out, "<head>\n\n\n<link href=\"/fonts/inter.css\" rel=\"stylesheet\">\n<link rel=\"stylesheet\" href=\"/site.css\">\n</head>");
        assert!(fonts.fetched.is_empty(), "nothing downloaded when the stylesheet exists");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
