//! Markdown to HTML. CommonMark plus tables and strikethrough, frozen.
//!
//! HTML inside Markdown passes through, which is how component tags like
//! <x-figure> are used from an article.

use pulldown_cmark::{html, Options, Parser};
use regex::Regex;
use std::sync::LazyLock;

// CommonMark only knows the standard block-level tag names, so a component on
// a line of its own comes out wrapped in <p>. Unwrap a paragraph that holds
// nothing but one component, so <x-figure> behaves like the block it is.
static LONE_COMPONENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<p>(\s*<(x-[a-z0-9-]+)\b[^>]*>.*?</x-[a-z0-9-]+>\s*)</p>").unwrap());

pub fn render_markdown(text: &str) -> String {
    let parser = Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH);
    let mut out = String::with_capacity(text.len() * 2);
    html::push_html(&mut out, parser);
    LONE_COMPONENT.replace_all(&out, "$1").into_owned()
}
