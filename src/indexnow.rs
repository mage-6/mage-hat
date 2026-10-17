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
//! `magehat build` writes the key file when site.toml has `indexnow = true`.
//! `magehat indexnow -- <deploy command>` reads the live sitemap, runs the
//! deploy, and submits only what it changed: URLs that are new, gone, or
//! carry a different lastmod (lastmod.rs). Bing asks for changed URLs only and
//! learns to discount a host that resends its whole sitemap on every deploy,
//! which every site did until 0.20.0. A bare `magehat indexnow` still submits
//! every URL, for a first submission. Right after a deploy the edge can still
//! serve the previous sitemap, so when dist/ holds the build just deployed,
//! `indexnow` waits until the live sitemap matches it.

use crate::config::Config;
use crate::errors::{MageError, Result};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use std::time::Duration;

const ENDPOINT: &str = "https://api.indexnow.org/indexnow";

static LOC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<loc>\s*([^<\s]+)\s*</loc>").unwrap());
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<url>(.*?)</url>").unwrap());
static LASTMOD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<lastmod>\s*([^<\s]+)\s*</lastmod>").unwrap());

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

/// Each page of a sitemap with its lastmod, in order.
fn pages(xml: &str) -> Vec<(String, Option<String>)> {
    URL.captures_iter(xml)
        .filter_map(|u| {
            let loc = LOC.captures(&u[1])?[1].replace("&amp;", "&");
            Some((loc, LASTMOD.captures(&u[1]).map(|c| c[1].to_string())))
        })
        .collect()
}

/// What a deploy changed, from the sitemap before it to the one after: the
/// pages that are new or carry another lastmod, then the ones that are gone
/// (IndexNow takes removed URLs too, so the engines drop them sooner).
pub fn changed_urls(before: &str, after: &str) -> Vec<String> {
    let old: std::collections::HashMap<String, Option<String>> = pages(before).into_iter().collect();
    let new = pages(after);
    let mut out: Vec<String> = new.iter().filter(|(loc, lastmod)| old.get(loc) != Some(lastmod)).map(|(loc, _)| loc.clone()).collect();
    let kept: std::collections::HashSet<&String> = new.iter().map(|(loc, _)| loc).collect();
    out.extend(pages(before).into_iter().map(|(loc, _)| loc).filter(|loc| !kept.contains(loc)));
    out
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

/// How often and how long `indexnow` asks for the live sitemap while it
/// waits for the one just deployed: every 5 s for 3 minutes.
const POLL: Duration = Duration::from_secs(5);
const ATTEMPTS: u32 = 36;

/// The live sitemap, once it matches `built` (dist/sitemap.xml) when there
/// is one. Without the wait, a ping sent seconds after a deploy described the
/// site before it: on 2026-10-13 four sites of twelve submitted their old
/// sitemap and left out every page that deploy added.
pub fn live_sitemap(mut fetch: impl FnMut() -> Result<String>, built: Option<&str>, attempts: u32, pause: impl Fn()) -> Result<String> {
    let mut xml = fetch()?;
    for _ in 1..attempts {
        if built.map_or(true, |b| b.trim() == xml.trim()) {
            return Ok(xml);
        }
        pause();
        xml = fetch()?;
    }
    match built {
        Some(b) if b.trim() != xml.trim() => Err(MageError::new("the live sitemap still differs from dist/sitemap.xml, so the deploy has not reached the site")
            .fix("deploy dist/ and run this again; to submit whatever is live instead, delete dist/ first")),
        _ => Ok(xml),
    }
}

fn ready(cfg: &Config) -> Result<()> {
    if !cfg.indexnow {
        return Err(MageError::in_file("IndexNow is off for this site", "site.toml")
            .fix("add indexnow = true to site.toml, build and deploy so the key file is live, then run this again"));
    }
    if cfg.url.is_empty() {
        return Err(MageError::in_file("IndexNow needs the site's address", "site.toml").fix("set url = \"https://your-domain\" in site.toml"));
    }
    Ok(())
}

/// The live sitemap, once. A new query each time, so an edge cache cannot
/// answer with the old file.
fn fetch_live(cfg: &Config, n: u64) -> Result<String> {
    let sitemap_url = format!("{}/sitemap.xml", cfg.url);
    ureq::get(&format!("{sitemap_url}?magehat={}-{n}", std::process::id()))
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| MageError::new(format!("could not read {sitemap_url}: {e}")).fix("deploy the site first; IndexNow submits what is live"))
}

/// The live sitemap once it is the one in dist/ (the build just deployed).
///
/// The live sitemap, not dist/: the ping has to describe what is deployed,
/// and a local build may be ahead of it or never have been published.
fn deployed_sitemap(cfg: &Config) -> Result<String> {
    let built = std::fs::read_to_string(cfg.dist().join("sitemap.xml")).ok();
    let mut n = 0u64;
    let fetch = || {
        n += 1;
        fetch_live(cfg, n)
    };
    live_sitemap(fetch, built.as_deref(), ATTEMPTS, || std::thread::sleep(POLL))
}

fn post(cfg: &Config, urls: &[String]) -> Result<()> {
    let body = payload(&cfg.url, urls);
    match ureq::post(ENDPOINT).header("Content-Type", "application/json; charset=utf-8").send(body.to_string()) {
        // 202 means accepted while the key is still being checked, normal on
        // the first submission for a host.
        Ok(r) if r.status() == 200 || r.status() == 202 => Ok(()),
        Ok(r) => Err(MageError::new(format!("IndexNow answered {}", r.status()))),
        Err(ureq::Error::StatusCode(403)) => Err(MageError::new("IndexNow rejected the key (403)")
            .fix(format!("check that {}/{} is live: build and deploy with indexnow = true first", cfg.url, key_file(cfg).map(|k| k.0).unwrap_or_default()))),
        Err(ureq::Error::StatusCode(422)) => Err(MageError::new("IndexNow rejected the URLs (422): they must all be on the site's own host")
            .fix("the sitemap's addresses come from url in site.toml; make it match the host being deployed")),
        Err(e) => Err(MageError::new(format!("IndexNow submission failed: {e}"))),
    }
}

/// Submit every URL in the live sitemap. Returns how many went.
pub fn submit(cfg: &Config) -> Result<usize> {
    ready(cfg)?;
    let urls = sitemap_urls(&deployed_sitemap(cfg)?);
    if urls.is_empty() {
        return Err(MageError::new(format!("{}/sitemap.xml lists no URLs", cfg.url)));
    }
    post(cfg, &urls)?;
    Ok(urls.len())
}

/// What a wrapped deploy did: the command's exit code, and what IndexNow was
/// told, or why it was not (the deploy stands either way).
pub struct Deployed {
    pub code: i32,
    pub submitted: Result<Option<usize>>,
}

/// Run the deploy command between two readings of the live sitemap, then
/// submit the URLs it changed. The first reading failing (a first deploy, a
/// site that was down) submits every URL, which is what a first deploy needs.
/// Nothing changed submits nothing: `Ok(None)`. A site with IndexNow off
/// still deploys, and is told why nothing was submitted.
pub fn deploy_and_submit(cfg: &Config, command: &[String]) -> Result<Deployed> {
    let ready = ready(cfg);
    let before = if ready.is_ok() { fetch_live(cfg, 0).ok() } else { None };
    let status = std::process::Command::new(&command[0])
        .args(&command[1..])
        .status()
        .map_err(|e| MageError::new(format!("could not run {}: {e}", command[0])).fix("on Windows, start a .cmd or .bat by its full name, such as pnpm.cmd"))?;
    let code = status.code().unwrap_or(1);
    if code != 0 {
        return Ok(Deployed { code, submitted: Ok(None) });
    }
    let submitted = ready.and_then(|_| deployed_sitemap(cfg)).and_then(|after| {
        let urls = match &before {
            Some(b) => changed_urls(b, &after),
            None => sitemap_urls(&after),
        };
        if urls.is_empty() {
            return Ok(None);
        }
        post(cfg, &urls).map(|_| Some(urls.len()))
    });
    Ok(Deployed { code, submitted })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetches(seq: &[&str]) -> impl Fn() -> Result<String> {
        let seq: Vec<String> = seq.iter().map(|s| s.to_string()).collect();
        let i = std::cell::Cell::new(0);
        move || {
            let n = i.get().min(seq.len() - 1);
            i.set(i.get() + 1);
            Ok(seq[n].clone())
        }
    }

    #[test]
    fn the_wait_ends_when_the_live_sitemap_matches_the_build() {
        assert_eq!(live_sitemap(fetches(&["old", "old", "new
"]), Some("new"), 10, || {}).unwrap(), "new
");
        assert_eq!(live_sitemap(fetches(&["old"]), None, 10, || {}).unwrap(), "old", "no build: what is live goes at once");
        let e = live_sitemap(fetches(&["old"]), Some("new"), 3, || {}).unwrap_err();
        assert!(e.to_string().contains("still differs from dist/sitemap.xml"), "{e}");
    }

    #[test]
    fn a_deploy_submits_what_it_added_changed_or_removed() {
        let page = |loc: &str, lastmod: Option<&str>| match lastmod {
            Some(d) => format!("<url>\n<loc>{loc}</loc>\n<lastmod>{d}</lastmod>\n</url>"),
            None => format!("<url><loc>{loc}</loc></url>"),
        };
        let before = [page("https://a.com/", Some("2026-10-01")), page("https://a.com/same/", Some("2026-09-01")), page("https://a.com/gone/", None), page("https://a.com/dated/", None)].concat();
        let after = [page("https://a.com/", Some("2026-10-17")), page("https://a.com/same/", Some("2026-09-01")), page("https://a.com/new/", Some("2026-10-17")), page("https://a.com/dated/", Some("2026-10-17"))].concat();
        assert_eq!(changed_urls(&before, &after), vec!["https://a.com/", "https://a.com/new/", "https://a.com/dated/", "https://a.com/gone/"]);
        assert!(changed_urls(&after, &after).is_empty(), "an unchanged deploy submits nothing");
    }

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
