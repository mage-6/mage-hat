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

/// Crawlers that gather AI training data and send no readers back. Search
/// engines and the AI answer engines that cite and link their sources
/// (OAI-SearchBot, PerplexityBot, Claude-SearchBot, Applebot) stay allowed
/// by `User-agent: *`. A site that wants a different policy ships its own
/// robots.txt, which replaces this one.
const TRAINING_ONLY_CRAWLERS: &[&str] = &["CCBot", "Bytespider", "meta-externalagent"];

pub fn robots_txt(site_url: &str, news_sitemap: bool) -> String {
    let mut lines = vec!["User-agent: *".to_string(), "Allow: /".to_string(), String::new()];
    lines.extend(TRAINING_ONLY_CRAWLERS.iter().map(|ua| format!("User-agent: {ua}")));
    lines.push("Disallow: /".into());
    if !site_url.is_empty() {
        lines.push(String::new());
        lines.push(format!("Sitemap: {site_url}/sitemap.xml"));
        if news_sitemap {
            lines.push(format!("Sitemap: {site_url}/news-sitemap.xml"));
        }
    }
    lines.join("\n") + "\n"
}

/// llms.txt: the site's name and description, and where its full list of
/// pages and its feeds are. Search engines do not read it; it costs nothing
/// and some assistants look for it. A site's own llms.txt replaces this.
pub fn llms_txt(name: &str, description: &str, site_url: &str, feeds: &[(String, String)]) -> String {
    let mut out = format!("# {name}\n\n");
    if !description.is_empty() {
        out.push_str(&format!("> {description}\n\n"));
    }
    out.push_str(&format!("- [Sitemap]({site_url}/sitemap.xml): every page\n"));
    for (label, url) in feeds {
        out.push_str(&format!("- [{label}]({url}): the newest items\n"));
    }
    out
}

pub struct NewsEntry {
    pub url: String,
    pub lang: String,
    pub title: String,
    pub date: String,
}

/// Google News sitemap. The caller picks the entries (newest two days).
pub fn news_sitemap_xml(entries: &[NewsEntry], publication: &str, site_url: &str) -> String {
    let mut out = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        "<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" xmlns:news=\"http://www.google.com/schemas/sitemap-news/0.9\">".to_string(),
    ];
    for e in entries {
        out.push("  <url>".into());
        out.push(format!("    <loc>{}</loc>", esc(&format!("{site_url}{}", e.url))));
        out.push("    <news:news>".into());
        out.push(format!(
            "      <news:publication><news:name>{}</news:name><news:language>{}</news:language></news:publication>",
            esc(publication),
            esc(&news_language(&e.lang))
        ));
        out.push(format!("      <news:publication_date>{}</news:publication_date>", esc(&e.date)));
        out.push(format!("      <news:title>{}</news:title>", esc(&e.title)));
        out.push("    </news:news>".into());
        out.push("  </url>".into());
    }
    out.push("</urlset>".into());
    out.join("\n") + "\n"
}

/// Google News wants the ISO 639 language alone, except for the two Chinese
/// scripts: `pt-BR` is `pt`, `zh-TW` stays `zh-tw`.
fn news_language(lang: &str) -> String {
    let lower = lang.to_ascii_lowercase();
    if lower == "zh-cn" || lower == "zh-tw" {
        return lower;
    }
    lower.split('-').next().unwrap_or(&lower).to_string()
}

/// An ISO 8601 date or timestamp as RSS wants it (RFC 822):
/// `2026-10-09` -> `Fri, 09 Oct 2026 00:00:00 +0000`. A time and an offset
/// are kept when given; a date alone is midnight UTC. None when unreadable.
pub fn rfc822(iso: &str) -> Option<String> {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let iso = iso.trim();
    let days = crate::media::days_since_epoch(iso)?;
    let (y, m, d) = (&iso[..4], iso[5..7].parse::<usize>().ok()?, &iso[8..10]);
    let rest = iso[10..].trim_start_matches(['T', 't', ' ']);
    let (mut time, mut zone) = ("00:00:00".to_string(), "+0000".to_string());
    if !rest.is_empty() {
        let split = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
        let (clock, offset) = rest.split_at(split);
        let clock = clock.split('.').next().unwrap_or("");
        time = match clock.len() {
            5 => format!("{clock}:00"),
            8 => clock.to_string(),
            _ => return None,
        };
        if !time.bytes().enumerate().all(|(i, b)| if i == 2 || i == 5 { b == b':' } else { b.is_ascii_digit() }) {
            return None;
        }
        zone = match offset {
            "" | "Z" | "z" => "+0000".into(),
            o if o.len() == 6 && &o[3..4] == ":" => format!("{}{}", &o[..3], &o[4..]),
            o if o.len() == 5 => o.to_string(),
            _ => return None,
        };
    }
    Some(format!("{}, {d} {} {y} {time} {zone}", DAYS[(days % 7) as usize], MONTHS[m - 1]))
}

/// An item's image, as Media RSS and the enclosure describe it.
pub struct FeedImage {
    pub url: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Bytes, 0 when unknown (accepted by readers).
    pub length: u64,
}

pub struct FeedItem {
    pub title: String,
    pub url: String,
    /// RFC 822, see `rfc822`.
    pub date: Option<String>,
    pub author: Option<String>,
    pub image: Option<FeedImage>,
    pub text: String,
}

/// RSS 2.0. Items newest first as given. Each item carries its image three
/// ways, because readers disagree on which they read: `media:content`,
/// `media:thumbnail` (Feedly, Inoreader, social auto-posters) and the
/// `enclosure` (Flipboard reads only that).
pub fn rss_xml(title: &str, link: &str, description: &str, lang: &str, items: &[FeedItem]) -> String {
    let mut out = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        "<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:media=\"http://search.yahoo.com/mrss/\">".to_string(),
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
        if let Some(a) = &it.author {
            out.push(format!("      <dc:creator>{}</dc:creator>", esc(a)));
        }
        out.push(format!("      <description>{}</description>", esc(&it.text)));
        if let Some(img) = &it.image {
            let size = match (img.width, img.height) {
                (Some(w), Some(h)) => format!(" width=\"{w}\" height=\"{h}\""),
                _ => String::new(),
            };
            out.push(format!("      <media:content url=\"{}\" medium=\"image\" type=\"{}\"{size}/>", esc(&img.url), esc(&img.mime)));
            out.push(format!("      <media:thumbnail url=\"{}\"{size}/>", esc(&img.url)));
            out.push(format!("      <enclosure url=\"{}\" length=\"{}\" type=\"{}\"/>", esc(&img.url), img.length, esc(&img.mime)));
        }
        out.push("    </item>".into());
    }
    out.push("  </channel>".into());
    out.push("</rss>".into());
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc822_dates() {
        assert_eq!(rfc822("2026-10-09").as_deref(), Some("Fri, 09 Oct 2026 00:00:00 +0000"));
        assert_eq!(rfc822("2026-03-01T14:30").as_deref(), Some("Sun, 01 Mar 2026 14:30:00 +0000"));
        assert_eq!(rfc822("2026-03-01T14:30:05.120Z").as_deref(), Some("Sun, 01 Mar 2026 14:30:05 +0000"));
        assert_eq!(rfc822("2024-02-29 08:00:00-03:00").as_deref(), Some("Thu, 29 Feb 2024 08:00:00 -0300"));
        assert_eq!(rfc822("1970-01-01T00:00:00+0530").as_deref(), Some("Thu, 01 Jan 1970 00:00:00 +0530"));
        assert_eq!(rfc822("October 9"), None);
        assert_eq!(rfc822("2026-10-09T9am"), None);
    }

    #[test]
    fn news_languages() {
        assert_eq!(news_language("pt-BR"), "pt");
        assert_eq!(news_language("en"), "en");
        assert_eq!(news_language("zh-TW"), "zh-tw");
    }
}
