//! The expression language inside {{ }}, `if` and `each`.
//!
//! Deliberately tiny and frozen: dotted paths, string and number literals,
//! true/false/null, `not`, `and`, `or`, `==`, `!=`, parentheses. Nothing
//! else. Syntax borrowed from other template languages is recognised and
//! answered with the MageHat way of doing it.

use crate::errors::{MageError, Result};
use crate::values::{truthy, Ctx, Value};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Str(String),
    Num(String),
    Name(String),
    Op(&'static str),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Value),
    Path(String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Eq(Box<Expr>, Box<Expr>),
    Ne(Box<Expr>, Box<Expr>),
}

/// A parse failure: what went wrong and, when the input looks like another
/// language, how to say it in MageHat.
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub fix: Option<String>,
}

fn foreign(message: &str, fix: &str) -> ParseError {
    ParseError { message: message.to_string(), fix: Some(fix.to_string()) }
}

fn plain(message: String) -> ParseError {
    ParseError { message, fix: None }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn tokenize(src: &str) -> std::result::Result<Vec<Tok>, ParseError> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            let start = i + 1;
            let mut j = start;
            while j < chars.len() && chars[j] != c {
                j += 1;
            }
            if j >= chars.len() {
                return Err(plain("unclosed string literal".into()));
            }
            out.push(Tok::Str(chars[start..j].iter().collect()));
            i = j + 1;
            continue;
        }
        let starts_number = c.is_ascii_digit() || (c == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit());
        if starts_number {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            out.push(Tok::Num(chars[start..i].iter().collect()));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (is_name_char(chars[i]) || chars[i] == '.') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            if i < chars.len() && chars[i] == '(' {
                return Err(foreign(
                    &format!("function calls do not exist: {name}(...)"),
                    "MageHat has no functions or filters; put the computed value in the page metadata, the content metadata or a file in src/data, then print it directly",
                ));
            }
            out.push(Tok::Name(name));
            continue;
        }
        let two: String = chars[i..chars.len().min(i + 2)].iter().collect();
        match two.as_str() {
            "==" => { out.push(Tok::Op("==")); i += 2; continue; }
            "!=" => { out.push(Tok::Op("!=")); i += 2; continue; }
            "&&" => { out.push(Tok::Op("and")); i += 2; continue; }
            "||" => { out.push(Tok::Op("or")); i += 2; continue; }
            "<=" | ">=" => return Err(foreign("comparisons other than == and != do not exist", "only == and != are available; precompute anything else in the content metadata or src/data")),
            _ => {}
        }
        match c {
            '(' => { out.push(Tok::Op("(")); i += 1; }
            ')' => { out.push(Tok::Op(")")); i += 1; }
            '!' => { out.push(Tok::Op("not")); i += 1; }
            '|' => return Err(foreign("filters do not exist ({{ value | filter }})", "print the value as it is; if it needs transforming, store the transformed value in the metadata or src/data")),
            '+' | '*' | '/' | '%' | '-' => return Err(foreign("arithmetic does not exist in expressions", "store the computed value in the content metadata or src/data and print it directly")),
            '[' => return Err(foreign("indexing with [ ] does not exist", "use a dotted path: items.0 or post.tags.1")),
            '<' | '>' => return Err(foreign("comparisons other than == and != do not exist", "only == and != are available; precompute anything else in the content metadata or src/data")),
            '?' | ':' => return Err(foreign("the ternary operator does not exist", "use two elements with if=\"cond\" and if=\"not cond\"")),
            _ => return Err(plain(format!("unexpected character {c:?}"))),
        }
    }
    Ok(out)
}

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn take(&mut self) -> std::result::Result<Tok, ParseError> {
        let tok = self.t.get(self.i).cloned().ok_or_else(|| plain("unexpected end of expression".into()))?;
        self.i += 1;
        Ok(tok)
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Name(n)) if n == w) || matches!(self.peek(), Some(Tok::Op(o)) if *o == w)
    }

    fn parse(&mut self) -> std::result::Result<Expr, ParseError> {
        if self.t.is_empty() {
            return Err(plain("empty expression".into()));
        }
        let e = self.p_or()?;
        if let Some(tok) = self.peek() {
            return Err(plain(format!("unexpected {}", describe(tok))));
        }
        Ok(e)
    }

    fn p_or(&mut self) -> std::result::Result<Expr, ParseError> {
        let mut left = self.p_and()?;
        while self.is_word("or") {
            self.i += 1;
            left = Expr::Or(Box::new(left), Box::new(self.p_and()?));
        }
        Ok(left)
    }

    fn p_and(&mut self) -> std::result::Result<Expr, ParseError> {
        let mut left = self.p_not()?;
        while self.is_word("and") {
            self.i += 1;
            left = Expr::And(Box::new(left), Box::new(self.p_not()?));
        }
        Ok(left)
    }

    fn p_not(&mut self) -> std::result::Result<Expr, ParseError> {
        if self.is_word("not") {
            self.i += 1;
            return Ok(Expr::Not(Box::new(self.p_not()?)));
        }
        self.p_cmp()
    }

    fn p_cmp(&mut self) -> std::result::Result<Expr, ParseError> {
        let left = self.p_primary()?;
        match self.peek() {
            Some(Tok::Op("==")) => { self.i += 1; Ok(Expr::Eq(Box::new(left), Box::new(self.p_primary()?))) }
            Some(Tok::Op("!=")) => { self.i += 1; Ok(Expr::Ne(Box::new(left), Box::new(self.p_primary()?))) }
            _ => Ok(left),
        }
    }

    fn p_primary(&mut self) -> std::result::Result<Expr, ParseError> {
        match self.take()? {
            Tok::Str(s) => Ok(Expr::Lit(Value::str(s))),
            Tok::Num(n) => Ok(Expr::Lit(if n.contains('.') {
                Value::Float(n.parse().map_err(|_| plain(format!("bad number {n}")))?)
            } else {
                Value::Int(n.parse().map_err(|_| plain(format!("bad number {n}")))?)
            })),
            Tok::Op("(") => {
                let e = self.p_or()?;
                match self.take()? {
                    Tok::Op(")") => Ok(e),
                    _ => Err(plain("missing )".into())),
                }
            }
            Tok::Name(n) => match n.as_str() {
                "true" => Ok(Expr::Lit(Value::Bool(true))),
                "false" => Ok(Expr::Lit(Value::Bool(false))),
                "null" | "none" | "None" => Ok(Expr::Lit(Value::Null)),
                "and" | "or" | "not" => Err(plain(format!("unexpected {n}"))),
                _ => Ok(Expr::Path(n)),
            },
            tok => Err(plain(format!("unexpected {}", describe(&tok)))),
        }
    }
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Str(s) => format!("{s:?}"),
        Tok::Num(n) | Tok::Name(n) => n.clone(),
        Tok::Op(o) => o.to_string(),
    }
}

pub fn parse_expr(src: &str) -> std::result::Result<Expr, ParseError> {
    Parser { t: tokenize(src)?, i: 0 }.parse()
}

pub enum EvalError {
    Undefined(String),
}

/// Walk a dotted path. The first segment must be a known variable. Later
/// segments that are missing are errors, except in lenient mode (used by
/// `if`), where anything missing simply reads as null so optional metadata
/// can be tested without erroring.
fn resolve_path(path: &str, ctx: &Ctx, lenient: bool) -> std::result::Result<Value, EvalError> {
    let mut parts = path.split('.');
    let first = parts.next().unwrap_or("");
    let mut value = match ctx.lookup(first) {
        Some(v) => v.clone(),
        None if lenient => return Ok(Value::Null),
        None => return Err(EvalError::Undefined(first.to_string())),
    };
    let mut seen = first.to_string();
    for key in parts {
        seen.push('.');
        seen.push_str(key);
        let next = match &value {
            Value::Map(m) => m.get(key).cloned(),
            Value::List(l) => key.parse::<usize>().ok().and_then(|i| l.get(i).cloned()),
            _ => None,
        };
        match next {
            Some(v) => value = v,
            None if lenient => return Ok(Value::Null),
            None => return Err(EvalError::Undefined(seen)),
        }
    }
    Ok(value)
}

pub fn evaluate(e: &Expr, ctx: &Ctx, lenient: bool) -> std::result::Result<Value, EvalError> {
    Ok(match e {
        Expr::Lit(v) => v.clone(),
        Expr::Path(p) => resolve_path(p, ctx, lenient)?,
        Expr::Not(a) => Value::Bool(!truthy(&evaluate(a, ctx, lenient)?)),
        Expr::And(a, b) => {
            let l = evaluate(a, ctx, lenient)?;
            if truthy(&l) { evaluate(b, ctx, lenient)? } else { l }
        }
        Expr::Or(a, b) => {
            // `a or b` is how a default is written, so a missing `a` is
            // false here rather than an error: {{ page.image or site.image }}.
            let l = evaluate(a, ctx, true)?;
            if truthy(&l) { l } else { evaluate(b, ctx, lenient)? }
        }
        Expr::Eq(a, b) => Value::Bool(evaluate(a, ctx, lenient)?.loose_eq(&evaluate(b, ctx, lenient)?)),
        Expr::Ne(a, b) => Value::Bool(!evaluate(a, ctx, lenient)?.loose_eq(&evaluate(b, ctx, lenient)?)),
    })
}

/// Names every scope can see; listed in the "undefined" fix so the reader
/// knows what is available.
pub fn undefined_error(path: &str, ctx: &Ctx, file: &str, line: usize) -> MageError {
    let root = path.split('.').next().unwrap_or(path);
    if root == "t" {
        let key = path.strip_prefix("t.").unwrap_or("");
        return MageError::at(format!("missing translation t.{key}"), file, line)
            .fix(format!("add \"{key}\" (nested by dots) to every file in src/i18n/"));
    }
    let mut names: Vec<String> = Vec::new();
    let mut c = Some(ctx);
    while let Some(cur) = c {
        for k in cur.vars.keys() {
            if !names.contains(k) {
                names.push(k.clone());
            }
        }
        c = cur.parent;
    }
    let fix = if path == root {
        format!("names in scope here: {}. Pass it as an attribute if this is a component prop, or test it with if=\"{root}\" if it is optional", names.join(", "))
    } else {
        format!("{root} has no key {:?}; check the spelling, or test it with if=\"{path}\" if it is optional", path.rsplit('.').next().unwrap_or(""))
    };
    MageError::at(format!("undefined variable '{path}'"), file, line).fix(fix).snippet(path)
}

/// Evaluate expression text, turning failures into located errors.
pub fn eval_str(src: &str, ctx: &Ctx, lenient: bool, file: &str, line: usize) -> Result<Value> {
    let expr = parse_expr(src).map_err(|e| {
        let err = MageError::at(format!("invalid expression {src:?}: {}", e.message), file, line).snippet(src);
        match e.fix {
            Some(fix) => err.fix(fix),
            None => err.fix("expressions are dotted paths, 'strings', numbers, true, false, null, not, and, or, ==, !="),
        }
    })?;
    evaluate(&expr, ctx, lenient).map_err(|EvalError::Undefined(p)| undefined_error(&p, ctx, file, line))
}

/// Root variable names referenced by an expression (used by inspect).
pub fn path_roots(src: &str) -> Vec<String> {
    let mut roots = Vec::new();
    if let Ok(e) = parse_expr(src) {
        collect_roots(&e, &mut roots);
    }
    roots
}

fn collect_roots(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Lit(_) => {}
        Expr::Path(p) => {
            let root = p.split('.').next().unwrap_or(p).to_string();
            if !out.contains(&root) {
                out.push(root);
            }
        }
        Expr::Not(a) => collect_roots(a, out),
        Expr::And(a, b) | Expr::Or(a, b) | Expr::Eq(a, b) | Expr::Ne(a, b) => {
            collect_roots(a, out);
            collect_roots(b, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::Map;

    fn ctx() -> Ctx<'static> {
        let mut post = Map::new();
        post.insert("title".into(), Value::str("A & B"));
        post.insert("tags".into(), Value::list(vec![Value::str("x"), Value::str("y")]));
        post.insert("n".into(), Value::Int(0));
        let mut vars = Map::new();
        vars.insert("post".into(), Value::map(post));
        Ctx::root(vars)
    }

    #[test]
    fn paths_and_logic() {
        let c = ctx();
        assert_eq!(eval_str("post.tags.1", &c, false, "f", 1).unwrap().as_str(), Some("y"));
        assert!(truthy(&eval_str("not post.n and post.title == 'A & B'", &c, false, "f", 1).unwrap()));
        assert!(matches!(eval_str("post.missing", &c, true, "f", 1).unwrap(), Value::Null));
        assert!(eval_str("post.missing", &c, false, "f", 1).is_err());
    }

    #[test]
    fn foreign_syntax_gets_a_fix() {
        let c = ctx();
        let e = eval_str("post.title | upper", &c, false, "f", 1).unwrap_err();
        assert!(e.message.contains("filters"));
        assert!(e.fix.is_some());
        assert!(eval_str("len(post.tags)", &c, false, "f", 1).unwrap_err().message.contains("function"));
        assert!(eval_str("post.n + 1", &c, false, "f", 1).unwrap_err().message.contains("arithmetic"));
    }

    #[test]
    fn roots() {
        assert_eq!(path_roots("not a.b and (c == 'x' or d)"), vec!["a", "c", "d"]);
    }
}
