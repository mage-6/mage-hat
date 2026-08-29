//! Values that flow through templates: the variable scope chain and the one
//! string type that is allowed to be inserted without escaping.

use indexmap::IndexMap;
use std::collections::HashMap;
use std::rc::Rc;

/// HTML produced by MageHat itself (rendered Markdown, slot content).
///
/// Only this type is inserted raw by {{ }}. Everything else is escaped, so
/// user data can never inject markup. `uses` carries the component tags the
/// fragment depends on, so a page that embeds it gets their CSS and JS.
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlValue {
    pub html: String,
    pub uses: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Html(Rc<HtmlValue>),
    List(Rc<Vec<Value>>),
    Map(Rc<IndexMap<String, Value>>),
}

pub type Map = IndexMap<String, Value>;

impl Value {
    pub fn str(s: impl AsRef<str>) -> Value {
        Value::Str(Rc::from(s.as_ref()))
    }

    pub fn html(html: String, uses: Vec<String>) -> Value {
        Value::Html(Rc::new(HtmlValue { html, uses }))
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(items))
    }

    pub fn map(m: Map) -> Value {
        Value::Map(Rc::new(m))
    }

    pub fn list_of_str(items: &[String]) -> Value {
        Value::list(items.iter().map(Value::str).collect())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(m) => m.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Html(h) => Some(&h.html),
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "a boolean",
            Value::Int(_) | Value::Float(_) => "a number",
            Value::Str(_) => "a string",
            Value::Html(_) => "html",
            Value::List(_) => "a list",
            Value::Map(_) => "an object",
        }
    }

    pub fn from_json(v: &serde_json::Value) -> Value {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(i) => Value::Int(i),
                None => Value::Float(n.as_f64().unwrap_or(0.0)),
            },
            serde_json::Value::String(s) => Value::str(s),
            serde_json::Value::Array(a) => Value::list(a.iter().map(Value::from_json).collect()),
            serde_json::Value::Object(o) => {
                Value::map(o.iter().map(|(k, v)| (k.clone(), Value::from_json(v))).collect())
            }
        }
    }

    pub fn from_toml(v: &toml::Value) -> Value {
        match v {
            toml::Value::String(s) => Value::str(s),
            toml::Value::Integer(i) => Value::Int(*i),
            toml::Value::Float(f) => Value::Float(*f),
            toml::Value::Boolean(b) => Value::Bool(*b),
            toml::Value::Datetime(d) => Value::str(d.to_string()),
            toml::Value::Array(a) => Value::list(a.iter().map(Value::from_toml).collect()),
            toml::Value::Table(t) => Value::map(t.iter().map(|(k, v)| (k.clone(), Value::from_toml(v))).collect()),
        }
    }

    /// Loose equality for `==`: numbers compare numerically, strings by text.
    pub fn loose_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => (*a as f64) == *b,
            (Value::List(a), Value::List(b)) => a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.loose_eq(y)),
            (Value::Map(a), Value::Map(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).map_or(false, |w| v.loose_eq(w)))
            }
            _ => match (self.as_str(), other.as_str()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }
}

pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => false,
        Value::Bool(true) => true,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !s.is_empty(),
        Value::Html(h) => !h.html.is_empty(),
        Value::List(l) => !l.is_empty(),
        Value::Map(m) => !m.is_empty(),
    }
}

/// How a value prints inside {{ }}.
pub fn to_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        Value::Str(s) => s.to_string(),
        Value::Html(h) => h.html.clone(),
        Value::List(l) => l.iter().map(to_text).collect::<Vec<_>>().join(", "),
        Value::Map(_) => "[object]".into(),
    }
}

/// Escape for element content: & < > only (matches Python's html.escape(quote=False)).
pub fn escape_text_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape for attribute values and XML: & < > " ' (matches html.escape(quote=True)).
pub fn escape_attr_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn escape_text(v: &Value) -> String {
    match v {
        Value::Html(h) => h.html.clone(),
        _ => escape_text_str(&to_text(v)),
    }
}

pub fn escape_attr(v: &Value) -> String {
    escape_attr_str(&to_text(v))
}

/// A variable scope. Lookups walk up to the parent; the root holds the page
/// globals (site, t, lang, page, data, collections).
pub struct Ctx<'a> {
    pub vars: Map,
    pub parent: Option<&'a Ctx<'a>>,
    pub slots: Option<HashMap<String, Rc<HtmlValue>>>,
}

impl<'a> Ctx<'a> {
    pub fn root(vars: Map) -> Ctx<'static> {
        Ctx { vars, parent: None, slots: None }
    }

    pub fn child(&'a self, vars: Map) -> Ctx<'a> {
        Ctx { vars, parent: Some(self), slots: None }
    }

    pub fn lookup(&self, name: &str) -> Option<&Value> {
        let mut c = Some(self);
        while let Some(cur) = c {
            if let Some(v) = cur.vars.get(name) {
                return Some(v);
            }
            c = cur.parent;
        }
        None
    }

    pub fn find_slots(&self) -> Option<&HashMap<String, Rc<HtmlValue>>> {
        let mut c = Some(self);
        while let Some(cur) = c {
            if let Some(s) = &cur.slots {
                return Some(s);
            }
            c = cur.parent;
        }
        None
    }
}
