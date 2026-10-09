//! Images, handled from a plain <img>.
//!
//!     <img src="/photos/hat.jpg" alt="A hat">
//!     <img src="/photos/hat.jpg" alt="A hat" width="800">
//!     <img src="media:cover" alt="A hat" width="800">
//!
//! Any <img> whose src is a JPEG, PNG or WebP under src/assets gets its
//! width and height filled in (no layout shift), loading="lazy" and
//! decoding="async" unless set, and a WebP <source> in a <picture>. With a
//! `width`, resized variants at 1x and 2x are generated and offered through
//! srcset. Variants are cached in .magehat/cache by content hash, so an
//! unchanged image is never encoded twice. An <img> already inside a
//! <picture> is left alone.
//!
//! A `media:` image (media.rs) gets the same markup from its record alone:
//! the variants were encoded by `magehat media add` and live in the bucket,
//! so the build writes their addresses and never touches the file.

use crate::build::BuildResult;
use crate::components::digest_bytes;
use crate::htmltree::{scan_start_tag, strip_attrs};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader};
use regex::Regex;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static IMG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<img\b[^>]*>").unwrap());

pub const OUT_DIR: &str = "_mh/img";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fmt {
    Jpeg,
    Png,
    Webp,
}

impl Fmt {
    pub fn from_key(key: &str) -> Option<Fmt> {
        match key.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
            Some("jpg") | Some("jpeg") => Some(Fmt::Jpeg),
            Some("png") => Some(Fmt::Png),
            Some("webp") => Some(Fmt::Webp),
            _ => None,
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Fmt::Jpeg => "jpg",
            Fmt::Png => "png",
            Fmt::Webp => "webp",
        }
    }
}

struct Source {
    bytes: Vec<u8>,
    hash: String,
    width: u32,
    height: u32,
    fmt: Fmt,
    decoded: Option<DynamicImage>,
}

/// One encoded size of an image: the address to write and its pixel width.
#[derive(Clone)]
struct Variant {
    url: String,
    width: u32,
}

pub struct Images<'a> {
    cache_dir: PathBuf,
    sources: HashMap<String, Option<Source>>,
    r: &'a mut BuildResult,
}

impl<'a> Images<'a> {
    pub fn new(r: &'a mut BuildResult, cache_dir: &Path) -> Self {
        Images { cache_dir: cache_dir.to_path_buf(), sources: HashMap::new(), r }
    }

    /// Rewrite every eligible <img> in every page.
    pub fn process(mut self) {
        let pages: Vec<(String, String)> = self.r.pages.iter().map(|p| (p.out.clone(), p.file.clone())).collect();
        for (out, file) in pages {
            let html = String::from_utf8_lossy(&self.r.outputs[&out]).to_string();
            let rewritten = self.rewrite_page(&html, &out, &file);
            self.r.outputs.insert(out, rewritten.into_bytes());
        }
    }

    fn rewrite_page(&mut self, html: &str, page_out: &str, file: &str) -> String {
        let mut result = String::with_capacity(html.len());
        let mut last = 0;
        for m in IMG.find_iter(html) {
            result.push_str(&html[last..m.start()]);
            last = m.end();
            let before = &html[..m.start()];
            let inside_picture = count_ci(before, "<picture") > count_ci(before, "</picture");
            match if inside_picture { None } else { self.rewrite_img(m.as_str(), page_out, file) } {
                Some(tag) => result.push_str(&tag),
                None => result.push_str(m.as_str()),
            }
        }
        result.push_str(&html[last..]);
        result
    }

    fn rewrite_img(&mut self, tag: &str, page_out: &str, file: &str) -> Option<String> {
        let (_, attrs, _) = scan_start_tag(tag)?;
        let attr = |name: &str| attrs.iter().find(|(k, _)| k == name).and_then(|(_, v)| v.clone());
        let src = attr("src")?;
        let requested: Option<u32> = attr("width").and_then(|w| w.trim().parse().ok()).filter(|w| *w > 0);
        let (src_w, src_h, fmt, webp, fallback) = match src.trim().strip_prefix(crate::media::PREFIX) {
            Some(name) => self.media_variants(name, requested, file)?,
            None => self.local_variants(&resolve_key(page_out, &src)?, requested, file)?,
        };
        let display_w = requested.map_or(src_w, |w| w.min(src_w));
        let display_h = ((display_w as u64 * src_h as u64 + src_w as u64 / 2) / src_w as u64).max(1) as u32;

        let sizes = requested.map(|_| format!("(max-width: {display_w}px) 100vw, {display_w}px"));
        let srcset = |vs: &[Variant]| vs.iter().map(|v| format!("{} {}w", v.url, v.width)).collect::<Vec<_>>().join(", ");
        // The plain src is the smallest variant that is not upscaled on display.
        let main = fallback.iter().find(|v| v.width >= display_w).or(fallback.last())?;
        let rest = strip_attrs(tag, &["src", "srcset", "sizes", "width", "height"]);
        let rest = rest.trim_start_matches("<img").trim_end_matches('>').trim_end_matches('/').trim_end();
        let mut img = format!("<img src=\"{}\" width=\"{display_w}\" height=\"{display_h}\"", main.url);
        if fallback.len() > 1 {
            img.push_str(&format!(" srcset=\"{}\"", srcset(&fallback)));
            if let Some(s) = &sizes {
                img.push_str(&format!(" sizes=\"{s}\""));
            }
        }
        if attr("loading").is_none() {
            img.push_str(" loading=\"lazy\"");
        }
        if attr("decoding").is_none() {
            img.push_str(" decoding=\"async\"");
        }
        img.push_str(rest);
        img.push('>');
        if fmt == Fmt::Webp {
            return Some(img);
        }
        let mut source = format!("<source type=\"image/webp\" srcset=\"{}\"", srcset(&webp));
        if webp.len() > 1 {
            if let Some(s) = &sizes {
                source.push_str(&format!(" sizes=\"{s}\""));
            }
        }
        source.push('>');
        Some(format!("<picture>{source}{img}</picture>"))
    }

    /// Variants of an image under src/assets: 1x and 2x of the display width,
    /// encoded now. Returns (source width, source height, format, webp, fallback).
    fn local_variants(&mut self, key: &str, requested: Option<u32>, file: &str) -> Option<(u32, u32, Fmt, Vec<Variant>, Vec<Variant>)> {
        let source = self.source(key)?;
        let (src_w, src_h, fmt, hash) = (source.width, source.height, source.fmt, source.hash.clone());
        let display_w = requested.map_or(src_w, |w| w.min(src_w));

        // Candidate widths: 1x and 2x of the display width, never above the source.
        let mut widths = vec![display_w];
        if display_w * 2 <= src_w {
            widths.push(display_w * 2);
        } else if display_w < src_w {
            widths.push(src_w);
        }

        let stem = key.rsplit('/').next().unwrap_or(key).rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(key.to_string());
        let mut webp: Vec<Variant> = Vec::new();
        let mut fallback: Vec<Variant> = Vec::new();
        for &w in &widths {
            webp.push(self.variant(key, &stem, &hash, w, Fmt::Webp, file)?);
            if fmt != Fmt::Webp {
                fallback.push(self.variant(key, &stem, &hash, w, fmt, file)?);
            }
        }
        if fmt == Fmt::Webp {
            fallback = webp.clone();
        }
        Some((src_w, src_h, fmt, webp, fallback))
    }

    /// Variants of a `media:` image, from its record: the ladder up to twice
    /// the display width, addressed in the bucket.
    fn media_variants(&mut self, name: &str, requested: Option<u32>, file: &str) -> Option<(u32, u32, Fmt, Vec<Variant>, Vec<Variant>)> {
        let media = self.r.cfg.media.clone();
        let rec = match (&media, self.r.media.get(name).cloned()) {
            (Some(_), Some(rec)) => rec,
            _ => {
                self.r.errors.push(crate::media::unknown_error(name, media.as_ref(), file));
                return None;
            }
        };
        let media = media?;
        let (Some(src_w), Some(src_h), Some(fmt)) = (rec.width, rec.height, Fmt::from_key(&format!("x.{}", rec.ext)).filter(|_| rec.is_image())) else {
            self.r.errors.push(
                crate::errors::MageError::in_file(format!("media:{name} is a {} file, not an image an <img> can show", rec.mime), file)
                    .fix("link to it, or put it in <video>, <audio> or <source> instead"),
            );
            return None;
        };
        let display_w = requested.map_or(src_w, |w| w.min(src_w));
        let mut widths: Vec<u32> = rec.widths.iter().copied().filter(|w| *w <= display_w * 2).collect();
        // Always one size that is not upscaled on display.
        if let Some(up) = rec.widths.iter().copied().find(|w| *w >= display_w) {
            if !widths.contains(&up) {
                widths.push(up);
            }
        }
        if widths.is_empty() {
            widths.push(rec.widths[0]);
        }
        let make = |ext: &str| widths.iter().map(|&w| Variant { url: media.url_for(&rec.variant_key(name, w, ext)), width: w }).collect::<Vec<_>>();
        let webp = make("webp");
        let fallback = if fmt == Fmt::Webp { webp.clone() } else { make(fmt.ext()) };
        Some((src_w, src_h, fmt, webp, fallback))
    }

    fn source(&mut self, key: &str) -> Option<&mut Source> {
        if !self.sources.contains_key(key) {
            let loaded = self.load_source(key);
            self.sources.insert(key.to_string(), loaded);
        }
        self.sources.get_mut(key).and_then(|s| s.as_mut())
    }

    fn load_source(&mut self, key: &str) -> Option<Source> {
        let fmt = Fmt::from_key(key)?;
        let bytes = self.r.outputs.get(key)?.clone();
        let (width, height) = ImageReader::new(Cursor::new(&bytes)).with_guessed_format().ok()?.into_dimensions().ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        Some(Source { hash: digest_bytes(&bytes), bytes, width, height, fmt, decoded: None })
    }

    /// Produce (or fetch from cache) one encoded variant and register it as an output.
    fn variant(&mut self, key: &str, stem: &str, hash: &str, width: u32, fmt: Fmt, file: &str) -> Option<Variant> {
        let out = format!("{OUT_DIR}/{stem}.{hash}.{width}.{}", fmt.ext());
        let url = format!("/{out}");
        if self.r.outputs.contains_key(&out) {
            return Some(Variant { url, width });
        }
        let cache_path = self.cache_dir.join(format!("{hash}.{width}.{}", fmt.ext()));
        let bytes = match std::fs::read(&cache_path) {
            Ok(b) if !b.is_empty() => b,
            _ => {
                let encoded = match self.encode(key, width, fmt) {
                    Ok(b) => b,
                    Err(e) => {
                        self.r.warn(format!("image /{key} left as-is: {e}"), Some(file), Some("re-export the image as a standard JPEG or PNG"));
                        return None;
                    }
                };
                let _ = std::fs::create_dir_all(&self.cache_dir);
                let _ = std::fs::write(&cache_path, &encoded);
                encoded
            }
        };
        self.r.outputs.insert(out, bytes);
        Some(Variant { url, width })
    }

    fn encode(&mut self, key: &str, width: u32, fmt: Fmt) -> std::result::Result<Vec<u8>, String> {
        let source = self.sources.get_mut(key).and_then(|s| s.as_mut()).ok_or("no source")?;
        let Source { bytes, decoded, width: src_w, fmt: src_fmt, .. } = source;
        encode_variant(bytes, decoded, *src_w, *src_fmt, width, fmt)
    }
}

/// One variant of an image: `width` pixels wide in `fmt`. `decoded` caches the
/// decoded source across calls for the same image. The original at its own
/// size and format is returned as it is: re-encoding could only lose quality
/// or grow it.
pub fn encode_variant(bytes: &[u8], decoded: &mut Option<DynamicImage>, src_w: u32, src_fmt: Fmt, width: u32, fmt: Fmt) -> std::result::Result<Vec<u8>, String> {
    if width >= src_w && fmt == src_fmt {
        return Ok(bytes.to_vec());
    }
    if decoded.is_none() {
        let img = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())?;
        *decoded = Some(img);
    }
    let full = decoded.as_ref().unwrap();
    let resized;
    let img: &DynamicImage = if width < src_w {
        resized = full.resize(width, u32::MAX, FilterType::Lanczos3);
        &resized
    } else {
        full
    };
    let mut buf = Cursor::new(Vec::new());
    match fmt {
        Fmt::Webp => {
            let rgba = img.to_rgba8();
            let encoder = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height());
            return Ok(encoder.encode(80.0).to_vec());
        }
        Fmt::Jpeg => {
            let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 82);
            rgb.write_with_encoder(encoder).map_err(|e| e.to_string())?;
        }
        Fmt::Png => img.write_to(&mut buf, ImageFormat::Png).map_err(|e| e.to_string())?,
    }
    Ok(buf.into_inner())
}

fn count_ci(haystack: &str, needle: &str) -> usize {
    haystack.to_ascii_lowercase().matches(needle).count()
}

/// Resolve an <img src> to an output key, or None for external and inline sources.
fn resolve_key(page_out: &str, src: &str) -> Option<String> {
    let v = src.trim();
    if v.is_empty() || v.starts_with("//") || v.starts_with("data:") || v.contains(':') || v.starts_with('#') {
        return None;
    }
    let path = v.split(['?', '#']).next().unwrap_or(v);
    if let Some(abs) = path.strip_prefix('/') {
        return Some(abs.to_string());
    }
    let dir = page_out.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => { parts.pop(); }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}
