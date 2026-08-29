//! `magehat init [dir]`: write the sample site.
//!
//! The sample site is embedded in the binary, so init works anywhere with no
//! files to look up. The same files live in scaffold/ in the repository and
//! are the golden-test fixture.

use crate::errors::{MageError, Result};
use std::path::Path;

const SCAFFOLD: &[(&str, &str)] = &[
    ("site.toml", include_str!("../scaffold/site.toml")),
    (".gitignore", include_str!("../scaffold/_gitignore")),
    ("src/assets/site.css", include_str!("../scaffold/src/assets/site.css")),
    ("src/components/base.html", include_str!("../scaffold/src/components/base.html")),
    ("src/components/card.html", include_str!("../scaffold/src/components/card.html")),
    ("src/components/counter.html", include_str!("../scaffold/src/components/counter.html")),
    ("src/components/nav.html", include_str!("../scaffold/src/components/nav.html")),
    ("src/content/blog/hello-world.md", include_str!("../scaffold/src/content/blog/hello-world.md")),
    ("src/content/blog/second-post.md", include_str!("../scaffold/src/content/blog/second-post.md")),
    ("src/i18n/en.json", include_str!("../scaffold/src/i18n/en.json")),
    ("src/icons/lucide/crown.svg", include_str!("../scaffold/src/icons/lucide/crown.svg")),
    ("src/pages/404.html", include_str!("../scaffold/src/pages/404.html")),
    ("src/pages/about.html", include_str!("../scaffold/src/pages/about.html")),
    ("src/pages/blog/[post].html", include_str!("../scaffold/src/pages/blog/[post].html")),
    ("src/pages/blog/index.html", include_str!("../scaffold/src/pages/blog/index.html")),
    ("src/pages/index.html", include_str!("../scaffold/src/pages/index.html")),
];

const SCAFFOLD_BINARY: &[(&str, &[u8])] = &[
    ("src/assets/photos/hat.jpg", include_bytes!("../scaffold/src/assets/photos/hat.jpg")),
];

/// Create a site in `target`. Returns the files written, relative to it.
pub fn init_site(target: &Path) -> Result<Vec<String>> {
    std::fs::create_dir_all(target)?;
    if target.join("site.toml").exists() {
        return Err(MageError::in_file("site.toml already exists here; init only creates new sites", "site.toml")
            .fix("run `magehat init <new-folder>` or delete the existing site first"));
    }
    let mut written = Vec::new();
    for (rel, content) in SCAFFOLD {
        write(target, rel, content)?;
        written.push(rel.to_string());
    }
    for (rel, bytes) in SCAFFOLD_BINARY {
        let path = target.join(rel);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, bytes)?;
        written.push(rel.to_string());
    }
    Ok(written)
}

fn write(target: &Path, rel: &str, content: &str) -> Result<()> {
    let path = target.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    Ok(())
}
