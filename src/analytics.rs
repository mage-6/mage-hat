//! `[analytics]` in site.toml: the web beacon from the analytics skill,
//! shipped inside the site so a page sends its events with no per-site code.
//!
//!     [analytics]
//!     endpoint = "https://events.example.com/v2/my-site"
//!
//! With the table set, every page loads one generated module,
//! `/_mh/analytics.<hash>.js`: beacon.js verbatim (src/beacon.js, a copy of
//! the skill's clients/web/beacon.js, never fetched from anywhere at run
//! time) followed by the glue that creates the beacon and fires the two
//! events a website has: `page_view` on every load and `conversion` with a
//! kind token when an element carrying `data-track="<kind>"` is clicked.
//! Without the table nothing is written: not a byte.
//!
//! The version in the batch header is `version` from site.toml when the
//! site has one, else empty. Pages served from localhost send nothing, so
//! `magehat dev` never writes to the production dataset.

use crate::build::BuildResult;
use crate::components::digest;
use crate::config::Config;
use regex::Regex;
use std::sync::LazyLock;

/// The beacon, byte for byte as the analytics skill ships it.
pub const BEACON: &str = include_str!("beacon.js");

/// The attribute content authors put on a button or link to count a conversion.
pub const TRACK_ATTR: &str = "data-track";

/// A text param of the contract: lowercase token, at most 64 characters.
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9/][a-z0-9_./-]{0,63}$").unwrap());
static TRACK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\sdata-track\s*=\s*["']([^"']*)["']"#).unwrap());

#[derive(Debug, Clone, PartialEq)]
pub struct Analytics {
    /// Where batches are posted: the site's own Worker or the collector.
    pub endpoint: String,
}

/// The module every page loads: the beacon, then the site's glue.
pub fn script(cfg: &Config, a: &Analytics) -> String {
    let version = cfg.extra.get("version").and_then(|v| v.as_str()).unwrap_or("");
    // JSON strings are valid JavaScript string literals, quotes included.
    let endpoint = serde_json::to_string(&a.endpoint).unwrap();
    let version = serde_json::to_string(version).unwrap();
    format!(
        "{}\n\
         // Written by MageHat from [analytics] in site.toml. A page served from\n\
         // localhost sends nothing: production only.\n\
         if (![\"localhost\", \"127.0.0.1\", \"[::1]\"].includes(location.hostname)) {{\n\
         \x20 const analytics = createBeacon({endpoint}, {version});\n\
         \x20 analytics(\"page_view\");\n\
         \x20 addEventListener(\"click\", (e) => {{\n\
         \x20   const el = e.target instanceof Element ? e.target.closest(\"[{TRACK_ATTR}]\") : null;\n\
         \x20   if (el) analytics(\"conversion\", [el.getAttribute(\"{TRACK_ATTR}\")]);\n\
         \x20 }});\n\
         }}\n",
        BEACON.trim_end()
    )
}

/// `_mh/analytics.<hash>.js`: hashed like a component script, so a changed
/// endpoint is a new file and no cache serves the old one.
pub fn file_name(text: &str) -> String {
    format!("analytics.{}.js", digest(text))
}

/// `magehat check`: a `data-track` value must be a contract token, and the
/// attribute does nothing until `[analytics]` is set.
pub fn check(r: &mut BuildResult) {
    let enabled = r.cfg.analytics.is_some();
    let mut warnings = Vec::new();
    for p in &r.pages {
        let html = String::from_utf8_lossy(&r.outputs[&p.out]);
        let mut seen = std::collections::HashSet::new();
        for m in TRACK.captures_iter(&html) {
            let kind = m[1].trim().to_string();
            if !seen.insert(kind.clone()) {
                continue;
            }
            if !TOKEN.is_match(&kind) {
                warnings.push((
                    format!("{TRACK_ATTR}={kind:?} on {} is not an event token", p.url),
                    p.file.clone(),
                    "lowercase letters, digits, _ . / and -, up to 64 characters: data-track=\"signup\", data-track=\"download\"",
                ));
            } else if !enabled {
                warnings.push((
                    format!("{TRACK_ATTR}={kind:?} on {} counts nothing without [analytics] in site.toml", p.url),
                    p.file.clone(),
                    "add [analytics] with endpoint = \"https://events.<publisher domain>/v2/<site slug>\", or remove the attribute",
                ));
            }
        }
    }
    for (m, f, fix) in warnings {
        r.warn_at(m, &f, None, fix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_follow_the_contract() {
        for ok in ["signup", "download", "a", "0", "store/play", "buy-pro_1.0"] {
            assert!(TOKEN.is_match(ok), "{ok}");
        }
        for bad in ["", "Sign Up", "_lead", "sign up", &"x".repeat(65)] {
            assert!(!TOKEN.is_match(bad), "{bad:?}");
        }
    }
}
