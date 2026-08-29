//! site.toml: the only configuration file.
//!
//!     name = "My site"
//!     url = "https://example.com"       # needed for canonical, sitemap, feeds
//!     languages = ["en", "pt-BR"]       # first one is the default
//!
//!     [collections.blog]
//!     feed = true                       # write /blog/feed.xml
//!
//!     [assets]
//!     brand = "../brand/svg"            # a folder outside src/assets, served at /brand/
//!
//!     [icons]
//!     brand = "../brand/svg"            # a folder outside src/icons, used as icon="brand:x"
//!
//! Any other top-level key is exposed to templates as site.<key>.

use crate::errors::{MageError, Result};
use crate::values::{Map, Value};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

pub const RESERVED_GLOBALS: &[&str] = &["site", "t", "lang", "page", "data"];

#[derive(Debug, Clone)]
pub struct Config {
    pub root: PathBuf,
    pub name: String,
    pub url: String,
    pub languages: Vec<String>,
    pub collections: IndexMap<String, toml::Table>,
    /// Extra asset folders: output folder name -> path relative to the site root.
    pub assets: IndexMap<String, String>,
    /// Extra icon sets: set name -> folder of SVGs, relative to the site root.
    pub icons: IndexMap<String, String>,
    pub extra: toml::Table,
}

impl Config {
    pub fn default_language(&self) -> &str {
        &self.languages[0]
    }

    pub fn src(&self) -> PathBuf {
        self.root.join("src")
    }

    pub fn dist(&self) -> PathBuf {
        self.root.join("dist")
    }

    /// URL prefix for a language: "" for the default, "/pt-br" otherwise.
    pub fn lang_prefix(&self, lang: &str) -> String {
        if lang == self.default_language() {
            String::new()
        } else {
            format!("/{}", lang.to_lowercase())
        }
    }

    pub fn site_vars(&self) -> Value {
        let mut m: Map = self.extra.iter().map(|(k, v)| (k.clone(), Value::from_toml(v))).collect();
        m.insert("name".into(), Value::str(&self.name));
        m.insert("url".into(), Value::str(&self.url));
        m.insert("languages".into(), Value::list_of_str(&self.languages));
        m.insert("default_language".into(), Value::str(self.default_language()));
        Value::map(m)
    }

    pub fn collection_option(&self, coll: &str, key: &str) -> Option<&toml::Value> {
        self.collections.get(coll).and_then(|t| t.get(key))
    }
}

pub fn load_config(root: &Path) -> Result<Config> {
    let path = root.join("site.toml");
    if !path.is_file() {
        return Err(MageError::in_file("site.toml not found", "site.toml")
            .fix("create it as shown under \"A site from nothing\" in `magehat -h`, or run `magehat init` for a sample site"));
    }
    let text = std::fs::read_to_string(&path)?;
    let mut table: toml::Table = toml::from_str(&text)
        .map_err(|e| MageError::in_file(format!("invalid site.toml: {}", e.message()), "site.toml"))?;
    let languages: Vec<String> = match table.remove("languages") {
        None => vec!["en".into()],
        Some(toml::Value::Array(a)) if !a.is_empty() && a.iter().all(|v| v.is_str()) => {
            a.iter().map(|v| v.as_str().unwrap().to_string()).collect()
        }
        Some(_) => {
            return Err(MageError::in_file("languages must be a non-empty list of language codes", "site.toml")
                .fix("languages = [\"en\"] or languages = [\"en\", \"pt-BR\"]; the first is the default"))
        }
    };
    let mut seen = std::collections::HashSet::new();
    for l in &languages {
        if !seen.insert(l) {
            return Err(MageError::in_file(format!("languages lists {l:?} twice"), "site.toml"));
        }
    }
    let collections: IndexMap<String, toml::Table> = match table.remove("collections") {
        None => IndexMap::new(),
        Some(toml::Value::Table(t)) => {
            let mut out = IndexMap::new();
            for (k, v) in t {
                if RESERVED_GLOBALS.contains(&k.as_str()) {
                    return Err(MageError::in_file(format!("collection name {k:?} is reserved"), "site.toml"));
                }
                match v {
                    toml::Value::Table(inner) => { out.insert(k, inner); }
                    _ => return Err(MageError::in_file(format!("[collections.{k}] must be a table"), "site.toml")),
                }
            }
            out
        }
        Some(_) => return Err(MageError::in_file("[collections] must be a table", "site.toml")),
    };
    let assets = folder_table(&mut table, "assets", "served at /<name>/")?;
    let icons = folder_table(&mut table, "icons", "used as icon=\"<name>:file\"")?;
    let url = table.remove("url").and_then(|v| v.as_str().map(|s| s.trim_end_matches('/').to_string())).unwrap_or_default();
    let name = table
        .remove("name")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| root.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "site".into()));
    Ok(Config { root: root.to_path_buf(), name, url, languages, collections, assets, icons, extra: table })
}

/// `[assets]` or `[icons]`: names mapped to folders. The folders must exist,
/// so a mapping that points nowhere fails here rather than building a site
/// with silently missing files.
fn folder_table(table: &mut toml::Table, key: &str, role: &str) -> Result<IndexMap<String, String>> {
    let mut out = IndexMap::new();
    let Some(value) = table.remove(key) else { return Ok(out) };
    let toml::Value::Table(t) = value else {
        return Err(MageError::in_file(format!("[{key}] must be a table of name = \"folder\""), "site.toml")
            .fix(format!("[{key}]\nbrand = \"../brand/svg\"   # {role}")));
    };
    for (name, v) in t {
        let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !ok {
            return Err(MageError::in_file(format!("[{key}] name {name:?} must be lowercase letters, digits and dashes"), "site.toml"));
        }
        let Some(dir) = v.as_str() else {
            return Err(MageError::in_file(format!("[{key}] {name} must be a folder path in quotes"), "site.toml"));
        };
        out.insert(name, dir.trim_end_matches(['/', '\\']).to_string());
    }
    Ok(out)
}
