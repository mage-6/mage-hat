//! The renderer: turns a parsed tree plus variables into HTML text.
//!
//! Everything MageHat adds to HTML is handled here:
//!     {{ expr }}                  in text and attribute values
//!     each="item in list"         repeat the element
//!     if="expr"                   keep the element only when true
//!     <x-name ...>                expand a component, its children fill <slot>s
//!     <template each|if>          repeat or drop children with no wrapper
//!
//! Untouched markup is written back exactly as it was read.

use crate::components::Component;
use crate::errors::{MageError, Result};
use crate::expr::eval_str;
use crate::htmltree::{parse, strip_attrs, Element, Node, RAW};
use crate::values::{escape_attr, escape_text, to_text, truthy, Ctx, HtmlValue, Map, Value};
use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

static EXPR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\{\{\s*(.+?)\s*\}\}").unwrap());
static SINGLE_EXPR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)^\s*\{\{\s*(.+?)\s*\}\}\s*$").unwrap());
static EACH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)^\s*([A-Za-z_][A-Za-z0-9_]*)\s+in\s+(.+?)\s*$").unwrap());
static HREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(\shref\s*=\s*)(["'])(/[^"']*)(["'])"#).unwrap());
static BLOCK_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{%\s*(\w+)").unwrap());

pub const DIRECTIVES: &[&str] = &["each", "if"];
const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Element content: escape, let Html through raw.
    Text,
    /// Attribute value: escape everything.
    Attr,
    /// Building a prop string: no escaping (escaped later on output).
    Plain,
}

/// Per-page rendering state.
pub struct Env<'a> {
    pub components: &'a IndexMap<String, Component>,
    pub root: &'a Ctx<'a>,
    pub interpolate: bool,
    pub link_map: Option<&'a HashMap<String, String>>,
    /// Ordered set of component tags used so far.
    pub used: IndexSet<String>,
    pub depth: usize,
}

impl<'a> Env<'a> {
    pub fn new(components: &'a IndexMap<String, Component>, root: &'a Ctx<'a>) -> Env<'a> {
        Env { components, root, interpolate: true, link_map: None, used: IndexSet::new(), depth: 0 }
    }
}

pub fn render_nodes(nodes: &[Node], ctx: &Ctx, env: &mut Env, file: &str) -> Result<String> {
    let mut out = String::new();
    for n in nodes {
        match n {
            Node::Text { text, line } => {
                if env.interpolate {
                    out.push_str(&interpolate(text, ctx, env, Mode::Text, file, *line)?);
                } else {
                    out.push_str(text);
                }
            }
            Node::Raw { text, .. } => out.push_str(text),
            Node::Element(el) => out.push_str(&render_element(el, ctx, env, file, &[])?),
        }
    }
    Ok(out)
}

pub fn render_fragment(text: &str, ctx: &Ctx, env: &mut Env, file: &str) -> Result<String> {
    render_nodes(&parse(text), ctx, env, file)
}

pub fn render_element(el: &Element, ctx: &Ctx, env: &mut Env, file: &str, strip: &[&str]) -> Result<String> {
    if !env.interpolate {
        return render_plain(el, ctx, env, file, strip);
    }
    check_foreign_attrs(el, file)?;
    let Some(each) = el.attr("each") else {
        return render_one(el, ctx, env, file, strip);
    };
    let Some(m) = EACH.captures(each) else {
        return Err(MageError::at(format!("each must look like each=\"item in list\", got {each:?}"), file, el.line)
            .fix("write each=\"post in blog\" (a variable name, `in`, then the list)")
            .snippet(each));
    };
    let var = &m[1];
    let expr = &m[2];
    let items = eval_str(expr, ctx, false, file, el.line)?;
    let items: Vec<Value> = match &items {
        Value::List(l) => l.iter().cloned().collect(),
        Value::Map(mm) => mm.values().cloned().collect(),
        Value::Null => Vec::new(),
        other => {
            return Err(MageError::at(format!("each expects a list, {expr:?} is {}", other.type_name()), file, el.line)
                .fix("loop over a collection (blog), a list from metadata (post.tags) or from src/data")
                .snippet(expr))
        }
    };
    let mut out = String::new();
    for item in items {
        let mut vars = Map::new();
        vars.insert(var.to_string(), item);
        let child = ctx.child(vars);
        out.push_str(&render_one(el, &child, env, file, strip)?);
    }
    Ok(out)
}

fn render_one(el: &Element, ctx: &Ctx, env: &mut Env, file: &str, strip: &[&str]) -> Result<String> {
    if let Some(cond) = el.attr("if") {
        if !truthy(&eval_str(cond, ctx, true, file, el.line)?) {
            return Ok(String::new());
        }
    }
    if el.tag == "template" && (el.has_attr("each") || el.has_attr("if")) {
        return render_nodes(&el.children, ctx, env, file);
    }
    let comps = env.components;
    if let Some(comp) = comps.get(&el.tag) {
        return render_component(el, ctx, env, comp, file);
    }
    if el.tag.starts_with("x-") {
        let name = &el.tag[2..];
        return Err(MageError::at(format!("unknown component <{}>", el.tag), file, el.line)
            .fix(format!(
                "create src/components/{}.html with a <template> (or run `magehat new component {}`), or use one of: {}",
                name.replace('-', "/"),
                name,
                if comps.is_empty() { "(none defined yet)".to_string() } else { comps.keys().map(|k| format!("<{k}>")).collect::<Vec<_>>().join(", ") }
            ))
            .snippet(format!("<{}", el.tag)));
    }
    if el.tag == "slot" {
        if let Some(slots) = ctx.find_slots() {
            let name = el.attr("name").unwrap_or("");
            if let Some(html) = slots.get(name) {
                return Ok(html.html.clone());
            }
            return render_nodes(&el.children, ctx, env, file);
        }
    }
    let mut strip_all: Vec<&str> = strip.to_vec();
    strip_all.extend_from_slice(DIRECTIVES);
    render_plain(el, ctx, env, file, &strip_all)
}

fn render_plain(el: &Element, ctx: &Ctx, env: &mut Env, file: &str, strip: &[&str]) -> Result<String> {
    if !env.interpolate {
        let comps = env.components;
        if let Some(comp) = comps.get(&el.tag) {
            return render_component(el, ctx, env, comp, file);
        }
    }
    let mut start = strip_attrs(&el.start_text, strip);
    // Localize literal links before interpolation: an href computed from a
    // value (a language switcher, an item url) is already the right one.
    if let Some(map) = env.link_map {
        start = HREF
            .replace_all(&start, |c: &regex::Captures| format!("{}{}{}{}", &c[1], &c[2], localize(&c[3], map), &c[4]))
            .into_owned();
    }
    if env.interpolate {
        start = interpolate(&start, ctx, env, Mode::Attr, file, el.line)?;
    }
    let inner = if RAW.contains(&el.tag.as_str()) {
        el.children
            .iter()
            .filter_map(|c| match c {
                Node::Text { text, .. } | Node::Raw { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>()
    } else {
        render_nodes(&el.children, ctx, env, file)?
    };
    let mut out = start;
    out.push_str(&inner);
    if el.closed {
        out.push_str("</");
        out.push_str(&el.tag);
        out.push('>');
    }
    Ok(out)
}

fn localize(href: &str, map: &HashMap<String, String>) -> String {
    let cut = href.find(['?', '#']).unwrap_or(href.len());
    let (path, tail) = href.split_at(cut);
    match map.get(path) {
        Some(target) => format!("{target}{tail}"),
        None => href.to_string(),
    }
}

fn render_component(el: &Element, ctx: &Ctx, env: &mut Env, comp: &Component, file: &str) -> Result<String> {
    if env.depth >= MAX_DEPTH {
        return Err(MageError::at(format!("component nesting too deep at <{}>", comp.tag), file, el.line)
            .fix("a component is using itself without end; add an if=\"...\" that stops the recursion"));
    }
    env.used.insert(comp.tag.clone()); // outer component first, so its CSS loads first
    let mut props = Map::new();
    let mut kept: Vec<(String, String)> = Vec::new();
    for (name, raw) in &el.attrs {
        if DIRECTIVES.contains(&name.as_str()) || name == "slot" {
            continue;
        }
        let raw = raw.as_deref().unwrap_or("");
        let value = match SINGLE_EXPR.captures(raw).filter(|_| env.interpolate) {
            Some(m) => eval_str(&m[1], ctx, false, file, el.line)?,
            None if env.interpolate => Value::str(interpolate(raw, ctx, env, Mode::Plain, file, el.line)?),
            None => Value::str(raw),
        };
        // Objects are for the template, not for the output tag.
        if !matches!(value, Value::Null | Value::List(_) | Value::Map(_)) {
            kept.push((name.clone(), to_text(&value)));
        }
        props.insert(name.clone(), value);
    }

    let mut slots: HashMap<String, Rc<HtmlValue>> = HashMap::new();
    let mut default_html = String::new();
    let mut named: IndexMap<String, Vec<&Element>> = IndexMap::new();
    for child in &el.children {
        match child {
            Node::Element(c) if c.attr("slot").map_or(false, |s| !s.is_empty()) => {
                named.entry(c.attr("slot").unwrap().to_string()).or_default().push(c);
            }
            Node::Text { text, line } => {
                if env.interpolate {
                    default_html.push_str(&interpolate(text, ctx, env, Mode::Text, file, *line)?);
                } else {
                    default_html.push_str(text);
                }
            }
            Node::Raw { text, .. } => default_html.push_str(text),
            Node::Element(c) => default_html.push_str(&render_element(c, ctx, env, file, &[])?),
        }
    }
    for (name, elements) in named {
        let mut html = String::new();
        for e in elements {
            html.push_str(&render_element(e, ctx, env, file, &["slot"])?);
        }
        slots.insert(name, Rc::new(HtmlValue { html, uses: Vec::new() }));
    }
    if !default_html.trim().is_empty() {
        slots.insert(String::new(), Rc::new(HtmlValue { html: default_html, uses: Vec::new() }));
    }

    let comp_ctx = Ctx { vars: props, parent: Some(env.root), slots: Some(slots) };
    env.depth += 1;
    // A component template is always a template, even when it is expanded
    // from content that is not (a Markdown body embedding <x-figure>).
    let was_interpolating = env.interpolate;
    env.interpolate = true;
    let body = render_nodes(&comp.template, &comp_ctx, env, &comp.file);
    env.interpolate = was_interpolating;
    env.depth -= 1;
    let body = body.map_err(|mut e| {
        if !e.message.contains("used from") {
            e.message.push_str(&format!(" (<{}> used from {file}:{})", comp.tag, el.line));
        }
        e
    })?;
    if comp.is_document {
        return Ok(body);
    }
    let attrs: String = kept.iter().map(|(n, v)| format!(" {n}=\"{}\"", crate::values::escape_attr_str(v))).collect();
    Ok(format!("<{}{attrs}>{body}</{}>", comp.tag, comp.tag))
}

/// Replace {{ expr }} in text.
pub fn interpolate(text: &str, ctx: &Ctx, env: &mut Env, mode: Mode, file: &str, line: usize) -> Result<String> {
    check_foreign_text(text, file, line)?;
    if !text.contains("{{") {
        return Ok(text.to_string());
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in EXPR.captures_iter(text) {
        let whole = m.get(0).unwrap();
        out.push_str(&text[last..whole.start()]);
        let at = if line > 0 { line + text[..whole.start()].matches('\n').count() } else { line };
        let value = eval_str(&m[1], ctx, false, file, at)?;
        match mode {
            Mode::Text => {
                if let Value::Html(h) = &value {
                    for tag in &h.uses {
                        env.used.insert(tag.clone());
                    }
                }
                out.push_str(&escape_text(&value));
            }
            Mode::Attr => out.push_str(&escape_attr(&value)),
            Mode::Plain => out.push_str(&to_text(&value)),
        }
        last = whole.end();
    }
    out.push_str(&text[last..]);
    Ok(out)
}

/// Template syntax from other tools, answered with the MageHat equivalent.
fn check_foreign_text(text: &str, file: &str, line: usize) -> Result<()> {
    if let Some(m) = BLOCK_TAG.captures(text) {
        let at = line + text[..m.get(0).unwrap().start()].matches('\n').count();
        let word = &m[1];
        let fix = match word {
            "if" | "elif" | "else" | "endif" => "put if=\"expr\" on the element to keep or drop (use <template if=\"...\"> to wrap several); there is no else, write a second element with if=\"not expr\"",
            "for" | "endfor" => "put each=\"item in list\" on the element to repeat (or on a <template> to repeat several)",
            "include" | "extends" | "block" | "macro" | "import" | "set" | "with" => "there are no includes or inheritance; make a component in src/components and use it as <x-name>; a layout is a component with a <slot>",
            _ => "MageHat has no {% %} blocks; its only syntax is {{ expr }}, each=\"item in list\", if=\"expr\" and <x-component> tags",
        };
        return Err(MageError::at(format!("{{% {word} %}} is not MageHat syntax"), file, at).fix(fix).snippet(m[0].to_string()));
    }
    if let Some(pos) = text.find("{{") {
        if !text[pos..].contains("}}") {
            let at = line + text[..pos].matches('\n').count();
            return Err(MageError::at("unclosed {{", file, at).fix("close the expression with }}").snippet("{{"));
        }
    }
    Ok(())
}

fn check_foreign_attrs(el: &Element, file: &str) -> Result<()> {
    for (name, _) in &el.attrs {
        let fix = match name.as_str() {
            "v-for" | "x-for" | "*ngfor" | "ng-repeat" => Some("use each=\"item in list\""),
            "v-if" | "x-if" | "x-show" | "*ngif" | "ng-if" => Some("use if=\"expr\""),
            "v-else" | "v-else-if" | "x-else" => Some("there is no else; write a second element with if=\"not expr\""),
            "v-bind" | "v-model" | "v-on" | "v-html" | "v-text" | "x-text" | "x-html" | "x-data" | "x-bind" | "x-on" => {
                Some("write the value into the attribute or text with {{ expr }}; browser behaviour goes in a component <script>")
            }
            n if n.starts_with(':') || n.starts_with('@') || n.starts_with("v-") => {
                Some("write the value into the attribute with {{ expr }}; browser behaviour goes in a component <script>")
            }
            _ => None,
        };
        if let Some(fix) = fix {
            return Err(MageError::at(format!("{name:?} is not MageHat syntax"), file, el.line).fix(fix).snippet(name.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::load_components;
    use std::path::Path;

    fn setup(dir: &Path, comps: &[(&str, &str)]) -> IndexMap<String, Component> {
        let base = dir.join("src").join("components");
        std::fs::create_dir_all(&base).unwrap();
        for (name, src) in comps {
            std::fs::write(base.join(format!("{name}.html")), src).unwrap();
        }
        load_components(dir).unwrap()
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("magehat-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn vars(pairs: Vec<(&str, Value)>) -> Map {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    fn render(comps: &IndexMap<String, Component>, root: &Ctx, html: &str) -> Result<String> {
        let mut env = Env::new(comps, root);
        render_fragment(html, root, &mut env, "test.html")
    }

    #[test]
    fn escapes_text_and_attributes_but_not_html() {
        let d = tmp("esc");
        let comps = setup(&d, &[]);
        let root = Ctx::root(vars(vec![("a", Value::str("<b>")), ("h", Value::html("<b>x</b>".into(), vec![])), ("q", Value::str("x\"y"))]));
        assert_eq!(render(&comps, &root, "<p title=\"{{ q }}\">{{ a }} {{ h }}</p>").unwrap(), "<p title=\"x&quot;y\">&lt;b&gt; <b>x</b></p>");
    }

    #[test]
    fn each_if_and_template() {
        let d = tmp("each");
        let comps = setup(&d, &[]);
        let items = vec![
            Value::map(vars(vec![("n", Value::Int(1)), ("on", Value::Bool(true))])),
            Value::map(vars(vec![("n", Value::Int(2)), ("on", Value::Bool(false))])),
        ];
        let root = Ctx::root(vars(vec![("xs", Value::list(items))]));
        assert_eq!(render(&comps, &root, "<ul><li each=\"x in xs\" if=\"x.on\">{{ x.n }}</li></ul>").unwrap(), "<ul><li>1</li></ul>");
        assert_eq!(render(&comps, &root, "<template each=\"x in xs\"><i>{{ x.n }}</i></template>").unwrap(), "<i>1</i><i>2</i>");
        assert_eq!(render(&comps, &root, "<b if=\"nothing\">y</b><b if=\"xs.9.n\">z</b>").unwrap(), "");
        let e = render(&comps, &root, "<p>\n\n{{ nope }}</p>").unwrap_err();
        assert_eq!((e.line, e.file.as_deref()), (Some(3), Some("test.html")));
    }

    #[test]
    fn components_props_slots_wrapper() {
        let d = tmp("comp");
        let comps = setup(&d, &[
            ("card", "<template><h2>{{ title }}</h2><slot>fallback</slot><aside><slot name=\"side\"></slot></aside></template>"),
            ("list", "<template><i each=\"x in items\">{{ x }}</i></template>"),
            ("c", "<template>{{ x }}</template>"),
            ("base", "<template><!doctype html><html><body><slot></slot></body></html></template>"),
        ]);
        let post = Value::map(vars(vec![("title", Value::str("A")), ("n", Value::Int(3))]));
        let root = Ctx::root(vars(vec![("post", post), ("xs", Value::list(vec![Value::str("a"), Value::str("b")]))]));
        assert_eq!(
            render(&comps, &root, "<x-card title=\"{{ post.title }}\" count=\"{{ post.n }}\"><p>body</p><i slot=\"side\">s</i></x-card>").unwrap(),
            "<x-card title=\"A\" count=\"3\"><h2>A</h2><p>body</p><aside><i>s</i></aside></x-card>"
        );
        assert_eq!(render(&comps, &root, "<x-card title=\"B\"></x-card>").unwrap(), "<x-card title=\"B\"><h2>B</h2>fallback<aside></aside></x-card>");
        assert_eq!(render(&comps, &root, "<x-list items=\"{{ xs }}\"></x-list>").unwrap(), "<x-list><i>a</i><i>b</i></x-list>");
        assert_eq!(render(&comps, &root, "<x-base>hi</x-base>").unwrap(), "<!doctype html><html><body>hi</body></html>");
        let e = render(&comps, &root, "<template each=\"x in xs\"><x-c></x-c></template>").unwrap_err();
        assert!(e.message.contains("undefined variable 'x'") && e.message.contains("used from test.html:1"));
        assert!(render(&comps, &root, "<x-nope></x-nope>").unwrap_err().message.contains("unknown component <x-nope>"));
    }

    #[test]
    fn raw_elements_links_and_content_mode() {
        let d = tmp("raw");
        let comps = setup(&d, &[("tag", "<template>#{{ name }}</template>")]);
        let root = Ctx::root(vars(vec![("home", Value::str("/"))]));
        let src = "<script>var a = '{{ x }}';</script><style>a::before{content:'{{ y }}'}</style>";
        assert_eq!(render(&comps, &root, src).unwrap(), src);
        let mut map = HashMap::new();
        map.insert("/".to_string(), "/pt-br/".to_string());
        map.insert("/about/".to_string(), "/pt-br/sobre/".to_string());
        let mut env = Env::new(&comps, &root);
        env.link_map = Some(&map);
        let out = render_fragment("<a href=\"/about/#x\">a</a><a href=\"{{ home }}\">b</a><a href=\"/other/\">c</a>", &root, &mut env, "t").unwrap();
        assert_eq!(out, "<a href=\"/pt-br/sobre/#x\">a</a><a href=\"/\">b</a><a href=\"/other/\">c</a>");
        let mut env = Env::new(&comps, &root);
        env.interpolate = false;
        let out = render_fragment("<p>{{ raw }} <x-tag name=\"k\"></x-tag></p>", &root, &mut env, "b.md").unwrap();
        assert_eq!(out, "<p>{{ raw }} <x-tag name=\"k\">#k</x-tag></p>");
        assert_eq!(env.used.iter().collect::<Vec<_>>(), vec!["x-tag"]);
    }

    #[test]
    fn foreign_syntax_is_explained() {
        let d = tmp("foreign");
        let comps = setup(&d, &[]);
        let root = Ctx::root(Map::new());
        let e = render(&comps, &root, "{% for x in xs %}").unwrap_err();
        assert!(e.message.contains("{% for %}") && e.fix.unwrap().contains("each="));
        let e = render(&comps, &root, "<li v-for=\"x in xs\">").unwrap_err();
        assert!(e.fix.unwrap().contains("each="));
    }
}
