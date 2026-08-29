//! Icons: `<svg icon="lucide:shield">` becomes that icon's SVG, inline.
//!
//! Icons are files, src/icons/<set>/<name>.svg. One that is not there yet is
//! downloaded once from the Iconify API (any of its sets, under the name the
//! Iconify site shows) and saved into that folder. From then on it is source:
//! commit it, and no build touches the network for it again. A file put there
//! by hand works the same way, so a set can also be a site's own SVGs.

use crate::errors::MageError;
use crate::htmltree::scan_start_tag;
use crate::values::escape_attr_str;
use regex::Regex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;

const API: &str = "https://api.iconify.design";

/// An icon file taken apart: the root <svg> attributes and what is inside.
pub struct Svg {
    pub attrs: Vec<(String, Option<String>)>,
    pub inner: String,
}

pub struct Icons {
    root: PathBuf,
    /// Sets that live in a folder of their own ([icons] in site.toml):
    /// set name -> folder relative to the site root. Never downloaded into.
    sets: HashMap<String, String>,
    cache: RefCell<HashMap<String, Result<Rc<Svg>, MageError>>>,
    /// Icons downloaded during this build, as "set:name -> file".
    pub fetched: RefCell<Vec<String>>,
}

impl Icons {
    pub fn new(root: &Path, sets: &indexmap::IndexMap<String, String>) -> Icons {
        Icons {
            root: root.to_path_buf(),
            sets: sets.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            cache: RefCell::new(HashMap::new()),
            fetched: RefCell::new(Vec::new()),
        }
    }

    /// The site-relative file for an icon name, or None when the name is not `set:name`.
    pub fn file(name: &str) -> Option<String> {
        let (set, icon) = name.split_once(':')?;
        let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if ok(set) && ok(icon) {
            Some(format!("src/icons/{set}/{icon}.svg"))
        } else {
            None
        }
    }

    /// The icon, from its file or downloaded into it. Errors carry no
    /// location; the caller adds where the icon was used.
    pub fn get(&self, name: &str) -> Result<Rc<Svg>, MageError> {
        if let Some(r) = self.cache.borrow().get(name) {
            return r.clone();
        }
        let r = self.load(name).map(Rc::new);
        self.cache.borrow_mut().insert(name.to_string(), r.clone());
        r
    }

    fn load(&self, name: &str) -> Result<Svg, MageError> {
        let Some(rel) = Self::file(name) else {
            return Err(MageError::new(format!("{name:?} is not an icon name"))
                .fix("icon names are set:name in lowercase, like lucide:shield or simple-icons:github; browse the sets at https://icon-sets.iconify.design")
                .snippet(name));
        };
        let (set, icon) = name.split_once(':').unwrap_or((name, ""));
        if let Some(dir) = self.sets.get(set) {
            // A set with its own folder is never downloaded into; a missing
            // file there is the author's, not Iconify's.
            let rel = format!("{dir}/{icon}.svg");
            let text = std::fs::read_to_string(self.root.join(&rel)).map_err(|_| {
                MageError::new(format!("no icon named {icon} in the {set} folder ({dir})"))
                    .fix(format!("add {rel}, or check the file name; [icons] in site.toml maps {set} to that folder"))
                    .snippet(name)
            })?;
            return parse_svg(&text).ok_or_else(|| MageError::in_file("not an SVG file", &rel).fix("the file must contain an <svg>...</svg> element"));
        }
        let path = self.root.join(&rel);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                let text = fetch(name, &rel)?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| MageError::new(format!("cannot create {}: {e}", parent.display())))?;
                }
                std::fs::write(&path, &text).map_err(|e| MageError::new(format!("cannot write {rel}: {e}")))?;
                self.fetched.borrow_mut().push(format!("{name} -> {rel}"));
                text
            }
        };
        parse_svg(&text).ok_or_else(|| MageError::in_file("not an SVG file", &rel).fix("the file must contain an <svg>...</svg> element"))
    }
}

fn fetch(name: &str, rel: &str) -> Result<String, MageError> {
    let (set, icon) = name.split_once(':').unwrap_or((name, ""));
    let url = format!("{API}/{set}/{icon}.svg");
    match ureq::get(&url).call() {
        Ok(mut resp) => resp
            .body_mut()
            .read_to_string()
            .map_err(|e| MageError::new(format!("could not read icon {name}: {e}"))),
        Err(ureq::Error::StatusCode(404)) => Err(MageError::new(format!("no icon named {name} in the Iconify sets"))
            .fix(format!("check the name at https://icon-sets.iconify.design/{set}/?query={icon} or put your own SVG at {rel}"))
            .snippet(name)),
        Err(e) => Err(MageError::new(format!("could not download icon {name}: {e}"))
            .fix(format!("connect to the internet once so MageHat can save it to {rel}, or put an SVG file there yourself"))
            .snippet(name)),
    }
}

/// Take an SVG file apart. Anything before the root <svg> (an XML
/// declaration, a comment) is dropped.
pub fn parse_svg(text: &str) -> Option<Svg> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("<svg")?;
    let (len, attrs, self_closing) = scan_start_tag(&text[start..])?;
    // The scanner lowercases attribute names, as HTML does; SVG's viewBox
    // and friends are case-sensitive in the file, so restore their spelling.
    static NAMES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\s([^\s=/>"']+)(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?"#).unwrap());
    let spelled: Vec<&str> = NAMES.captures_iter(&text[start..start + len]).map(|c| c.get(1).unwrap().as_str()).collect();
    let attrs: Vec<(String, Option<String>)> = if spelled.len() == attrs.len() {
        attrs.into_iter().zip(spelled).map(|((_, v), name)| (name.to_string(), v)).collect()
    } else {
        attrs
    };
    if self_closing {
        return Some(Svg { attrs, inner: String::new() });
    }
    let end = lower.rfind("</svg")?;
    if end < start + len {
        return None;
    }
    Some(Svg { attrs, inner: text[start + len..end].trim().to_string() })
}

/// The inline element: the file's root attributes, with the author's on
/// top (same name wins), then the file's content.
pub fn render(svg: &Svg, own: &[(String, String)]) -> String {
    let mut out = String::from("<svg");
    for (k, v) in svg.attrs.iter().filter(|(k, _)| !own.iter().any(|(n, _)| n == k)) {
        match v {
            Some(v) => out.push_str(&format!(" {k}=\"{}\"", escape_attr_str(v))),
            None => out.push_str(&format!(" {k}")),
        }
    }
    for (k, v) in own {
        out.push_str(&format!(" {k}=\"{}\"", escape_attr_str(v)));
    }
    out.push('>');
    out.push_str(&svg.inner);
    out.push_str("</svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_files() {
        assert_eq!(Icons::file("lucide:shield").as_deref(), Some("src/icons/lucide/shield.svg"));
        assert_eq!(Icons::file("simple-icons:github").as_deref(), Some("src/icons/simple-icons/github.svg"));
        assert!(Icons::file("shield").is_none() && Icons::file("Lucide:Shield").is_none() && Icons::file("a:").is_none());
    }

    #[test]
    fn files_are_taken_apart_and_attributes_merged() {
        let svg = parse_svg("<?xml version=\"1.0\"?>\n<!-- c -->\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1em\" height=\"1em\" viewBox=\"0 0 24 24\">\n  <path d=\"M1 1\"/>\n</svg>\n").unwrap();
        assert_eq!(svg.inner, "<path d=\"M1 1\"/>");
        let own = vec![("class".to_string(), "ico".to_string()), ("width".to_string(), "2em".to_string())];
        assert_eq!(
            render(&svg, &own),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" height=\"1em\" viewBox=\"0 0 24 24\" class=\"ico\" width=\"2em\"><path d=\"M1 1\"/></svg>"
        );
        assert!(parse_svg("<p>not svg</p>").is_none());
        assert_eq!(parse_svg("<svg viewBox=\"0 0 1 1\"/>").unwrap().inner, "");
    }
}
