//! Links to an item that is still scheduled.
//!
//! Items written ahead link to each other: a guide published today points at
//! a sibling dated next month. Until that date the sibling has no page, so the
//! link would be a 404 on the live site. The build writes such a link as its
//! text instead, and the first build on or after the sibling's date writes it
//! as a link again, with nothing to edit on either side. Only links to an item
//! the clock put off are touched; any other broken link is still `check`'s to
//! report.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

// Anchors do not nest, so the first closing tag ends the element.
static ANCHOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<a\b([^>]*)>(.*?)</a\s*>"#).unwrap());
static HREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)\shref\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap());

#[derive(Debug, Default)]
pub struct ScheduledLinks {
    /// URL paths of the scheduled items, each with a trailing slash.
    paths: HashSet<String>,
    /// The site's `url`, so an absolute link to the site matches too.
    site_url: String,
}

impl ScheduledLinks {
    pub fn new(paths: impl IntoIterator<Item = String>, site_url: &str) -> Self {
        ScheduledLinks { paths: paths.into_iter().map(|p| with_slash(&p)).collect(), site_url: site_url.trim_end_matches('/').to_string() }
    }

    /// Replaces every `<a>` that points at a scheduled item with its content.
    /// Returns the HTML and how many links it replaced.
    pub fn unlink(&self, html: &str) -> (String, usize) {
        if self.paths.is_empty() || !html.contains("<a") && !html.contains("<A") {
            return (html.to_string(), 0);
        }
        let mut n = 0;
        let out = ANCHOR.replace_all(html, |c: &regex::Captures| {
            let href = HREF.captures(&c[1]).and_then(|h| h.get(1).or(h.get(2))).map(|m| m.as_str());
            if href.is_some_and(|h| self.targets(h)) {
                n += 1;
                c[2].to_string()
            } else {
                c[0].to_string()
            }
        });
        (out.into_owned(), n)
    }

    fn targets(&self, href: &str) -> bool {
        let href = href.trim();
        let href = if self.site_url.is_empty() { href } else { href.strip_prefix(&self.site_url).unwrap_or(href) };
        let path = &href[..href.find(['?', '#']).unwrap_or(href.len())];
        path.starts_with('/') && self.paths.contains(&with_slash(path))
    }
}

fn with_slash(path: &str) -> String {
    if path.ends_with('/') { path.to_string() } else { format!("{path}/") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links() -> ScheduledLinks {
        ScheduledLinks::new(["/posts/next-week/".to_string()], "https://news.example/")
    }

    #[test]
    fn a_link_to_a_scheduled_item_becomes_its_text() {
        let (out, n) = links().unlink(r#"<p>See <a href="/posts/next-week/">next <em>week</em></a> and <a href="/posts/live/">this</a>.</p>"#);
        assert_eq!(out, r#"<p>See next <em>week</em> and <a href="/posts/live/">this</a>.</p>"#);
        assert_eq!(n, 1);
    }

    #[test]
    fn every_way_of_writing_the_address_matches() {
        for href in ["/posts/next-week", "/posts/next-week/#faq", "/posts/next-week/?a=1", "https://news.example/posts/next-week/", "'/posts/next-week/'"] {
            let attr = if href.starts_with('\'') { format!("href={href}") } else { format!("href=\"{href}\"") };
            let (out, n) = links().unlink(&format!(r#"<a class="x" {attr}>t</a>"#));
            assert_eq!((out.as_str(), n), ("t", 1), "{href}");
        }
    }

    #[test]
    fn other_links_are_left_alone() {
        for html in [r#"<a href="/posts/next-week-two/">t</a>"#, r#"<a href="https://other.example/posts/next-week/">t</a>"#, r##"<a href="#posts/next-week/">t</a>"##, r#"<a name="x">t</a>"#] {
            assert_eq!(links().unlink(html), (html.to_string(), 0));
        }
    }
}
