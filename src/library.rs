//! `magehat add <name>`: copy a ready-made component into the site.
//!
//! The files live in library/ in the repository and are embedded here, like
//! the scaffold. A site gets a copy under src/components and owns it from
//! then on; a fix or a better technique goes to the library, and a site
//! picks it up by running `add` again on a fresh file. Every entry is built
//! by a test, so a component that stops compiling cannot ship.
//!
//! Each file starts with a comment that is its contract: props, the CSS
//! custom properties it reads, the attributes that switch variants, and the
//! class names of its parts, which never change.

use crate::errors::{MageError, Result};
use std::path::Path;

pub struct Entry {
    pub name: &'static str,
    pub summary: &'static str,
    pub file: &'static str,
}

pub const LIBRARY: &[Entry] = &[Entry {
    name: "faq",
    summary: "accordion of questions: <details> with a CSS-only animation, FAQPage structured data from the same items",
    file: include_str!("../library/faq.html"),
}];

pub fn entry(name: &str) -> Option<&'static Entry> {
    LIBRARY.iter().find(|e| e.name == name)
}

/// The catalogue, one line each, for a bare `magehat add`.
pub fn listing() -> String {
    let width = LIBRARY.iter().map(|e| e.name.len()).max().unwrap_or(0);
    let mut lines = vec!["Ready-made components; `magehat add <name>` copies one into src/components/:".to_string()];
    for e in LIBRARY {
        lines.push(format!("  {:width$}  {}", e.name, e.summary, width = width));
    }
    lines.push(String::new());
    lines.push("The comment at the top of each file is its contract: props, tokens to restyle it, variants, and the class names of its parts.".into());
    lines.join("\n")
}

/// Copy `name` into src/components/<name>.html. Refuses to overwrite.
pub fn add(root: &Path, name: &str) -> Result<String> {
    let Some(e) = entry(name) else {
        let names: Vec<&str> = LIBRARY.iter().map(|e| e.name).collect();
        return Err(MageError::new(format!("no ready-made component named {name:?}")).fix(format!("available: {}; `magehat add` lists them", names.join(", "))));
    };
    let rel = format!("src/components/{}.html", e.name);
    let path = root.join(&rel);
    if path.exists() {
        return Err(MageError::in_file("already exists", &rel).fix("the site already has this component; delete the file first to take a fresh copy"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, e.file)?;
    let usage = e
        .file
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(&format!("<x-{}", e.name)))
        .unwrap_or_default()
        .to_string();
    Ok(format!("Created {rel}\nUse it as {usage}\nThe comment at the top of the file lists its props, tokens and parts."))
}
