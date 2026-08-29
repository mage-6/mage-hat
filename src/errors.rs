//! One error type for everything the compiler can complain about.
//!
//! Every error carries a location and, wherever possible, the fix. The
//! reader is usually an agent in a build-fix loop, so an error that does not
//! say what to do next is only half an error.

use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MageError {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub fix: Option<String>,
    /// The exact source text at fault, so the excerpt can point at it.
    pub snippet: Option<String>,
}

impl MageError {
    pub fn new(message: impl Into<String>) -> Self {
        MageError { message: message.into(), file: None, line: None, fix: None, snippet: None }
    }

    pub fn at(message: impl Into<String>, file: &str, line: usize) -> Self {
        MageError { message: message.into(), file: Some(file.to_string()), line: Some(line), fix: None, snippet: None }
    }

    pub fn in_file(message: impl Into<String>, file: &str) -> Self {
        MageError { message: message.into(), file: Some(file.to_string()), line: None, fix: None, snippet: None }
    }

    pub fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    pub fn snippet(mut self, snippet: impl Into<String>) -> Self {
        self.snippet = Some(snippet.into());
        self
    }

    /// Location prefix as "file:line: " or "file: " or "".
    pub fn where_(&self) -> String {
        match (&self.file, self.line) {
            (Some(f), Some(l)) => format!("{f}:{l}: "),
            (Some(f), None) => format!("{f}: "),
            _ => String::new(),
        }
    }

    /// The source line with a caret under the snippet, when the file can be read.
    pub fn excerpt(&self, root: &Path) -> Option<String> {
        let file = self.file.as_ref()?;
        let line = self.line?;
        let text = std::fs::read_to_string(root.join(file)).ok()?;
        let src = text.lines().nth(line.checked_sub(1)?)?;
        let trimmed = src.trim_start();
        if trimmed.is_empty() {
            return None;
        }
        let indent = src.len() - trimmed.len();
        let (col, len) = match self.snippet.as_deref().filter(|s| !s.is_empty()).and_then(|s| src.find(s).map(|i| (i, s))) {
            Some((i, s)) => (src[indent..i.max(indent)].chars().count(), s.chars().count()),
            None => (0, trimmed.chars().count()),
        };
        Some(format!("    {trimmed}\n    {}{}", " ".repeat(col), "^".repeat(len.max(1))))
    }
}

impl fmt::Display for MageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.where_(), self.message)?;
        if let Some(fix) = &self.fix {
            write!(f, "\n  fix: {fix}")?;
        }
        Ok(())
    }
}

impl std::error::Error for MageError {}

impl From<std::io::Error> for MageError {
    fn from(e: std::io::Error) -> Self {
        MageError::new(format!("I/O error: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, MageError>;
