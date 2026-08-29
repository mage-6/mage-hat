//! The build: source folder in, a map of output files out.
//!
//! Everything is computed in memory first, so `check` can inspect the result
//! without writing, and `build` writes it in one go. A full rebuild every time
//! keeps the output a pure function of the source.

use crate::components::{digest, load_components, scoped_css, walk_files, Component};
use crate::config::{load_config, Config};
use crate::content::{load_collections, split_html_meta, Collections, Item};
use crate::errors::{MageError, Result};
use crate::htmltree::{parse, Node};
use crate::pages::{discover_pages, output_path, page_url, resolve, PageSource};
use crate::render::{interpolate, render_fragment, render_nodes, Env, Mode};
use crate::seo::{alternate_links, inject_head, robots_txt, rss_xml, sitemap_xml, FeedItem, SitemapEntry, Translation};
use crate::values::{to_text, Ctx, Map, Value};
use indexmap::{IndexMap, IndexSet};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub const ASSET_DIR: &str = "_mh";

#[derive(Debug, Clone)]
pub struct BuiltPage {
    pub identity: String,
    pub lang: String,
    pub url: String,
    pub file: String,
    pub out: String,
    pub translations: Vec<Translation>,
    pub item_id: Option<String>,
    pub lastmod: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Warning {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub fix: Option<String>,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.file, self.line) {
            (Some(file), Some(line)) => write!(f, "{file}:{line}: {}", self.message),
            (Some(file), None) => write!(f, "{file}: {}", self.message),
            _ => write!(f, "{}", self.message),
        }
    }
}

pub struct BuildResult {
    pub cfg: Config,
    pub outputs: BTreeMap<String, Vec<u8>>,
    pub pages: Vec<BuiltPage>,
    pub errors: Vec<MageError>,
    pub warnings: Vec<Warning>,
    /// Things the build did that are worth knowing (an icon downloaded).
    pub notes: Vec<String>,
    pub components: IndexMap<String, Component>,
    pub collections: Collections,
    pub sources: Vec<PageSource>,
    pub i18n: IndexMap<String, Value>,
    pub data: Value,
}

impl BuildResult {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn warn(&mut self, message: impl Into<String>, file: Option<&str>, fix: Option<&str>) {
        self.warnings.push(Warning { message: message.into(), file: file.map(String::from), line: None, fix: fix.map(String::from) });
    }

    pub fn warn_at(&mut self, message: impl Into<String>, file: &str, line: Option<usize>, fix: &str) {
        self.warnings.push(Warning { message: message.into(), file: Some(file.to_string()), line, fix: Some(fix.to_string()) });
    }

    /// Where image variants are cached between builds.
    pub fn cache_dir(&self) -> std::path::PathBuf {
        self.cfg.root.join(".magehat").join("cache").join("img")
    }
}

pub fn build_site(root: &Path) -> Result<BuildResult> {
    let cfg = load_config(root)?;
    let components = load_components(root)?;
    let i18n = load_i18n(&cfg)?;
    let data = load_data(&cfg)?;
    let collections = load_collections(&cfg)?;
    let sources = discover_pages(&cfg)?;
    let mut result = BuildResult {
        cfg,
        outputs: BTreeMap::new(),
        pages: Vec::new(),
        errors: Vec::new(),
        warnings: Vec::new(),
        notes: Vec::new(),
        components,
        collections,
        sources,
        i18n,
        data,
    };
    Builder::run(&mut result);
    Ok(result)
}

/// Write outputs and remove files from an earlier build that no longer exist.
pub fn write_outputs(result: &BuildResult, dist: &Path) -> Result<()> {
    std::fs::create_dir_all(dist)?;
    for (path, rel) in walk_files(dist, dist) {
        if !result.outputs.contains_key(&rel) {
            std::fs::remove_file(&path)?;
        }
    }
    for (rel, data) in &result.outputs {
        let path = dist.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::read(&path).map_or(true, |old| &old != data) {
            std::fs::write(&path, data)?;
        }
    }
    remove_empty_dirs(dist);
    Ok(())
}

fn remove_empty_dirs(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                remove_empty_dirs(&p);
                let _ = std::fs::remove_dir(&p); // fails when not empty, which is fine
            }
        }
    }
}

fn load_i18n(cfg: &Config) -> Result<IndexMap<String, Value>> {
    let mut out = IndexMap::new();
    for lang in &cfg.languages {
        let rel = format!("src/i18n/{lang}.json");
        let path = cfg.root.join(&rel);
        if path.is_file() {
            let text = std::fs::read_to_string(&path)?;
            let json: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| MageError::at(format!("invalid JSON: {e}"), &rel, e.line()).fix("translation files are plain JSON objects, nested by key"))?;
            if !json.is_object() {
                return Err(MageError::in_file("translation file must contain an object", &rel).fix("{ \"nav\": { \"home\": \"Home\" } }"));
            }
            out.insert(lang.clone(), Value::from_json(&json));
        } else {
            out.insert(lang.clone(), Value::map(Map::new()));
        }
    }
    Ok(out)
}

fn load_data(cfg: &Config) -> Result<Value> {
    let mut out = Map::new();
    let base = cfg.src().join("data");
    if !base.is_dir() {
        return Ok(Value::map(out));
    }
    for (path, rel) in walk_files(&cfg.root, &base) {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        match path.extension().and_then(|e| e.to_str()) {
            Some("json") => {
                let text = std::fs::read_to_string(&path)?;
                let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| MageError::at(format!("invalid JSON: {e}"), &rel, e.line()))?;
                out.insert(stem, Value::from_json(&json));
            }
            Some("toml") => {
                let text = std::fs::read_to_string(&path)?;
                let t: toml::Value = toml::from_str(&text).map_err(|e| MageError::in_file(format!("invalid TOML: {}", e.message()), &rel))?;
                out.insert(stem, Value::from_toml(&t));
            }
            _ => {}
        }
    }
    Ok(Value::map(out))
}

struct Instance {
    identity: String,
    lang: String,
    source: usize,
    url: String,
    /// The item as planned; bodies are rendered later, so the page render
    /// looks the item up again by collection and id.
    item: Option<Value>,
    collection: Option<String>,
    item_var: Option<String>,
    translations: Vec<Translation>,
    item_id: Option<String>,
    lastmod: Option<String>,
}

struct Builder<'r> {
    r: &'r mut BuildResult,
    trees: HashMap<String, Vec<Node>>,
    used_all: IndexSet<String>,
    /// collection -> (page identity, item variable)
    item_pages: IndexMap<String, (String, String)>,
    /// lang -> collection -> item dicts (without body until bodies are rendered)
    items: IndexMap<String, IndexMap<String, Vec<Map>>>,
    link_maps: HashMap<String, HashMap<String, String>>,
    asset_map: crate::assets::AssetMap,
    icons: crate::icons::Icons,
    fonts: crate::fonts::Fonts,
}

impl<'r> Builder<'r> {
    fn run(result: &'r mut BuildResult) {
        let icons = crate::icons::Icons::new(&result.cfg.root);
        let fonts = crate::fonts::Fonts::new(&result.cfg.root);
        let mut b = Builder {
            r: result,
            trees: HashMap::new(),
            used_all: IndexSet::new(),
            item_pages: IndexMap::new(),
            items: IndexMap::new(),
            link_maps: HashMap::new(),
            asset_map: crate::assets::AssetMap::new(),
            icons,
            fonts,
        };
        b.find_item_pages();
        b.build_item_dicts();
        let instances = b.plan_instances();
        b.build_link_maps(&instances);
        let languages = b.r.cfg.languages.clone();
        for lang in &languages {
            b.render_bodies(lang);
            let root = b.root_ctx(lang);
            for inst in instances.iter().filter(|i| &i.lang == lang) {
                b.render_instance(inst, &root);
            }
        }
        for fetched in b.icons.fetched.borrow().iter() {
            b.r.notes.push(format!("downloaded icon {fetched}; commit the file"));
        }
        // Before the assets are copied, so font files saved now ship now.
        b.localize_fonts();
        let asset_keys = b.copy_assets();
        b.asset_map = crate::assets::plan(b.r, &asset_keys);
        b.write_component_assets();
        let cache = b.r.cache_dir();
        crate::images::Images::new(b.r, &cache).process();
        b.finish_pages();
        b.write_feeds();
        b.write_sitemap_and_robots();
    }

    /// Google Fonts links become local stylesheets (fonts.rs).
    fn localize_fonts(&mut self) {
        let pages: Vec<(String, String)> = self.r.pages.iter().map(|p| (p.out.clone(), p.file.clone())).collect();
        for (out, file) in pages {
            let html = String::from_utf8_lossy(&self.r.outputs[&out]).to_string();
            if !html.contains("fonts.googleapis.com") && !html.contains("fonts.gstatic.com") {
                continue;
            }
            match self.fonts.localize_page(&html, &file) {
                Ok(html) => {
                    self.r.outputs.insert(out, html.into_bytes());
                }
                Err(e) => self.r.errors.push(e),
            }
        }
        for fetched in self.fonts.fetched.drain(..) {
            self.r.notes.push(format!("downloaded {fetched}; commit the files"));
        }
    }

    /// Point every page at hashed assets, then minify.
    fn finish_pages(&mut self) {
        let pages: Vec<String> = self.r.pages.iter().map(|p| p.out.clone()).collect();
        for out in pages {
            let html = String::from_utf8_lossy(&self.r.outputs[&out]).to_string();
            let html = crate::assets::rewrite_html(&html, &out, &self.asset_map);
            self.r.outputs.insert(out, crate::minify::html(&html).into_bytes());
        }
    }

    // -- planning

    fn find_item_pages(&mut self) {
        let sources = self.r.sources.clone();
        for s in sources.iter().filter(|s| s.item_var.is_some()) {
            let coll = s.collection.clone().unwrap();
            let var = s.item_var.clone().unwrap();
            if !self.r.collections.contains_key(&coll) {
                self.r.errors.push(MageError::in_file(format!("no collection named {coll:?} in src/content for this item page"), &s.file)
                    .fix(format!("create src/content/{coll}/ with at least one .md or .html item, or move this page")));
                continue;
            }
            if let Some((prev, _)) = self.item_pages.get(&coll) {
                if prev != &s.identity {
                    self.r.errors.push(MageError::in_file(format!("collection {coll:?} already has an item page ({prev})"), &s.file)
                        .fix("a collection has exactly one [item] page; remove one of them"));
                    continue;
                }
            }
            self.item_pages.insert(coll, (s.identity.clone(), var));
        }
    }

    fn item_translations(&self, item: &Item, by_lang: &IndexMap<String, Vec<Item>>) -> Vec<Translation> {
        let Some((identity, _)) = self.item_pages.get(&item.collection) else { return Vec::new() };
        let mut out = Vec::new();
        for lang in &self.r.cfg.languages {
            if let Some(other) = by_lang[lang].iter().find(|i| i.id == item.id) {
                if resolve(&self.r.sources, identity, lang, &self.r.cfg).is_some() {
                    out.push(Translation { lang: lang.clone(), url: page_url(&self.r.cfg, identity, lang, Some(&other.slug())) });
                }
            }
        }
        out
    }

    fn item_dict(&self, item: &Item, by_lang: &IndexMap<String, Vec<Item>>) -> Map {
        let translations = self.item_translations(item, by_lang);
        let url = translations.iter().find(|t| t.lang == item.lang).map(|t| Value::str(&t.url)).unwrap_or(Value::Null);
        let mut d = item.meta.clone();
        d.insert("id".into(), Value::str(&item.id));
        d.insert("lang".into(), Value::str(&item.lang));
        d.insert("url".into(), url);
        d.insert("translations".into(), translations_value(&translations));
        d.insert("body".into(), Value::html(String::new(), Vec::new()));
        if !d.contains_key("title") {
            d.insert("title".into(), Value::str(&item.id));
        }
        d
    }

    fn build_item_dicts(&mut self) {
        for lang in self.r.cfg.languages.clone() {
            let mut per_coll = IndexMap::new();
            for (coll, by_lang) in &self.r.collections {
                let dicts: Vec<Map> = by_lang[&lang].iter().map(|item| self.item_dict(item, by_lang)).collect();
                per_coll.insert(coll.clone(), dicts);
            }
            self.items.insert(lang, per_coll);
        }
    }

    fn plan_instances(&mut self) -> Vec<Instance> {
        let mut out = Vec::new();
        let mut identities: Vec<String> = Vec::new();
        for s in &self.r.sources {
            if s.item_var.is_none() && !identities.contains(&s.identity) {
                identities.push(s.identity.clone());
            }
        }
        let cfg = &self.r.cfg;
        for identity in &identities {
            for lang in &cfg.languages {
                if let Some(src) = resolve(&self.r.sources, identity, lang, cfg) {
                    let translations: Vec<Translation> = cfg
                        .languages
                        .iter()
                        .filter(|l| resolve(&self.r.sources, identity, l, cfg).is_some())
                        .map(|l| Translation { lang: l.clone(), url: page_url(cfg, identity, l, None) })
                        .collect();
                    out.push(Instance {
                        identity: identity.clone(),
                        lang: lang.clone(),
                        source: self.r.sources.iter().position(|p| std::ptr::eq(p, src)).unwrap(),
                        url: page_url(cfg, identity, lang, None),
                        item: None,
                        collection: None,
                        item_var: None,
                        translations,
                        item_id: None,
                        lastmod: None,
                    });
                }
            }
        }
        for (coll, (identity, var)) in &self.item_pages {
            for lang in &cfg.languages {
                let Some(src) = resolve(&self.r.sources, identity, lang, cfg) else { continue };
                let source = self.r.sources.iter().position(|p| std::ptr::eq(p, src)).unwrap();
                for d in &self.items[lang][coll] {
                    let Some(url) = d.get("url").and_then(|u| u.as_str()) else { continue };
                    out.push(Instance {
                        identity: identity.clone(),
                        lang: lang.clone(),
                        source,
                        url: url.to_string(),
                        item: Some(Value::map(d.clone())),
                        collection: Some(coll.clone()),
                        item_var: Some(var.clone()),
                        translations: translations_from_value(&d["translations"]),
                        item_id: d.get("id").map(to_text),
                        lastmod: d.get("date").map(to_text).filter(|s| !s.is_empty()),
                    });
                }
            }
        }
        out
    }

    /// Map each page's default-language URL to its URL in every other
    /// language, so links written once are localized on every page.
    fn build_link_maps(&mut self, instances: &[Instance]) {
        let default = self.r.cfg.default_language().to_string();
        for lang in &self.r.cfg.languages {
            if *lang == default {
                continue;
            }
            let mut m = HashMap::new();
            for inst in instances.iter().filter(|i| &i.lang == lang) {
                let key = if inst.item.is_none() {
                    Some(page_url(&self.r.cfg, &inst.identity, &default, None))
                } else {
                    inst.translations.iter().find(|t| t.lang == default).map(|t| t.url.clone())
                };
                if let Some(key) = key {
                    m.insert(key, inst.url.clone());
                }
            }
            self.link_maps.insert(lang.clone(), m);
        }
    }

    // -- rendering

    fn root_ctx(&self, lang: &str) -> Ctx<'static> {
        let mut vars = Map::new();
        vars.insert("site".into(), self.r.cfg.site_vars());
        vars.insert("t".into(), self.r.i18n.get(lang).cloned().unwrap_or(Value::map(Map::new())));
        vars.insert("lang".into(), Value::str(lang));
        vars.insert("data".into(), self.r.data.clone());
        for (coll, dicts) in &self.items[lang] {
            vars.insert(coll.clone(), Value::list(dicts.iter().cloned().map(Value::map).collect()));
        }
        Ctx::root(vars)
    }

    fn render_bodies(&mut self, lang: &str) {
        let root = self.root_ctx(lang);
        let mut rendered: Vec<(String, usize, String, Vec<String>)> = Vec::new();
        for (coll, by_lang) in &self.r.collections {
            for (i, item) in by_lang[lang].iter().enumerate() {
                let mut env = Env::new(&self.r.components, &root);
                env.interpolate = false;
                env.link_map = self.link_maps.get(lang);
                env.icons = Some(&self.icons);
                match render_fragment(&item.body_html, &root, &mut env, &item.file) {
                    Ok(html) => rendered.push((coll.clone(), i, html, env.used.iter().cloned().collect())),
                    Err(e) => {
                        self.r.errors.push(e);
                        rendered.push((coll.clone(), i, String::new(), Vec::new()));
                    }
                }
            }
        }
        for (coll, i, html, uses) in rendered {
            self.items[lang][&coll][i].insert("body".into(), Value::html(html, uses));
        }
    }

    fn tree(&mut self, file: &str, path: &Path) -> Result<&Vec<Node>> {
        if !self.trees.contains_key(file) {
            let text = std::fs::read_to_string(path)?;
            self.trees.insert(file.to_string(), parse(&text));
        }
        Ok(&self.trees[file])
    }

    fn render_instance(&mut self, inst: &Instance, root: &Ctx) {
        let source = self.r.sources[inst.source].clone();
        let file = source.file.clone();
        let rendered = self.render_page(inst, root, &source);
        let (html, used) = match rendered {
            Ok(x) => x,
            Err(e) => {
                self.r.errors.push(e);
                return;
            }
        };
        let out = output_path(&inst.url);
        if self.r.outputs.contains_key(&out) {
            let other = self.r.pages.iter().find(|p| p.out == out).map(|p| p.file.clone()).unwrap_or_else(|| "an asset".into());
            self.r.errors.push(MageError::in_file(format!("URL {} is produced twice (also by {other})", inst.url), &file)
                .fix("two pages or items resolve to the same URL; rename one, or give the item a different slug"));
            return;
        }
        self.r.outputs.insert(out.clone(), html.into_bytes());
        for tag in &used {
            self.used_all.insert(tag.clone());
        }
        self.r.pages.push(BuiltPage {
            identity: inst.identity.clone(),
            lang: inst.lang.clone(),
            url: inst.url.clone(),
            file,
            out,
            translations: inst.translations.clone(),
            item_id: inst.item_id.clone(),
            lastmod: inst.lastmod.clone(),
        });
    }

    fn render_page(&mut self, inst: &Instance, root: &Ctx, source: &PageSource) -> Result<(String, Vec<String>)> {
        let file = source.file.clone();
        let tree = self.tree(&file, &source.path)?.clone();
        let (meta, start, _) = split_html_meta(&tree);
        // Fresh copy of the item: the planned one predates body rendering.
        let item: Option<Value> = match (&inst.collection, &inst.item_id) {
            (Some(coll), Some(id)) => self.items[&inst.lang][coll]
                .iter()
                .find(|d| d.get("id").and_then(|v| v.as_str()) == Some(id))
                .map(|d| Value::map(d.clone())),
            _ => None,
        };
        let mut page = Map::new();
        page.insert("title".into(), Value::Null);
        page.insert("description".into(), Value::Null);
        if let Some(item) = &item {
            for key in ["title", "description", "date"] {
                page.insert(key.into(), item.get(key).cloned().unwrap_or(Value::Null));
            }
        }
        page.insert("id".into(), Value::str(&inst.identity));
        page.insert("url".into(), Value::str(&inst.url));
        page.insert("lang".into(), Value::str(&inst.lang));
        page.insert("file".into(), Value::str(&file));
        page.insert("translations".into(), translations_value(&inst.translations));

        // Metadata may use expressions ({{ post.title }} in a <title>), so it
        // is interpolated in the page's own scope before `page` is final.
        let mut item_vars = Map::new();
        if let (Some(var), Some(item)) = (&inst.item_var, &item) {
            item_vars.insert(var.clone(), item.clone());
        }
        let components = &self.r.components;
        let link_map = self.link_maps.get(&inst.lang);
        let meta_ctx = root.child(item_vars.clone());
        {
            let mut env = Env::new(components, root);
            env.link_map = link_map;
            for (key, value) in meta {
                let value = match &value {
                    Value::Str(s) => Value::str(interpolate(s, &meta_ctx, &mut env, Mode::Plain, &file, 1)?),
                    _ => value,
                };
                page.insert(key, value);
            }
        }
        let mut page_vars = Map::new();
        page_vars.insert("page".into(), Value::map(page.clone()));
        let page_ctx = root.child(page_vars);
        let ctx = page_ctx.child(item_vars);
        let mut env = Env::new(components, &page_ctx);
        env.link_map = link_map;
        env.icons = Some(&self.icons);
        let html = render_nodes(&tree[start..], &ctx, &mut env, &file)?;
        let html = html.trim_start().to_string();
        let lines = self.head_lines(&env.used, &inst.url, &inst.translations, &html);
        Ok((inject_head(&html, &lines), env.used.iter().cloned().collect()))
    }

    fn head_lines(&self, used: &IndexSet<String>, url: &str, translations: &[Translation], html: &str) -> Vec<String> {
        let mut lines = Vec::new();
        for tag in used {
            let comp = &self.r.components[tag];
            if !comp.style.is_empty() {
                lines.push(format!("<link rel=\"stylesheet\" href=\"/{ASSET_DIR}/{}\">", self.css_name(comp)));
            }
            if !comp.script.is_empty() {
                lines.push(format!("<script type=\"module\" src=\"/{ASSET_DIR}/{}\"></script>", self.js_name(comp)));
            }
        }
        let cfg = &self.r.cfg;
        if !cfg.url.is_empty() && !html.contains("rel=\"canonical\"") {
            lines.push(format!("<link rel=\"canonical\" href=\"{}{url}\">", cfg.url));
        }
        if !html.contains("hreflang=\"x-default\"") {
            lines.extend(alternate_links(translations, &cfg.url, cfg.default_language()));
        }
        lines
    }

    // -- assets and generated files

    fn all_tags(&self) -> Vec<String> {
        self.r.components.keys().cloned().collect()
    }

    fn css_text(&self, comp: &Component) -> String {
        let css = scoped_css(comp, &self.all_tags());
        crate::minify::css(&crate::assets::rewrite_css(&css, &format!("{ASSET_DIR}/x.css"), &self.asset_map))
    }

    fn css_name(&self, comp: &Component) -> String {
        format!("{}.{}.css", comp.tag, digest(&self.css_text(comp)))
    }

    fn js_name(&self, comp: &Component) -> String {
        format!("{}.{}.js", comp.tag, digest(&comp.script))
    }

    fn write_component_assets(&mut self) {
        for tag in self.used_all.clone() {
            let comp = self.r.components[&tag].clone();
            if !comp.style.is_empty() {
                self.r.outputs.insert(format!("{ASSET_DIR}/{}", self.css_name(&comp)), self.css_text(&comp).into_bytes());
            }
            if !comp.script.is_empty() {
                self.r.outputs.insert(format!("{ASSET_DIR}/{}", self.js_name(&comp)), format!("{}\n", comp.script).into_bytes());
            }
        }
    }

    /// Copy src/assets to the site root. Returns the output keys added.
    fn copy_assets(&mut self) -> Vec<String> {
        let mut keys = Vec::new();
        let base = self.r.cfg.src().join("assets");
        if !base.is_dir() {
            return keys;
        }
        for (path, rel) in walk_files(&base, &base) {
            if self.r.outputs.contains_key(&rel) {
                self.r.errors.push(MageError::in_file(format!("asset collides with a generated page: /{rel}"), &format!("src/assets/{rel}"))
                    .fix("rename the asset or the page"));
                continue;
            }
            match std::fs::read(&path) {
                Ok(bytes) => {
                    self.r.outputs.insert(rel.clone(), bytes);
                    keys.push(rel);
                }
                Err(e) => self.r.errors.push(MageError::in_file(format!("cannot read asset: {e}"), &format!("src/assets/{rel}"))),
            }
        }
        keys
    }

    fn write_feeds(&mut self) {
        let cfg = self.r.cfg.clone();
        for (coll, opts) in &cfg.collections {
            if !opts.get("feed").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }
            if !self.r.collections.contains_key(coll) {
                self.r.warn(format!("site.toml enables a feed for {coll:?} but src/content/{coll} does not exist"), Some("site.toml"),
                    Some(&format!("create src/content/{coll}/ with items, or remove [collections.{coll}]")));
                continue;
            }
            if cfg.url.is_empty() {
                self.r.warn(format!("feed for {coll:?} skipped"), Some("site.toml"), Some("set url = \"https://your-domain\" in site.toml"));
                continue;
            }
            let description = opts.get("description").and_then(|v| v.as_str()).unwrap_or(&cfg.name).to_string();
            for lang in &cfg.languages {
                let items: Vec<FeedItem> = self.items[lang][coll]
                    .iter()
                    .filter(|d| d.get("url").and_then(|u| u.as_str()).is_some())
                    .map(|d| FeedItem {
                        title: d.get("title").map(to_text).unwrap_or_default(),
                        url: format!("{}{}", cfg.url, d["url"].as_str().unwrap()),
                        date: d.get("date").map(to_text).filter(|s| !s.is_empty()),
                        text: d.get("description").map(to_text).filter(|s| !s.is_empty()).unwrap_or_else(|| d.get("body").map(to_text).unwrap_or_default()),
                    })
                    .collect();
                if items.is_empty() {
                    continue;
                }
                let base = format!("{}/{coll}/", cfg.lang_prefix(lang));
                let xml = rss_xml(&cfg.name, &format!("{}{base}", cfg.url), &description, lang, &items);
                self.r.outputs.insert(output_path(&format!("{base}feed.xml")), xml.into_bytes());
            }
        }
    }

    fn write_sitemap_and_robots(&mut self) {
        if !self.r.outputs.contains_key("robots.txt") {
            self.r.outputs.insert("robots.txt".into(), robots_txt(&self.r.cfg.url).into_bytes());
        }
        if self.r.cfg.url.is_empty() {
            self.r.warn("sitemap.xml and canonical links skipped", Some("site.toml"), Some("set url = \"https://your-domain\" in site.toml"));
            return;
        }
        let entries: Vec<SitemapEntry> = self
            .r
            .pages
            .iter()
            .filter(|p| !p.url.ends_with("404.html"))
            .map(|p| SitemapEntry { url: p.url.clone(), lastmod: p.lastmod.clone(), translations: p.translations.clone() })
            .collect();
        self.r.outputs.insert("sitemap.xml".into(), sitemap_xml(&entries, &self.r.cfg.url).into_bytes());
    }
}

fn translations_value(t: &[Translation]) -> Value {
    Value::list(
        t.iter()
            .map(|t| {
                let mut m = Map::new();
                m.insert("lang".into(), Value::str(&t.lang));
                m.insert("url".into(), Value::str(&t.url));
                Value::map(m)
            })
            .collect(),
    )
}

fn translations_from_value(v: &Value) -> Vec<Translation> {
    match v {
        Value::List(l) => l
            .iter()
            .filter_map(|m| Some(Translation { lang: m.get("lang")?.as_str()?.to_string(), url: m.get("url")?.as_str()?.to_string() }))
            .collect(),
        _ => Vec::new(),
    }
}
