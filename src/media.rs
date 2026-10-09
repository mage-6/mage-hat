//! Media: files that grow with the content and would never stop growing the
//! repository, kept in an R2 bucket instead.
//!
//!     [media]                                  # site.toml
//!     url = "https://media.example.com"        # the bucket's public address
//!     bucket = "example-bucket-media"
//!     account = "<Cloudflare account id>"
//!
//!     magehat media add photos/cover.jpg car-park-capital
//!     <img src="media:car-park-capital" alt="A car park" width="800">
//!
//! `add` encodes an image exactly as the build treats a `src/assets` image
//! (WebP plus the original format, at a fixed ladder of widths), uploads
//! every variant under a content-hashed name and writes `src/media/<name>.json`
//! with what the build needs: type, hash, dimensions, widths. The original
//! never enters the repository, and the build reads only the record, so it
//! stays offline and deterministic. The same shape as fonts and icons:
//! network once, commit the result, no build touches the network again.
//!
//! `prune` is the other half: run after a deploy, it deletes every object no
//! record names. Objects younger than GRACE_DAYS stay, because a story on an
//! open branch has uploaded its cover before its record is merged.

use crate::components::digest_bytes;
use crate::config::load_config;
use crate::errors::{MageError, Result};
use crate::images::{encode_variant, Fmt};
use indexmap::IndexMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;

pub const DIR: &str = "src/media";
pub const PREFIX: &str = "media:";
/// Widths offered for an image: every step below the source width, then the
/// source itself. Fixed, so a record written today still matches a build
/// made in two years.
pub const LADDER: &[u32] = &[400, 800, 1200, 1600, 2000, 2400];
/// `prune` keeps an object this young even when no record names it.
pub const GRACE_DAYS: u64 = 14;
const API: &str = "https://api.cloudflare.com/client/v4";
const TOKEN_VAR: &str = "CLOUDFLARE_API_TOKEN";

/// The pattern for `media:<name>` wherever an attribute or a JSON string would
/// hold a path, also when written after the site's own address: the layout's
/// og:image is `{{ site.url }}{{ page.image }}`, and it must keep working
/// when the image moves to the bucket.
pub fn ref_pattern(site_url: &str) -> Regex {
    let site = if site_url.is_empty() { String::new() } else { format!("(?:{}/?)?", regex::escape(site_url.trim_end_matches('/'))) };
    Regex::new(&format!(r#"(["'(]|&quot;){site}media:([a-z0-9-]+)"#)).unwrap()
}

/// The `[media]` table of site.toml.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub url: String,
    pub bucket: String,
    pub account: String,
}

impl Media {
    pub fn url_for(&self, key: &str) -> String {
        format!("{}/{key}", self.url)
    }
}

/// One `src/media/<name>.json`. Everything the build needs to write the
/// markup for a file it has never seen.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Record {
    #[serde(rename = "type")]
    pub mime: String,
    pub ext: String,
    pub hash: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widths: Vec<u32>,
}

impl Record {
    pub fn is_image(&self) -> bool {
        self.width.is_some() && self.height.is_some() && !self.widths.is_empty()
    }

    /// Object name of one resized variant: `<name>.<hash>.<width>.<ext>`.
    pub fn variant_key(&self, name: &str, width: u32, ext: &str) -> String {
        format!("{name}.{}.{width}.{ext}", self.hash)
    }

    /// Object name of a file uploaded as it is: `<name>.<hash>.<ext>`.
    pub fn file_key(&self, name: &str) -> String {
        format!("{name}.{}.{}", self.hash, self.ext)
    }

    /// The one object to point at when only a single address fits: the
    /// original for a file, the full-size original format for an image.
    pub fn main_key(&self, name: &str) -> String {
        match self.width {
            Some(w) if self.is_image() => self.variant_key(name, w, &self.ext),
            _ => self.file_key(name),
        }
    }

    /// Every object this record expects in the bucket.
    pub fn keys(&self, name: &str) -> Vec<String> {
        if !self.is_image() {
            return vec![self.file_key(name)];
        }
        let mut out = Vec::new();
        for &w in &self.widths {
            out.push(self.variant_key(name, w, "webp"));
            if self.ext != "webp" {
                out.push(self.variant_key(name, w, &self.ext));
            }
        }
        out
    }
}

pub type Library = IndexMap<String, Record>;

/// Read every record under src/media. A record that does not parse fails
/// the build: a page would otherwise point at nothing.
pub fn load(root: &Path) -> Result<Library> {
    let dir = root.join(DIR);
    let mut lib = Library::new();
    if !dir.is_dir() {
        return Ok(lib);
    }
    let mut entries: Vec<_> = std::fs::read_dir(&dir)?.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    entries.sort();
    for path in entries {
        let file = format!("{DIR}/{}", path.file_name().unwrap_or_default().to_string_lossy());
        let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".json")) else {
            return Err(MageError::in_file("only <name>.json records belong in src/media", &file).fix("move the file elsewhere; media files themselves live in the bucket"));
        };
        if !valid_name(name) {
            return Err(MageError::in_file(format!("media name {name:?} must be lowercase letters, digits and dashes"), &file)
                .fix(format!("rename it to {}.json and upload the file again under that name", slug(name))));
        }
        let text = std::fs::read_to_string(&path)?;
        let rec: Record = serde_json::from_str(&text)
            .map_err(|e| MageError::in_file(format!("invalid media record: {e}"), &file).fix("records are written by `magehat media add <file> <name>`; run it again rather than editing"))?;
        lib.insert(name.to_string(), rec);
    }
    Ok(lib)
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !name.starts_with('-') && !name.ends_with('-')
}

/// A file stem as a media name: lowercase, anything else becomes a dash.
pub fn slug(stem: &str) -> String {
    let mut out = String::new();
    for c in stem.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The widths `add` encodes for a source this wide.
pub fn ladder(source_width: u32) -> Vec<u32> {
    let mut out: Vec<u32> = LADDER.iter().copied().filter(|w| *w < source_width).collect();
    out.push(source_width);
    out
}

/// Replace `media:<name>` references with addresses. Returns the text and
/// the names that have no record, for the caller to report.
pub fn rewrite_refs(text: &str, media: Option<&Media>, lib: &Library, re: &Regex) -> (String, Vec<String>) {
    if !text.contains(PREFIX) {
        return (text.to_string(), Vec::new());
    }
    let mut unknown = Vec::new();
    let out = re.replace_all(text, |c: &regex::Captures| {
        let name = &c[2];
        match (media, lib.get(name)) {
            (Some(m), Some(rec)) => format!("{}{}", &c[1], m.url_for(&rec.main_key(name))),
            _ => {
                if !unknown.iter().any(|u| u == name) {
                    unknown.push(name.to_string());
                }
                c[0].to_string()
            }
        }
    });
    (out.into_owned(), unknown)
}

/// The error for a `media:<name>` the records do not know.
pub fn unknown_error(name: &str, media: Option<&Media>, file: &str) -> MageError {
    match media {
        None => MageError::in_file(format!("media:{name} needs a [media] table in site.toml"), file)
            .fix("add [media] with url, bucket and account to site.toml (see Media in `magehat -h`)"),
        Some(_) => MageError::in_file(format!("media:{name} has no record in {DIR}"), file).fix(format!("magehat media add <file> {name}")),
    }
}

// -- commands

fn require(root: &Path) -> Result<(Media, String)> {
    let cfg = load_config(root)?;
    let media = cfg.media.ok_or_else(|| {
        MageError::in_file("this site has no [media] table", "site.toml")
            .fix("add [media] with url, bucket and account (see Media in `magehat -h`)")
    })?;
    let token = std::env::var(TOKEN_VAR).ok().filter(|t| !t.trim().is_empty()).ok_or_else(|| {
        MageError::new(format!("{TOKEN_VAR} is not set"))
            .fix("run this through the tool that holds the Cloudflare API token; the token needs R2 edit rights on the account")
    })?;
    Ok((media, token))
}

fn mime_for(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "json" => "application/json",
        "vtt" => "text/vtt",
        "txt" | "srt" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// `magehat media add <file> [name]`: encode, upload, write the record.
pub fn add(root: &Path, file: &str, name: Option<&str>) -> Result<String> {
    let (media, token) = require(root)?;
    let path = Path::new(file);
    let bytes = std::fs::read(path).map_err(|e| MageError::new(format!("cannot read {file}: {e}")))?;
    if bytes.is_empty() {
        return Err(MageError::new(format!("{file} is empty")));
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
    if ext.is_empty() {
        return Err(MageError::new(format!("{file} has no extension, so its type is unknown")).fix("rename it with the extension of what it is"));
    }
    let name = match name {
        Some(n) => n.to_string(),
        None => slug(stem),
    };
    if !valid_name(&name) {
        return Err(MageError::new(format!("media name {name:?} must be lowercase letters, digits and dashes"))
            .fix(format!("magehat media add {file} {}", slug(&name))));
    }
    let hash = digest_bytes(&bytes);
    let record_path = root.join(DIR).join(format!("{name}.json"));
    let existing = load(root)?.shift_remove(&name);
    if let Some(prev) = &existing {
        if prev.hash != hash {
            return Err(MageError::in_file(format!("{name} already names a different file"), &format!("{DIR}/{name}.json"))
                .fix(format!("pick another name, or delete {DIR}/{name}.json to replace it (the old objects go on the next prune)")));
        }
    }

    // An image gets the ladder; anything else goes up as it is.
    let fmt = Fmt::from_key(&format!("x.{ext}"));
    let image = fmt.and_then(|f| {
        let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok()?.into_dimensions().ok()?;
        (w > 0 && h > 0).then_some((f, w, h))
    });
    let record = match image {
        Some((f, w, h)) => Record { mime: mime_for(f.ext()).into(), ext: f.ext().into(), hash: hash.clone(), size: bytes.len() as u64, width: Some(w), height: Some(h), widths: ladder(w) },
        None => Record { mime: mime_for(&ext).into(), ext: ext.clone(), hash: hash.clone(), size: bytes.len() as u64, width: None, height: None, widths: Vec::new() },
    };

    let r2 = R2::new(API, &media.account, &media.bucket, &token);
    let mut uploaded = 0usize;
    match image {
        Some((src_fmt, src_w, _)) => {
            let mut decoded = None;
            for &w in &record.widths {
                // WebP for every browser that takes it, the source format as the fallback;
                // a WebP source needs no second copy.
                let formats = if src_fmt == Fmt::Webp { vec![Fmt::Webp] } else { vec![Fmt::Webp, src_fmt] };
                for out_fmt in formats {
                    let encoded = encode_variant(&bytes, &mut decoded, src_w, src_fmt, w, out_fmt)
                        .map_err(|e| MageError::new(format!("cannot encode {file} at {w}px: {e}")).fix("re-export the image as a standard JPEG or PNG"))?;
                    r2.put(&record.variant_key(&name, w, out_fmt.ext()), &encoded, mime_for(out_fmt.ext()))?;
                    uploaded += 1;
                }
            }
        }
        None => {
            r2.put(&record.file_key(&name), &bytes, &record.mime)?;
            uploaded += 1;
        }
    }

    std::fs::create_dir_all(root.join(DIR))?;
    let json = serde_json::to_string_pretty(&record).unwrap() + "\n";
    std::fs::write(&record_path, json)?;
    let what = match (record.width, record.height) {
        (Some(w), Some(h)) => format!("{w}x{h} {}, {} widths", record.mime, record.widths.len()),
        _ => format!("{} bytes {}", record.size, record.mime),
    };
    let verb = if existing.is_some() { "re-uploaded" } else { "uploaded" };
    Ok(format!(
        "{PREFIX}{name}\n  {what}, {uploaded} objects {verb} to {}\n  record {DIR}/{name}.json: commit it\n  main file {}",
        media.bucket,
        media.url_for(&record.main_key(&name))
    ))
}

/// `magehat media prune [--dry-run]`: delete what no record names.
pub fn prune(root: &Path, dry_run: bool) -> Result<String> {
    let (media, token) = require(root)?;
    let lib = load(root)?;
    let mut expected = std::collections::BTreeSet::new();
    for (name, rec) in &lib {
        expected.extend(rec.keys(name));
    }
    let r2 = R2::new(API, &media.account, &media.bucket, &token);
    let objects = r2.list()?;
    let today = days_since_epoch_now();
    let mut lines = Vec::new();
    let mut deleted = 0usize;
    let mut young = 0usize;
    let mut present = std::collections::BTreeSet::new();
    for obj in &objects {
        if expected.contains(&obj.key) {
            present.insert(obj.key.clone());
            continue;
        }
        let age = obj.last_modified.as_deref().and_then(days_since_epoch).map(|d| today.saturating_sub(d));
        // An object whose date cannot be read is treated as young: keeping a
        // stray file costs nothing, deleting a fresh cover breaks a story.
        if age.map_or(true, |a| a < GRACE_DAYS) {
            young += 1;
            continue;
        }
        if dry_run {
            lines.push(format!("would delete {}", obj.key));
        } else {
            r2.delete(&obj.key)?;
            deleted += 1;
        }
    }
    let missing: Vec<&String> = expected.iter().filter(|k| !present.contains(*k)).collect();
    let mut summary = if dry_run {
        format!("{} objects in {}: {} referenced by records, {} would be deleted, {} younger than {GRACE_DAYS} days kept", objects.len(), media.bucket, present.len(), lines.len(), young)
    } else {
        format!("{} objects in {}: {} referenced by records, {deleted} deleted, {young} younger than {GRACE_DAYS} days kept", objects.len(), media.bucket, present.len())
    };
    for l in lines {
        summary.push_str(&format!("\n  {l}"));
    }
    if !missing.is_empty() {
        summary.push_str(&format!("\nwarning: {} objects named by records are missing from the bucket:", missing.len()));
        for k in missing {
            summary.push_str(&format!("\n  {k}"));
        }
        summary.push_str("\n  fix: run `magehat media add <original file> <name>` again for each; the record stays as it is");
    }
    Ok(summary)
}

// -- the bucket

pub struct Object {
    pub key: String,
    pub last_modified: Option<String>,
}

/// The Cloudflare API's object endpoints, the same ones wrangler uses, so
/// the account's existing API token is enough and no S3 key is created.
pub struct R2 {
    agent: ureq::Agent,
    objects: String,
    token: String,
}

impl R2 {
    pub fn new(api: &str, account: &str, bucket: &str, token: &str) -> Self {
        // Status codes are read, not turned into errors: the API's own error
        // message is in the body and is what the user needs to see.
        let config = ureq::Agent::config_builder().http_status_as_error(false).build();
        R2 { agent: ureq::Agent::new_with_config(config), objects: format!("{}/accounts/{account}/r2/buckets/{bucket}/objects", api.trim_end_matches('/')), token: token.to_string() }
    }

    fn fail(what: &str, status: u16, body: &str) -> MageError {
        let detail = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v["errors"][0]["message"].as_str().map(String::from))
            .unwrap_or_else(|| body.chars().take(200).collect());
        let e = MageError::new(format!("{what} failed ({status}): {detail}"));
        match status {
            401 | 403 => e.fix("the token needs Workers R2 Storage edit rights on this account; check account and bucket in site.toml"),
            404 => e.fix("create the bucket first: wrangler r2 bucket create <bucket> --location enam"),
            _ => e,
        }
    }

    pub fn put(&self, key: &str, bytes: &[u8], content_type: &str) -> Result<()> {
        let url = format!("{}/{key}", self.objects);
        let mut resp = self
            .agent
            .put(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Content-Type", content_type)
            // The name carries the content hash, so the object never changes.
            .header("Cache-Control", "public, max-age=31536000, immutable")
            .send(bytes)
            .map_err(|e| MageError::new(format!("upload of {key} failed: {e}")))?;
        let status = resp.status().as_u16();
        if status == 200 {
            return Ok(());
        }
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        Err(Self::fail(&format!("upload of {key}"), status, &body))
    }

    pub fn list(&self) -> Result<Vec<Object>> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut url = format!("{}?per_page=1000", self.objects);
            if let Some(c) = &cursor {
                url.push_str(&format!("&cursor={c}"));
            }
            let mut resp = self
                .agent
                .get(&url)
                .header("Authorization", &format!("Bearer {}", self.token))
                .call()
                .map_err(|e| MageError::new(format!("listing the bucket failed: {e}")))?;
            let status = resp.status().as_u16();
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            if status != 200 {
                return Err(Self::fail("listing the bucket", status, &body));
            }
            let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| MageError::new(format!("listing the bucket returned something that is not JSON: {e}")))?;
            for o in v["result"].as_array().into_iter().flatten() {
                if let Some(key) = o["key"].as_str() {
                    out.push(Object { key: key.to_string(), last_modified: o["last_modified"].as_str().map(String::from) });
                }
            }
            let info = &v["result_info"];
            let truncated = info["is_truncated"].as_bool().unwrap_or(false);
            cursor = info["cursor"].as_str().filter(|c| !c.is_empty()).map(String::from);
            if !truncated || cursor.is_none() {
                return Ok(out);
            }
        }
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        let url = format!("{}/{key}", self.objects);
        let mut resp = self
            .agent
            .delete(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| MageError::new(format!("deleting {key} failed: {e}")))?;
        let status = resp.status().as_u16();
        if status == 200 || status == 204 || status == 404 {
            return Ok(());
        }
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        Err(Self::fail(&format!("deleting {key}"), status, &body))
    }
}

/// Days since 1970-01-01 of an ISO 8601 timestamp's date part. The hour does
/// not matter at a 14-day grace.
pub fn days_since_epoch(iso: &str) -> Option<u64> {
    let mut parts = iso.get(..10)?.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    u64::try_from(days).ok()
}

fn days_since_epoch_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86400).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_record() -> Record {
        Record { mime: "image/jpeg".into(), ext: "jpg".into(), hash: "abcdef0123".into(), size: 10, width: Some(1600), height: Some(900), widths: ladder(1600) }
    }

    #[test]
    fn names_are_slugs() {
        assert_eq!(slug("Car Park Capital (1).JPG"), "car-park-capital-1-jpg");
        assert_eq!(slug("--cover--"), "cover");
        assert!(valid_name("car-park-1"));
        assert!(!valid_name("Car") && !valid_name("-x") && !valid_name(""));
    }

    #[test]
    fn the_ladder_stops_at_the_source() {
        assert_eq!(ladder(1600), vec![400, 800, 1200, 1600]);
        assert_eq!(ladder(1000), vec![400, 800, 1000]);
        assert_eq!(ladder(300), vec![300]);
        assert_eq!(ladder(3000), vec![400, 800, 1200, 1600, 2000, 2400, 3000]);
    }

    #[test]
    fn keys_name_every_variant_and_the_main_file() {
        let r = image_record();
        let keys = r.keys("cover");
        assert_eq!(keys.len(), 8);
        assert!(keys.contains(&"cover.abcdef0123.400.webp".to_string()));
        assert!(keys.contains(&"cover.abcdef0123.1600.jpg".to_string()));
        assert_eq!(r.main_key("cover"), "cover.abcdef0123.1600.jpg");
        let f = Record { mime: "video/mp4".into(), ext: "mp4".into(), hash: "ff".into(), size: 1, width: None, height: None, widths: Vec::new() };
        assert_eq!(f.keys("trailer"), vec!["trailer.ff.mp4"]);
        assert_eq!(f.main_key("trailer"), "trailer.ff.mp4");
        let webp = Record { ext: "webp".into(), mime: "image/webp".into(), ..image_record() };
        assert_eq!(webp.keys("w").len(), 4, "a WebP source has no second format");
    }

    #[test]
    fn records_round_trip_without_empty_fields() {
        let f = Record { mime: "video/mp4".into(), ext: "mp4".into(), hash: "ff".into(), size: 1, width: None, height: None, widths: Vec::new() };
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains("width") && !json.contains("widths"), "{json}");
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), f);
    }

    #[test]
    fn references_become_addresses_and_unknown_names_are_reported() {
        let media = Media { url: "https://media.example.com".into(), bucket: "b".into(), account: "a".into() };
        let mut lib = Library::new();
        lib.insert("cover".into(), image_record());
        let html = r#"<a href="media:cover">x</a><meta content='media:cover'>{"image":"media:cover"} url(media:cover) <a href="media:nope">y</a> <meta content="https://site.example/media:cover"> media:prose"#;
        let (out, unknown) = rewrite_refs(html, Some(&media), &lib, &ref_pattern("https://site.example/"));
        assert!(out.contains("href=\"https://media.example.com/cover.abcdef0123.1600.jpg\""), "{out}");
        assert!(out.contains("<meta content=\"https://media.example.com/cover.abcdef0123.1600.jpg\">"), "the site's own address in front is dropped: {out}");
        assert!(out.contains("content='https://media.example.com/cover.abcdef0123.1600.jpg'"));
        assert!(out.contains("\"image\":\"https://media.example.com/cover.abcdef0123.1600.jpg\""));
        assert!(out.contains("url(https://media.example.com/cover.abcdef0123.1600.jpg)"));
        assert!(out.contains("href=\"media:nope\"") && out.ends_with("media:prose"), "{out}");
        assert_eq!(unknown, vec!["nope"]);
    }

    #[test]
    fn dates_turn_into_days() {
        assert_eq!(days_since_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(days_since_epoch("2000-03-01T12:00:00.000Z"), Some(11017));
        assert_eq!(days_since_epoch("2026-10-09T23:59:59Z").unwrap() - days_since_epoch("2026-09-25T00:00:00Z").unwrap(), 14);
        assert_eq!(days_since_epoch("yesterday"), None);
    }

    /// A stand-in for the Cloudflare API: records what it was asked and
    /// answers like the real one, including a two-page listing.
    fn fake_api(responses: Vec<(u16, String)>) -> (String, std::sync::mpsc::Receiver<(String, String, Vec<(String, String)>, Vec<u8>)>, std::thread::JoinHandle<()>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr().to_ip().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            for (status, body) in responses {
                let mut req = server.recv().unwrap();
                let headers: Vec<(String, String)> = req.headers().iter().map(|h| (h.field.as_str().to_string().to_ascii_lowercase(), h.value.as_str().to_string())).collect();
                let mut data = Vec::new();
                let _ = std::io::Read::read_to_end(req.as_reader(), &mut data);
                tx.send((req.method().as_str().to_string(), req.url().to_string(), headers, data)).unwrap();
                let _ = req.respond(tiny_http::Response::from_string(body).with_status_code(status));
            }
        });
        (base, rx, handle)
    }

    #[test]
    fn the_client_speaks_the_cloudflare_object_api() {
        let page1 = r#"{"success":true,"result":[{"key":"a.1.400.webp","last_modified":"2026-10-01T00:00:00.000Z"}],"result_info":{"cursor":"c2","is_truncated":true}}"#;
        let page2 = r#"{"success":true,"result":[{"key":"old.0.mp4","last_modified":"2026-01-01T00:00:00.000Z"}],"result_info":{"cursor":"","is_truncated":false}}"#;
        let (base, rx, handle) = fake_api(vec![
            (200, r#"{"success":true,"result":{"key":"a.1.400.webp"}}"#.into()),
            (200, page1.into()),
            (200, page2.into()),
            (200, r#"{"success":true,"result":{"key":"old.0.mp4"}}"#.into()),
            (403, r#"{"success":false,"errors":[{"code":10000,"message":"Authentication error"}]}"#.into()),
        ]);
        let r2 = R2::new(&base, "acct", "bucket", "tok");
        r2.put("a.1.400.webp", b"bytes", "image/webp").unwrap();
        let (method, url, headers, data) = rx.recv().unwrap();
        assert_eq!((method.as_str(), url.as_str()), ("PUT", "/accounts/acct/r2/buckets/bucket/objects/a.1.400.webp"));
        assert!(headers.contains(&("authorization".into(), "Bearer tok".into())), "{headers:?}");
        assert!(headers.contains(&("content-type".into(), "image/webp".into())), "{headers:?}");
        assert!(headers.iter().any(|(k, v)| k == "cache-control" && v.contains("immutable")), "{headers:?}");
        assert_eq!(data, b"bytes");

        let objects = r2.list().unwrap();
        let (_, url1, _, _) = rx.recv().unwrap();
        let (_, url2, _, _) = rx.recv().unwrap();
        assert_eq!(url1, "/accounts/acct/r2/buckets/bucket/objects?per_page=1000");
        assert_eq!(url2, "/accounts/acct/r2/buckets/bucket/objects?per_page=1000&cursor=c2");
        assert_eq!(objects.iter().map(|o| o.key.as_str()).collect::<Vec<_>>(), vec!["a.1.400.webp", "old.0.mp4"]);
        assert_eq!(objects[1].last_modified.as_deref(), Some("2026-01-01T00:00:00.000Z"));

        r2.delete("old.0.mp4").unwrap();
        let (method, url, _, _) = rx.recv().unwrap();
        assert_eq!((method.as_str(), url.as_str()), ("DELETE", "/accounts/acct/r2/buckets/bucket/objects/old.0.mp4"));

        let err = r2.put("b", b"x", "text/plain").unwrap_err();
        assert!(err.message.contains("403") && err.message.contains("Authentication error"), "{err}");
        assert!(err.fix.as_deref().unwrap_or_default().contains("R2"), "{err}");
        handle.join().unwrap();
    }
}
