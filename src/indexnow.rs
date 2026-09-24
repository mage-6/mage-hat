//! IndexNow: telling Bing, Yandex and the other participating engines that a
//! site changed, instead of waiting for their crawlers to come by. Google does
//! not take part; it reads the sitemap.
//!
//! The protocol proves ownership with a key file served at the site root. The
//! key is derived from the site URL rather than generated and stored: it is
//! public by design (anyone can read the file), so a secret would add nothing,
//! and derivation means there is no value to keep, copy between machines or
//! lose. Changing the URL changes the key, which is right: the key belongs to
//! the host.
//!
//! `magehat build` writes the key file when site.toml has `indexnow = true`;
//! `magehat indexnow` submits every URL in the live sitemap after a deploy.
//! Resubmitting unchanged URLs is harmless, and one list is simpler than a
//! diff that could silently miss a page.

use crate::config::Config;
use crate::errors::{MageError, Result};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

const ENDPOINT: &str = "https://api.indexnow.org/indexnow";

static LOC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<loc>\s*([^<\s]+)\s*</loc>").unwrap());

/// 32 hex characters, well inside the protocol's 8 to 128 [a-zA-Z0-9-].
pub fn key_for(site_url: &str) -> String {
    let digest = Sha256::digest(format!("magehat-indexnow:{}", site_url.trim_end_matches('/')));
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// The key file's path in dist/, or None when the site has not opted in.
pub fn key_file(cfg: &Config) -> Option<(String, String)> {
    if !cfg.indexnow || cfg.url.is_empty() {
        return None;
    }
    let key = key_for(&cfg.url);
    Some((format!("{key}.txt"), key))
}

/// Every <loc> in a sitemap, in order.
pub fn sitemap_urls(xml: &str) -> Vec<String> {
    LOC.captures_iter(xml).map(|c| c[1].replace("&amp;", "&")).collect()
}

/// The JSON body IndexNow expects.
pub fn payload(site_url: &str, urls: &[String]) -> serde_json::Value {
    let key = key_for(site_url);
    let host = site_url.split("://").nth(1).unwrap_or(site_url).split('/').next().unwrap_or_default();
    serde_json::json!({
        "host": host,
        "key": key,
        "keyLocation": format!("{}/{key}.txt", site_url.trim_end_matches('/')),
        "urlList": urls,
    })
}

/// Read the live sitemap and submit it. Returns how many URLs went.
///
/// The live sitemap, not dist/: the ping has to describe what is deployed,
/// and a local build may be ahead of it or never have been published.
pub fn submit(cfg: &Config) -> Result<usize> {
    if !cfg.indexnow {
        return Err(MageError::in_file("IndexNow is off for this site", "site.toml")
            .fix("add indexnow = true to site.toml, build and deploy so the key file is live, then run this again"));
    }
    if cfg.url.is_empty() {
        return Err(MageError::in_file("IndexNow needs the site's address", "site.toml").fix("set url = \"https://your-domain\" in site.toml"));
    }
    let sitemap_url = format!("{}/sitemap.xml", cfg.url);
    let xml = ureq::get(&sitemap_url)
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| MageError::new(format!("could not read {sitemap_url}: {e}")).fix("deploy the site first; IndexNow submits what is live"))?;
    let urls = sitemap_urls(&xml);
    if urls.is_empty() {
        return Err(MageError::new(format!("{sitemap_url} lists no URLs")));
    }
    let body = payload(&cfg.url, &urls);
    match ureq::post(ENDPOINT).header("Content-Type", "application/json; charset=utf-8").send(body.to_string()) {
        // 202 means accepted while the key is still being checked, normal on
        // the first submission for a host.
        Ok(r) if r.status() == 200 || r.status() == 202 => Ok(urls.len()),
        Ok(r) => Err(MageError::new(format!("IndexNow answered {}", r.status()))),
        Err(ureq::Error::StatusCode(403)) => Err(MageError::new("IndexNow rejected the key (403)")
            .fix(format!("check that {}/{} is live: build and deploy with indexnow = true first", cfg.url, key_file(cfg).map(|k| k.0).unwrap_or_default()))),
        Err(ureq::Error::StatusCode(422)) => Err(MageError::new("IndexNow rejected the URLs (422): they must all be on the site's own host")
            .fix("the sitemap's addresses come from url in site.toml; make it match the host being deployed")),
        Err(e) => Err(MageError::new(format!("IndexNow submission failed: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_stable_valid_and_ignores_a_trailing_slash() {
        let k = key_for("https://example.com");
        assert_eq!(k, key_for("https://example.com/"));
        assert_eq!(k.len(), 32);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(k, key_for("https://example.org"));
    }

    #[test]
    fn sitemap_urls_are_read_in_order_and_unescaped() {
        let xml = "<urlset><url><loc>https://a.com/</loc></url><url>\n<loc> https://a.com/x/?a=1&amp;b=2 </loc></url></urlset>";
        assert_eq!(sitemap_urls(xml), vec!["https://a.com/", "https://a.com/x/?a=1&b=2"]);
    }

    #[test]
    fn the_payload_names_the_host_and_the_key_file() {
        let p = payload("https://a.com", &["https://a.com/".into()]);
        let key = key_for("https://a.com");
        assert_eq!(p["host"], "a.com");
        assert_eq!(p["key"], key.as_str());
        assert_eq!(p["keyLocation"], format!("https://a.com/{key}.txt"));
        assert_eq!(p["urlList"][0], "https://a.com/");
    }
}
