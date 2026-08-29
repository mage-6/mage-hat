//! Build-time SEO output: head links, sitemap, robots.txt, feeds.
//!
//! Nothing here reads the clock. Dates come from content metadata, so the
//! same source always produces the same bytes.

use crate::values::escape_attr_str as esc;
use regex::Regex;
use std::sync::LazyLock;

static HEAD_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</head\s*>").unwrap());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    pub lang: String,
    pub url: String,
}

/// Insert lines before </head>, or at the top when the page has no head.
pub fn inject_head(html: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return html.to_string();
    }
    let block: String = lines.iter().map(|l| format!("  {l}\n")).collect();
    match HEAD_END.find(html) {
        Some(m) => format!("{}{}{}", &html[..m.start()], block, &html[m.start()..]),
        None => format!("{block}{html}"),
    }
}

/// hreflang links for every translation of a page, plus x-default.
pub fn alternate_links(translations: &[Translation], site_url: &str, default_language: &str) -> Vec<String> {
    if translations.len() < 2 || site_url.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = translations
        .iter()
        .map(|t| format!("<link rel=\"alternate\" hreflang=\"{}\" href=\"{}\">", t.lang, esc(&format!("{site_url}{}", t.url))))
        .collect();
    let default = translations.iter().find(|t| t.lang == default_language).unwrap_or(&translations[0]);
    lines.push(format!("<link rel=\"alternate\" hreflang=\"x-default\" href=\"{}\">", esc(&format!("{site_url}{}", default.url))));
    lines
}

pub struct SitemapEntry {
    pub url: String,
    pub lastmod: Option<String>,
    pub translations: Vec<Translation>,
}

pub fn sitemap_xml(entries: &[SitemapEntry], site_url: &str) -> String {
    let mut sorted: Vec<&SitemapEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.url.cmp(&b.url));
    let mut out = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        "<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">".to_string(),
    ];
    for e in sorted {
        out.push("  <url>".into());
        out.push(format!("    <loc>{}</loc>", esc(&format!("{site_url}{}", e.url))));
        if let Some(d) = &e.lastmod {
            out.push(format!("    <lastmod>{}</lastmod>", esc(d)));
        }
        if e.translations.len() > 1 {
            for t in &e.translations {
                out.push(format!("    <xhtml:link rel=\"alternate\" hreflang=\"{}\" href=\"{}\"/>", t.lang, esc(&format!("{site_url}{}", t.url))));
            }
        }
        out.push("  </url>".into());
    }
    out.push("</urlset>".into());
    out.join("\n") + "\n"
}

pub fn robots_txt(site_url: &str) -> String {
    let mut lines = vec!["User-agent: *".to_string(), "Allow: /".to_string()];
    if !site_url.is_empty() {
        lines.push(format!("Sitemap: {site_url}/sitemap.xml"));
    }
    lines.join("\n") + "\n"
}

pub struct FeedItem {
    pub title: String,
    pub url: String,
    pub date: Option<String>,
    pub text: String,
}

/// RSS 2.0. Items newest first as given.
pub fn rss_xml(title: &str, link: &str, description: &str, lang: &str, items: &[FeedItem]) -> String {
    let mut out = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        "<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">".to_string(),
        "  <channel>".to_string(),
        format!("    <title>{}</title>", esc(title)),
        format!("    <link>{}</link>", esc(link)),
        format!("    <description>{}</description>", esc(description)),
        format!("    <language>{}</language>", esc(lang)),
        format!("    <atom:link href=\"{}feed.xml\" rel=\"self\" type=\"application/rss+xml\"/>", esc(link)),
    ];
    for it in items {
        out.push("    <item>".into());
        out.push(format!("      <title>{}</title>", esc(&it.title)));
        out.push(format!("      <link>{}</link>", esc(&it.url)));
        out.push(format!("      <guid>{}</guid>", esc(&it.url)));
        if let Some(d) = &it.date {
            out.push(format!("      <pubDate>{}</pubDate>", esc(d)));
        }
        out.push(format!("      <description>{}</description>", esc(&it.text)));
        out.push("    </item>".into());
    }
    out.push("  </channel>".into());
    out.push("</rss>".into());
    out.join("\n") + "\n"
}
