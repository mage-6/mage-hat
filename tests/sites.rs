//! Whole-site builds: the sample site (byte-for-byte golden), a bilingual
//! site, images, assets, and the agent-facing error output.

use magehat::build::{build_site, BuildResult};
use magehat::check::{report_json, run_check};
use magehat::init::init_site;
use magehat::inspect::inspect_site;
use std::path::{Path, PathBuf};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden").join("scaffold")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("magehat-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn scaffold(name: &str) -> PathBuf {
    let dir = temp(name);
    init_site(&dir).unwrap();
    dir
}

fn bilingual(name: &str) -> PathBuf {
    let dir = temp(name);
    copy_dir(&fixture("bilingual"), &dir);
    dir
}

fn text(r: &BuildResult, path: &str) -> String {
    String::from_utf8(r.outputs.get(path).unwrap_or_else(|| panic!("no output {path}")).clone()).unwrap()
}

fn all_files(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if entry.path().is_dir() {
            all_files(&entry.path(), &rel, out);
        } else {
            out.push(rel);
        }
    }
}

fn page(site: &Path, name: &str, body: &str) {
    std::fs::write(
        site.join("src/pages").join(name),
        format!("<title>T</title>\n<meta name=\"description\" content=\"d\">\n<x-base>\n{body}\n</x-base>\n"),
    )
    .unwrap();
}

#[test]
fn scaffold_builds_clean() {
    let site = scaffold("clean");
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>());
    for key in ["index.html", "about/index.html", "blog/index.html", "blog/hello-world/index.html", "404.html", "sitemap.xml", "robots.txt", "site.css", "blog/feed.xml"] {
        assert!(r.outputs.contains_key(key), "missing {key}");
    }
    assert!(site.join("AGENTS.md").is_file() && site.join(".claude/skills/magehat/SKILL.md").is_file() && site.join("CLAUDE.md").is_file());
}

#[test]
fn scaffold_page_content() {
    let r = build_site(&scaffold("content")).unwrap();
    let index = text(&r, "index.html");
    assert!(index.starts_with("<!doctype html>"));
    assert!(index.contains("<x-card title=\"A second post\" url=\"/blog/second-post/\""));
    assert!(index.find("second-post").unwrap() < index.find("hello-world").unwrap(), "newest first");
    assert!(index.contains("<link rel=\"canonical\" href=\"https://example.com/\">"));
    assert!(index.contains("/_mh/x-counter.") && index.contains(".js"));
    assert!(!text(&r, "about/index.html").contains("x-counter"), "assets only for components the page uses");
    let post = text(&r, "blog/hello-world/index.html");
    assert!(post.contains("<title>Hello, world · My Site</title>"));
    assert!(post.contains("<h2>Headings, lists, code</h2>"));
    assert!(!post.contains("<p><x-counter"), "a lone component in Markdown is not wrapped in <p>");
    let css = r.outputs.iter().find(|(k, _)| k.starts_with("_mh/x-card.")).map(|(_, v)| String::from_utf8(v.clone()).unwrap()).unwrap();
    assert!(css.starts_with("x-card{display:contents}@scope (x-card) to (:scope :is("), "{css}");
}

#[test]
fn output_is_minified_and_assets_hashed() {
    let r = build_site(&scaffold("minify")).unwrap();
    let index = text(&r, "index.html");
    assert!(!index.contains("\n  "), "indentation removed:\n{index}");
    assert!(!index.contains("<!--"), "comments removed");
    let hashed = r.outputs.keys().find(|k| k.starts_with("site.") && k.ends_with(".css") && k.len() == "site.0123456789.css".len()).cloned().expect("hashed site.css");
    assert!(index.contains(&format!("href=\"/{hashed}\"")), "layout points at the hashed stylesheet");
    assert!(r.outputs.contains_key("site.css"), "original kept");
    assert!(!text(&r, &hashed).contains("\n"), "css minified");
}

#[test]
fn images_get_picture_sources_and_variants() {
    let site = scaffold("images");
    let r = build_site(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let about = text(&r, "about/index.html");
    let start = about.find("<picture>").expect("picture element");
    let picture = &about[start..about[start..].find("</picture>").unwrap() + start];
    assert!(picture.contains("<source type=\"image/webp\" srcset=\"/_mh/img/hat."), "{picture}");
    assert!(picture.contains(".600.webp 600w, /_mh/img/hat.") && picture.contains(".1200.webp 1200w\""), "{picture}");
    assert!(picture.contains("sizes=\"(max-width: 600px) 100vw, 600px\""));
    assert!(picture.contains("width=\"600\" height=\"400\""), "{picture}");
    assert!(picture.contains("loading=\"lazy\"") && picture.contains("decoding=\"async\"") && picture.contains("alt=\"A hat"), "{picture}");
    let variants: Vec<&String> = r.outputs.keys().filter(|k| k.starts_with("_mh/img/")).collect();
    assert_eq!(variants.len(), 4, "{variants:?}");
    assert!(site.join(".magehat/cache/img").is_dir());
    assert_eq!(build_site(&site).unwrap().outputs, r.outputs, "cached variants are byte-identical");
}

#[test]
fn build_is_deterministic() {
    let site = scaffold("determinism");
    assert_eq!(build_site(&site).unwrap().outputs, build_site(&site).unwrap().outputs);
}

#[test]
fn inspect_describes_the_site() {
    let info = inspect_site(&scaffold("inspect")).unwrap();
    let card = info["components"].as_array().unwrap().iter().find(|c| c["tag"] == "x-card").unwrap();
    assert_eq!(card["props"], serde_json::json!(["date", "title", "url"]));
    assert_eq!(card["slots"], serde_json::json!([""]));
    assert_eq!(card["usage"], "<x-card date=\"...\" title=\"...\" url=\"...\">children</x-card>");
    let base = info["components"].as_array().unwrap().iter().find(|c| c["tag"] == "x-base").unwrap();
    assert_eq!(base["layout"], true);
    assert_eq!(info["collections"]["blog"]["item_var"], "post");
    let ids: Vec<&str> = info["collections"]["blog"]["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["second-post", "hello-world"]);
    assert!(info["i18n"]["en"]["keys"].as_array().unwrap().iter().any(|k| k == "nav.blog"));
}

#[test]
fn bilingual_site() {
    let r = build_site(&bilingual("bi")).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let pt = text(&r, "pt-br/index.html");
    assert!(pt.contains("<a href=\"/pt-br/blog/\">Blogue</a>"), "literal link localized");
    assert!(pt.contains("<a href=\"/about/\">Sobre</a>"), "no translation, link stays");
    assert!(pt.contains("<a href=\"/\" hreflang=\"en\">en</a>"), "computed link untouched");
    assert!(pt.contains("<a href=\"/pt-br/blog/ola/\">Olá</a>"), "translated slug");
    assert!(text(&r, "blog/hello/index.html").contains("<link rel=\"alternate\" hreflang=\"pt-BR\" href=\"https://bi.example/pt-br/blog/ola/\">"));
    assert!(r.outputs.contains_key("pt-br/blog/feed.xml"));
    assert!(!r.outputs.contains_key("pt-br/about/index.html"));
    assert!(text(&r, "pt-br/blog/ola/index.html").contains("<a href=\"/pt-br/blog/\">o blogue</a>"), "links inside content are localized too");
}

#[test]
fn bilingual_check_warnings() {
    let r = run_check(&bilingual("bi-check")).unwrap();
    assert!(r.errors.is_empty());
    let mut got: Vec<String> = r.warnings.iter().map(|w| w.to_string()).collect();
    got.sort();
    let mut expected = vec![
        "src/pages/about.html: page \"about\" has no translation for: pt-BR".to_string(),
        "src/content/blog/only-en.md: blog/only-en has no translation for: pt-BR".to_string(),
        "src/pages/about.html: broken link \"/nope/\" on /about/".to_string(),
        "src/i18n/pt-BR.json: missing keys present in en.json: hello".to_string(),
    ];
    expected.sort();
    assert_eq!(got, expected);
    assert!(r.warnings.iter().all(|w| w.fix.is_some()), "every warning says how to fix it");
}

#[test]
fn errors_say_how_to_fix_and_show_the_source() {
    let site = scaffold("errors");
    page(&site, "broken.html", "<ul><li v-for=\"x in xs\">{{ x }}</li></ul>");
    let r = build_site(&site).unwrap();
    assert_eq!(r.errors.len(), 1);
    let e = &r.errors[0];
    assert_eq!((e.file.as_deref(), e.line), (Some("src/pages/broken.html"), Some(4)));
    assert!(e.message.contains("v-for") && e.fix.as_deref().unwrap().contains("each="));
    assert_eq!(e.excerpt(&site).unwrap(), "    <ul><li v-for=\"x in xs\">{{ x }}</li></ul>\n            ^^^^^");
    let json = report_json(&r);
    assert!(json["errors"][0]["excerpt"].as_str().unwrap().contains("^^^^^"));

    page(&site, "broken.html", "{{ post.title | upper }}");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("filters"));

    page(&site, "broken.html", "<x-card></x-card>");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("undefined variable 'url'"), "{}", r.errors[0]);
    assert!(r.errors[0].message.contains("used from src/pages/broken.html:4"));
    assert!(r.errors[0].fix.as_deref().unwrap().contains("Pass it as an attribute"));
}

#[test]
fn check_finds_markup_problems() {
    let site = scaffold("markup");
    page(&site, "sloppy.html", "<span>never closed\n<p id=\"a\">x</p><p id=\"a\">y</p>\n<img src=\"/photos/hat.jpg\">\n<a>no href</a>\n</section>");
    let r = run_check(&site).unwrap();
    let got: Vec<String> = r.warnings.iter().map(|w| w.to_string()).collect();
    assert!(got.iter().any(|w| w == "src/pages/sloppy.html:4: <span> is never closed"), "{got:?}");
    assert!(got.iter().any(|w| w == "src/pages/sloppy.html:8: stray </section> with no open <section>"), "{got:?}");
    assert!(got.iter().any(|w| w.contains("id \"a\" appears 2 times on /sloppy/")), "{got:?}");
    assert!(got.iter().any(|w| w.contains("1 <img> without alt on /sloppy/")), "{got:?}");
    assert!(got.iter().any(|w| w.contains("1 <a> without href on /sloppy/")), "{got:?}");
    assert!(r.warnings.iter().all(|w| w.fix.is_some()));
}

#[test]
fn new_creates_files_that_check_clean() {
    let site = scaffold("new");
    magehat::new::new(&site, "page", &["team".into()], None).unwrap();
    magehat::new::new(&site, "component", &["quote".into()], None).unwrap();
    magehat::new::new(&site, "item", &["blog".into(), "third-post".into()], None).unwrap();
    assert!(site.join("src/pages/team.html").is_file());
    assert!(site.join("src/components/quote.html").is_file());
    assert!(site.join("src/content/blog/third-post.md").is_file());
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>());
    assert!(r.outputs.contains_key("team/index.html") && r.outputs.contains_key("blog/third-post/index.html"));
    let e = magehat::new::new(&site, "page", &["team".into()], None).unwrap_err();
    assert!(e.message.contains("already exists"));
}

/// The four files shown under "A site from nothing" in SKILL.md, verbatim.
/// If this test fails, fix the doc or the tool; never let them drift.
#[test]
fn site_from_nothing_as_documented() {
    let skill = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("SKILL.md")).unwrap();
    let section = &skill[skill.find("## A site from nothing").unwrap()..skill.find("## Files").unwrap()];
    // Indented blocks, in order: site.toml, base.html, index.html. A blank
    // line stays inside a block when the next non-blank line is indented.
    let lines: Vec<&str> = section.lines().collect();
    let mut blocks: Vec<String> = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for (i, line) in lines.iter().enumerate() {
        let indented = line.starts_with("    ");
        let continues_block = line.trim().is_empty()
            && lines[i + 1..].iter().find(|l| !l.trim().is_empty()).map_or(false, |l| l.starts_with("    "));
        match (&mut current, indented || continues_block) {
            (Some(block), true) => block.push(line.get(4..).unwrap_or("")),
            (None, true) if indented => current = Some(vec![&line[4..]]),
            (Some(_), false) => blocks.push(current.take().unwrap().join("\n") + "\n"),
            _ => {}
        }
    }
    if let Some(block) = current {
        blocks.push(block.join("\n") + "\n");
    }
    assert_eq!(blocks.len(), 3, "expected three file blocks in the doc, got {}", blocks.len());
    let site = temp("from-nothing");
    std::fs::create_dir_all(site.join("src/components")).unwrap();
    std::fs::create_dir_all(site.join("src/pages")).unwrap();
    std::fs::create_dir_all(site.join("src/assets")).unwrap();
    std::fs::write(site.join("site.toml"), &blocks[0]).unwrap();
    std::fs::write(site.join("src/components/base.html"), &blocks[1]).unwrap();
    std::fs::write(site.join("src/pages/index.html"), &blocks[2]).unwrap();
    std::fs::write(site.join("src/assets/site.css"), "body { margin: 0 }\n").unwrap();
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>());
    let index = text(&r, "index.html");
    assert!(index.contains("<title>Home · Hat Co</title>") && index.contains("<h1>Hats, made by hand.</h1>"), "{index}");
    assert!(index.contains("<link rel=\"canonical\" href=\"https://example.com/\">"));
}

#[test]
fn canonical_forms_are_enforced() {
    let site = scaffold("canonical");
    std::fs::write(site.join("src/components/loose.html"), "<div>{{ x }}</div>").unwrap();
    let e = build_site(&site).err().expect("expected an error");
    assert!(e.message.contains("no <template>") && e.fix.is_some());
    std::fs::remove_file(site.join("src/components/loose.html")).unwrap();

    std::fs::create_dir_all(site.join("src/content/blog/nested")).unwrap();
    std::fs::write(site.join("src/content/blog/nested/en.md"), "---\ntitle: x\n---\nx").unwrap();
    let e = build_site(&site).err().expect("expected an error");
    assert!(e.message.contains("sub-folders"));
    assert!(e.fix.as_deref().unwrap().contains("src/content/blog/nested.md"));
}

/// Byte-for-byte snapshot of the sample site. Regenerate with UPDATE_GOLDEN=1.
#[test]
fn scaffold_matches_golden() {
    let outputs = build_site(&scaffold("golden")).unwrap().outputs;
    let golden = golden_dir();
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        let _ = std::fs::remove_dir_all(&golden);
        for (rel, data) in &outputs {
            let path = golden.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, data).unwrap();
        }
    }
    let mut expected = Vec::new();
    all_files(&golden, "", &mut expected);
    expected.sort();
    let mut got: Vec<String> = outputs.keys().cloned().collect();
    got.sort();
    assert_eq!(got, expected, "set of output files differs from golden");
    for (rel, data) in &outputs {
        let want = std::fs::read(golden.join(rel)).unwrap();
        assert!(data == &want, "{rel} differs from golden:\n{}", String::from_utf8_lossy(data));
    }
}
