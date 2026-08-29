//! `magehat new page|component|item`: write a correctly shaped file so
//! nobody has to remember the shape.
//!
//!     magehat new page about                 src/pages/about.html
//!     magehat new page about --lang pt-BR    src/pages/about.pt-BR.html
//!     magehat new component card             src/components/card.html
//!     magehat new item blog hello-world      src/content/blog/hello-world.md

use crate::components::load_components;
use crate::config::{load_config, Config};
use crate::errors::{MageError, Result};
use std::path::Path;

pub fn new(root: &Path, kind: &str, args: &[String], lang: Option<&str>) -> Result<String> {
    let cfg = load_config(root)?;
    if let Some(l) = lang {
        if !cfg.languages.iter().any(|x| x == l) {
            return Err(MageError::new(format!("{l:?} is not one of the site languages"))
                .fix(format!("languages in site.toml are {:?}", cfg.languages)));
        }
    }
    let suffix = lang.filter(|l| *l != cfg.default_language()).map(|l| format!(".{l}")).unwrap_or_default();
    match kind {
        "page" => {
            let name = arg(args, 0, "magehat new page <name>", "the name becomes the URL: about -> /about/, blog/index -> /blog/")?;
            let name = name.trim_matches('/').trim_end_matches(".html");
            let layout = layout_tag(root)?;
            let title = titlecase(name.rsplit('/').next().unwrap_or(name));
            let body = format!(
                "<title>{title}</title>\n<meta name=\"description\" content=\"Describe this page in one sentence.\">\n\n<{layout}>\n  <h1>{title}</h1>\n  <p>Content goes here, as plain HTML.</p>\n</{layout}>\n"
            );
            write_new(root, &format!("src/pages/{name}{suffix}.html"), &body)
        }
        "component" => {
            let name = arg(args, 0, "magehat new component <name>", "lowercase letters, digits and dashes; card -> <x-card>")?;
            let name = name.trim_end_matches(".html").to_ascii_lowercase();
            if !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || name.is_empty() {
                return Err(MageError::new(format!("component name {name:?} must be lowercase letters, digits and dashes")));
            }
            let body = format!(
                "<template>\n  <div class=\"{name}\">\n    <h2>{{{{ title }}}}</h2>\n    <slot></slot>\n  </div>\n</template>\n\n<style>\n  .{name} {{ }}\n</style>\n"
            );
            write_new(root, &format!("src/components/{name}.html"), &body)
                .map(|msg| format!("{msg}\nUse it as <x-{name} title=\"...\">content</x-{name}>"))
        }
        "item" => {
            let coll = arg(args, 0, "magehat new item <collection> <id>", "for example: magehat new item blog hello-world")?;
            let id = arg(args, 1, "magehat new item <collection> <id>", "the id becomes the URL slug")?;
            let id = id.trim_end_matches(".md");
            let body = format!(
                "---\ntitle: {}\ndescription: Describe this item in one sentence.\ndate: {}\n---\n\nContent goes here, as Markdown.\n",
                titlecase(id),
                today()
            );
            write_new(root, &format!("src/content/{coll}/{id}{suffix}.md"), &body)
        }
        other => Err(MageError::new(format!("cannot create {other:?}")).fix("magehat new page <name> | component <name> | item <collection> <id>")),
    }
}

fn arg<'a>(args: &'a [String], i: usize, usage: &str, hint: &str) -> Result<&'a str> {
    args.get(i).map(String::as_str).ok_or_else(|| MageError::new(format!("usage: {usage}")).fix(hint))
}

fn layout_tag(root: &Path) -> Result<String> {
    let comps = load_components(root)?;
    Ok(comps.values().find(|c| c.is_document).map(|c| c.tag.clone()).unwrap_or_else(|| "x-base".into()))
}

fn write_new(root: &Path, rel: &str, body: &str) -> Result<String> {
    let path = root.join(rel);
    if path.exists() {
        return Err(MageError::in_file("already exists", rel).fix("edit it, or pick another name"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, body)?;
    Ok(format!("Created {rel}"))
}

fn titlecase(s: &str) -> String {
    s.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .enumerate()
        .map(|(i, w)| {
            let mut c = w.chars();
            match c.next() {
                Some(f) if i == 0 => f.to_uppercase().collect::<String>() + c.as_str(),
                Some(f) => f.to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Today's date as YYYY-MM-DD, from the system clock (only `new` reads the
/// clock; builds never do).
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86400) as i64;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[allow(dead_code)]
fn _config_used(_: &Config) {}
