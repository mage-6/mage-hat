//! Metadata at the top of content and page files.
//!
//! Markdown files use a `---` block with a small, fixed YAML subset:
//!     key: value            strings, numbers, true/false, null
//!     key: "quoted string"
//!     tags: [a, b, "c d"]   inline list
//!     tags:                 block list
//!       - a
//!       - b
//!
//! HTML files use real HTML instead: a leading <title> and <meta name=.. content=..>
//! elements. Their values go through the same scalar parser, so `content="[a, b]"`
//! is a list and `content="true"` is a boolean.

use crate::errors::{MageError, Result};
use crate::values::{Map, Value};

pub fn parse_scalar(text: &str) -> Value {
    let text = text.trim();
    if text.is_empty() {
        return Value::str("");
    }
    let bytes = text.as_bytes();
    if text.len() >= 2 && (bytes[0] == b'"' || bytes[0] == b'\'') && bytes[0] == bytes[text.len() - 1] {
        return Value::str(&text[1..text.len() - 1]);
    }
    if text.starts_with('[') && text.ends_with(']') {
        let inner = text[1..text.len() - 1].trim();
        if inner.is_empty() {
            return Value::list(Vec::new());
        }
        return Value::list(split_list(inner).iter().map(|p| parse_scalar(p)).collect());
    }
    match text.to_ascii_lowercase().as_str() {
        "true" | "yes" => return Value::Bool(true),
        "false" | "no" => return Value::Bool(false),
        "null" | "~" => return Value::Null,
        _ => {}
    }
    if let Ok(i) = text.parse::<i64>() {
        if text.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Value::Int(i);
        }
    }
    if text.contains('.') {
        if let Ok(f) = text.parse::<f64>() {
            if text.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '.') {
                return Value::Float(f);
            }
        }
    }
    Value::str(text)
}

fn split_list(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut quote: Option<char> = None;
    for ch in inner.chars() {
        match quote {
            Some(q) => {
                buf.push(ch);
                if ch == q {
                    quote = None;
                }
            }
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                buf.push(ch);
            }
            None if ch == ',' => {
                parts.push(std::mem::take(&mut buf));
            }
            None => buf.push(ch),
        }
    }
    parts.push(buf);
    parts.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

/// Split a `---` metadata block off the top. Returns (metadata, body, body start line).
pub fn split_frontmatter(text: &str, file: &str) -> Result<(Map, String, usize)> {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return Ok((Map::new(), text.to_string(), 1));
    }
    let mut meta = Map::new();
    let mut key: Option<String> = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            let body = lines[i + 1..].join("\n");
            return Ok((meta, body, i + 2));
        }
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let trimmed = line.trim_start();
        if line.starts_with(char::is_whitespace) && trimmed.starts_with('-') {
            if let Some(k) = &key {
                if let Some(Value::List(list)) = meta.get(k) {
                    let mut items = (**list).clone();
                    items.push(parse_scalar(trimmed[1..].trim()));
                    meta.insert(k.clone(), Value::list(items));
                    continue;
                }
            }
        }
        let Some((k, v)) = line.split_once(':') else {
            return Err(MageError::at(format!("cannot parse metadata line {line:?}"), file, i + 1)
                .fix("metadata lines look like `key: value`; lists are `tags: [a, b]`"));
        };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(MageError::at(format!("cannot parse metadata line {line:?}"), file, i + 1)
                .fix("metadata keys use letters, digits, - and _"));
        }
        let value = if v.trim().is_empty() { Value::list(Vec::new()) } else { parse_scalar(v) };
        meta.insert(k.to_string(), value);
        key = Some(k.to_string());
    }
    Err(MageError::at("metadata block is not closed", file, 1).fix("end the block with a line containing only ---"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_yaml_subset() {
        let (m, body, line) = split_frontmatter(
            "---\ntitle: \"Hi: there\"\ndate: 2026-01-02\ntags: [a, \"b c\"]\nlist:\n  - x\n  - 2\ndraft: false\n---\nBody",
            "f.md",
        )
        .unwrap();
        assert_eq!(m["title"].as_str(), Some("Hi: there"));
        assert_eq!(m["date"].as_str(), Some("2026-01-02"));
        assert!(matches!(&m["tags"], Value::List(l) if l.len() == 2 && l[1].as_str() == Some("b c")));
        assert!(matches!(&m["list"], Value::List(l) if matches!(l[1], Value::Int(2))));
        assert!(matches!(m["draft"], Value::Bool(false)));
        assert_eq!(body, "Body");
        assert_eq!(line, 10);
    }
}
