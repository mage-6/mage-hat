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

fn news(name: &str) -> PathBuf {
    let dir = temp(name);
    copy_dir(&fixture("news"), &dir);
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
    for key in ["index.html", "about/index.html", "blog/index.html", "blog/hello-world/index.html", "tags/news/index.html", "tags/meta/index.html", "404.html", "sitemap.xml", "robots.txt", "site.css", "favicon.svg", "blog/feed.xml"] {
        assert!(r.outputs.contains_key(key), "missing {key}");
    }
    assert!(!site.join("AGENTS.md").exists() && !site.join(".claude").exists(), "init writes the site and nothing else");
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

/// The four files shown under "A site from nothing" in MANUAL.md, verbatim.
/// If this test fails, fix the doc or the tool; never let them drift.
#[test]
fn site_from_nothing_as_documented() {
    let skill = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("MANUAL.md")).unwrap();
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

/// A bare `magehat` lists the commands; the list is cut from MANUAL.md, so a
/// command documented there and nowhere else still shows up.
#[test]
fn bare_command_lists_commands_from_the_manual() {
    let usage = magehat::cli::usage();
    for cmd in ["magehat check", "magehat build", "magehat dev", "magehat init", "magehat clean", "magehat add"] {
        assert!(usage.contains(cmd), "{cmd} missing from:\n{usage}");
    }
    assert!(usage.contains("Run `magehat -h`"), "no pointer to the manual");
    assert!(!usage.contains("## "), "the markdown heading leaked in:\n{usage}");
}

#[test]
fn json_ld_scripts_are_templates() {
    let site = scaffold("jsonld");
    std::fs::write(
        site.join("src/pages/ld.html"),
        concat!(
            "<title>A </b> title</title>\n<meta name=\"description\" content=\"d\">\n<x-base>\n",
            "<script type=\"application/ld+json\">\n{ \"@type\": \"ItemList\", \"name\": \"{{ page.title }}\", \"items\": [\n",
            "  <template each=\"post in blog\">{ \"name\": \"{{ post.title }}\", \"url\": \"{{ site.url }}{{ post.url }}\" },</template>\n",
            "] }\n</script>\n<script>var keep = '{{ untouched }}';</script>\n</x-base>\n"
        ),
    )
    .unwrap();
    let r = build_site(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let html = text(&r, "ld/index.html");
    assert!(html.contains(concat!(
        "<script type=\"application/ld+json\">{\"@type\":\"ItemList\",\"name\":\"A <\\/b> title\",\"items\":[",
        "{\"name\":\"A second post\",\"url\":\"https://example.com/blog/second-post/\"},",
        "{\"name\":\"Hello, world\",\"url\":\"https://example.com/blog/hello-world/\"}]}</script>"
    )), "{html}");
    assert!(html.contains("var keep = '{{ untouched }}';"), "other scripts are untouched");
    let post = text(&r, "blog/hello-world/index.html");
    assert!(post.contains("\"keywords\":[\"news\",\"meta\"]") && post.contains("\"@type\":\"Article\""), "{post}");

    page(&site, "ld.html", "<script type=\"application/ld+json\">{ \"name\": {{ page.title }} }</script>");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.starts_with("JSON-LD is not valid JSON"), "{}", r.errors[0]);
    assert!(r.errors[0].fix.as_deref().unwrap().contains("quotes"));
    assert_eq!(r.errors[0].line, Some(4));

    page(&site, "ld.html", "<script type=\"application/ld+json\">{ \"name\": <b>x</b> }</script>");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("<b> inside a JSON-LD script"), "{}", r.errors[0]);
}

#[test]
fn icons_are_inlined_from_files() {
    let site = scaffold("icons");
    std::fs::create_dir_all(site.join("src/icons/own")).unwrap();
    std::fs::write(
        site.join("src/icons/own/dot.svg"),
        "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1em\" height=\"1em\" viewBox=\"0 0 2 2\">\n  <circle cx=\"1\" cy=\"1\" r=\"1\"/>\n</svg>\n",
    )
    .unwrap();
    page(&site, "icons.html", "<p><svg icon=\"own:dot\" class=\"i\" width=\"2em\"></svg><svg icon=\"{{ 'own:dot' }}\" aria-hidden=\"true\"></svg></p>");
    let r = build_site(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let html = text(&r, "icons/index.html");
    assert!(html.contains("<svg xmlns=\"http://www.w3.org/2000/svg\" height=\"1em\" viewBox=\"0 0 2 2\" class=\"i\" width=\"2em\"><circle cx=\"1\" cy=\"1\" r=\"1\"/></svg>"), "{html}");
    assert!(html.contains("viewBox=\"0 0 2 2\" aria-hidden=\"true\"><circle"), "{html}");
    assert!(!html.contains("icon="), "{html}");
    assert!(text(&r, "index.html").contains("<a href=\"/\" class=\"brand\"><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1em\" height=\"1em\" viewBox=\"0 0 24 24\" aria-hidden=\"true\"><path"), "scaffold icon from the committed file");
    assert!(r.notes.is_empty(), "nothing downloaded: {:?}", r.notes);

    page(&site, "icons.html", "<svg icon=\"Shield\"></svg>");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("not an icon name"), "{}", r.errors[0]);
    assert_eq!((r.errors[0].file.as_deref(), r.errors[0].line), (Some("src/pages/icons.html"), Some(4)));

    page(&site, "icons.html", "<i class=\"icon-[lucide--shield]\"></i>");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("Tailwind icon syntax") && r.errors[0].fix.as_deref().unwrap().contains("icon=\"lucide:shield\""), "{}", r.errors[0]);

    page(&site, "icons.html", "<svg icon=\"own:dot\"><path/></svg>");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("must be empty"), "{}", r.errors[0]);
}

#[test]
fn google_fonts_are_served_locally() {
    let site = scaffold("fonts");
    // Already localized (as after a first online build), so no network here.
    std::fs::create_dir_all(site.join("src/assets/fonts/inter")).unwrap();
    std::fs::write(site.join("src/assets/fonts/inter/inter-400-normal-latin.woff2"), b"wOF2").unwrap();
    std::fs::write(site.join("src/assets/fonts/inter.css"), "@font-face { font-family: 'Inter'; src: url(/fonts/inter/inter-400-normal-latin.woff2) format('woff2'); }\n").unwrap();
    page(&site, "fonts.html", "<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\n<link href=\"https://fonts.googleapis.com/css2?family=Inter:wght@400;700&display=swap\" rel=\"stylesheet\">");
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>());
    let html = text(&r, "fonts/index.html");
    assert!(!html.contains("google") && !html.contains("gstatic") && !html.contains("preconnect"), "{html}");
    let css_link = html.lines().find(|l| l.contains("/fonts/inter.")).expect("local stylesheet link");
    assert!(css_link.contains("<link href=\"/fonts/inter.") && css_link.ends_with(".css\" rel=\"stylesheet\">"), "{css_link}");
    let css = r.outputs.iter().find(|(k, _)| k.starts_with("fonts/inter.") && k.ends_with(".css")).map(|(_, v)| String::from_utf8(v.clone()).unwrap()).unwrap();
    assert!(css.contains("url(/fonts/inter/inter-400-normal-latin.") && css.contains(".woff2)"), "font file hashed too: {css}");

    std::fs::write(site.join("src/assets/site.css"), "@import url(https://fonts.googleapis.com/css2?family=Lora);\nbody { margin: 0 }\n").unwrap();
    let r = run_check(&site).unwrap();
    let got: Vec<String> = r.warnings.iter().map(|w| w.to_string()).collect();
    assert!(got.iter().any(|w| w == "src/assets/site.css: stylesheet imports fonts from Google"), "{got:?}");
}

/// Every ready-made component: copied into a fresh site by `add`, identical
/// to the library file, and the site still checks clean. The scaffold uses
/// the FAQ, so its output is checked here too.
#[test]
fn ready_made_components_build_clean() {
    let site = scaffold("library");
    for e in magehat::library::LIBRARY {
        let rel = format!("src/components/{}.html", e.name);
        let _ = std::fs::remove_file(site.join(&rel));
        let msg = magehat::library::add(&site, e.name).unwrap();
        assert!(msg.contains(&format!("Use it as <x-{}", e.name)), "{msg}");
        assert_eq!(std::fs::read_to_string(site.join(&rel)).unwrap(), e.file);
        assert!(magehat::library::add(&site, e.name).unwrap_err().message.contains("already exists"));
    }
    assert!(magehat::library::add(&site, "nope").unwrap_err().fix.as_deref().unwrap().contains("faq"));
    assert!(magehat::library::listing().contains("  faq  "));
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>());
    let about = text(&r, "about/index.html");
    assert!(about.contains("<details class=\"faq-item\" name=\"about\">") && about.contains("<span>Does the site need a server?</span>"), "{about}");
    assert!(about.contains("<x-faq group=\"about\" schema=\"faq\">"), "list props stay off the tag: {about}");
    assert!(about.contains("\"@type\":\"FAQPage\"") && about.contains("\"name\":\"Does the site need a server?\""), "{about}");
    assert!(about.contains("\"text\":\"magehat new page <name>, then"), "answers are JSON strings: {about}");
    assert!(about.contains("<div class=\"faq-answer\">magehat new page &lt;name&gt;, then"), "and escaped text on the page: {about}");
    let css = r.outputs.iter().find(|(k, _)| k.starts_with("_mh/x-faq.")).map(|(_, v)| String::from_utf8(v.clone()).unwrap()).unwrap();
    assert!(css.contains(".faq-item[open]::details-content{block-size:auto;block-size:calc-size(auto,size)}"), "{css}");

    // Items from a collection: title and body instead of question and answer,
    // ordered by `order`, answers as HTML, and no schema unless asked.
    std::fs::create_dir_all(site.join("src/content/help")).unwrap();
    std::fs::write(site.join("src/content/help/b.md"), "---\ntitle: Second?\norder: 2\n---\nWith a [link](/about/).\n").unwrap();
    std::fs::write(site.join("src/content/help/a.md"), "---\ntitle: First?\norder: 1\n---\nYes.\n").unwrap();
    page(&site, "help.html", "<x-faq items=\"{{ help }}\" group=\"\"></x-faq>");
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let help = text(&r, "help/index.html");
    assert!(!help.contains("FAQPage"), "{help}");
    assert!(help.find("First?").unwrap() < help.find("Second?").unwrap(), "order key: {help}");
    assert!(help.contains("<div class=\"faq-answer\"><p>With a <a href=\"/about/\">link</a>.</p>"), "{help}");
}

#[test]
fn or_writes_a_default() {
    let site = scaffold("or");
    page(&site, "d.html", "<p>{{ page.image or site.image }}|{{ nothing or 'x' }}|{{ site.name or 'y' }}</p>");
    let r = build_site(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(text(&r, "d/index.html").contains("<p>/photos/hat.jpg|x|My Site</p>"));
    assert!(text(&r, "index.html").contains("<meta property=\"og:image\" content=\"https://example.com/photos/hat.jpg\">"));
    page(&site, "d.html", "<p>{{ nothing }}</p>");
    assert!(build_site(&site).unwrap().errors[0].message.contains("undefined variable"), "still an error without `or`");
}

#[test]
fn noindex_pages_stay_out_of_the_sitemap() {
    let site = scaffold("noindex");
    std::fs::write(
        site.join("src/pages/thanks.html"),
        "<title>Thanks</title>\n<meta name=\"description\" content=\"d\">\n<meta name=\"robots\" content=\"noindex\">\n<x-base><p>ok</p></x-base>\n",
    )
    .unwrap();
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>());
    let thanks = text(&r, "thanks/index.html");
    assert!(thanks.contains("<meta name=\"robots\" content=\"noindex\">"), "{thanks}");
    assert_eq!(thanks.matches("name=\"robots\"").count(), 1);
    let sitemap = text(&r, "sitemap.xml");
    assert!(!sitemap.contains("/thanks/") && sitemap.contains("/about/"), "{sitemap}");
}

#[test]
fn anchors_and_social_images_are_checked() {
    let site = scaffold("anchors");
    page(
        &site,
        "a.html",
        "<h2 id=\"here\">x</h2><a href=\"#here\">ok</a><a href=\"#nope\">bad</a><a href=\"/about/#questions\">ok</a><a href=\"/about/#missing\">bad</a><a href=\"https://x.y/#z\">ext</a>",
    );
    let r = run_check(&site).unwrap();
    let got: Vec<String> = r.warnings.iter().map(|w| w.message.clone()).collect();
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[0].contains("\"#nope\"") && got[1].contains("\"/about/#missing\""), "{got:?}");

    let layout = std::fs::read_to_string(site.join("src/components/base.html")).unwrap();
    std::fs::write(site.join("src/components/base.html"), layout.replace("content=\"{{ site.url }}{{ page.image or site.image }}\"", "content=\"{{ page.image or site.image }}\"")).unwrap();
    page(&site, "a.html", "<p>x</p>");
    let r = run_check(&site).unwrap();
    let got: Vec<String> = r.warnings.iter().map(|w| w.to_string()).collect();
    assert!(got.iter().any(|w| w.starts_with("src/pages/index.html: og:image \"/photos/hat.") && w.ends_with("on / is not an absolute URL")), "{got:?}");
}

#[test]
fn asset_and_icon_folders_can_live_outside_src() {
    let site = scaffold("outside");
    let kit = site.join("kit");
    std::fs::create_dir_all(&kit).unwrap();
    std::fs::write(kit.join("logo.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"><rect width=\"1\" height=\"1\"/></svg>").unwrap();
    let toml = std::fs::read_to_string(site.join("site.toml")).unwrap();
    std::fs::write(site.join("site.toml"), format!("{toml}\n[assets]\nbrand = \"kit\"\n[icons]\nbrand = \"kit\"\n")).unwrap();
    page(&site, "b.html", "<svg icon=\"brand:nope\"></svg>");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("no icon named nope in the brand folder (kit)"), "{:?}", r.errors);
    assert!(r.notes.is_empty(), "a mapped set is never downloaded into");

    page(&site, "b.html", "<img src=\"/brand/logo.svg\" alt=\"logo\"><svg icon=\"brand:logo\" class=\"i\"></svg>");
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>());
    assert!(r.outputs.contains_key("brand/logo.svg"));
    let html = text(&r, "b/index.html");
    assert!(html.contains("<img src=\"/brand/logo.") && html.contains("viewBox=\"0 0 1 1\" class=\"i\"><rect"), "{html}");

    std::fs::write(site.join("site.toml"), format!("{toml}\n[assets]\nbrand = \"missing\"\n")).unwrap();
    let r = build_site(&site).unwrap();
    assert!(r.errors.iter().any(|e| e.message.contains("does not exist")), "{:?}", r.errors);
    std::fs::write(site.join("site.toml"), format!("{toml}\n[assets]\nBrand = \"kit\"\n")).unwrap();
    assert!(build_site(&site).err().expect("expected an error").message.contains("lowercase"));
}

#[test]
fn text_nodes_may_start_with_a_multibyte_character() {
    // The parser used to skip one *byte* when looking for the next '<' in a run
    // of text, which landed inside the first character and panicked whenever that
    // character was not ASCII. Any accented word opening a paragraph did it, so
    // every Portuguese, French or Japanese page was a build away from crashing.
    let site = scaffold("multibyte");
    page(&site, "acentos.html", "<p>\u{c9} importante.</p>\n<p>A\u{e7}\u{e3}o e cora\u{e7}\u{e3}o</p>\n<p>\u{65e5}\u{672c}\u{8a9e}</p>");
    let r = build_site(&site).unwrap();
    let out = text(&r, "acentos/index.html");
    assert!(out.contains("<p>\u{c9} importante.</p>"), "{out}");
    assert!(out.contains("A\u{e7}\u{e3}o e cora\u{e7}\u{e3}o"), "{out}");
    assert!(out.contains("\u{65e5}\u{672c}\u{8a9e}"), "{out}");
}

/// A list page is rendered once per page of items, an archive page once per
/// value of its field and page, and `limit` cuts a loop short.
#[test]
fn lists_archives_and_limit() {
    let site = news("news");
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>());

    // limit: the two newest only, from a number in site.toml
    let home = text(&r, "index.html");
    assert!(home.contains("second expansion") && home.contains("patch 1.20") && !home.contains("speedrun"), "{home}");
    // terms.posts.tags: by count, then name; accents folded in the slug
    let tags = &home[home.find("<ul class=\"tags\">").unwrap()..];
    assert!(tags.contains(concat!(
        "<li><a href=\"/tags/acao/\">Ação</a> (3)</li>",
        "<li><a href=\"/tags/elden-ring/\">Elden Ring</a> (3)</li>",
        "<li><a href=\"/tags/fromsoftware/\">FromSoftware</a> (1)</li>",
        "<li><a href=\"/tags/hollow-knight/\">Hollow Knight</a> (1)</li>"
    )), "{tags}");

    // five posts, two per page: three pages
    assert!(r.outputs.contains_key("posts/index.html") && r.outputs.contains_key("posts/page/2/index.html") && r.outputs.contains_key("posts/page/3/index.html"));
    assert!(!r.outputs.contains_key("posts/page/4/index.html") && !r.outputs.contains_key("posts/page/1/index.html"));
    let p1 = text(&r, "posts/index.html");
    assert!(p1.contains("<title>All posts, page 1 · News</title>"), "{p1}");
    assert!(p1.contains("<p class=\"count\">5 posts, page 1 of 3</p>"), "{p1}");
    assert!(p1.contains("second expansion") && p1.contains("patch 1.20") && !p1.contains("speedrun"), "{p1}");
    assert!(!p1.contains("rel=\"prev\"") && p1.contains("<a href=\"/posts/page/2/\" rel=\"next\">Older</a>"), "{p1}");
    assert!(p1.contains("<b>1</b><a href=\"/posts/page/2/\">2</a><a href=\"/posts/page/3/\">3</a>"), "{p1}");
    assert!(p1.contains("<link rel=\"canonical\" href=\"https://news.example/posts/\">"), "{p1}");
    let p3 = text(&r, "posts/page/3/index.html");
    assert!(p3.contains("no tags") && !p3.contains("Silksong"), "{p3}");
    assert!(p3.contains("<a href=\"/posts/page/2/\" rel=\"prev\">Newer</a>") && !p3.contains("rel=\"next\""), "{p3}");
    assert!(p3.contains("<link rel=\"canonical\" href=\"https://news.example/posts/page/3/\">"), "{p3}");

    // archive: one listing per tag, paginated the same way
    let elden = text(&r, "tags/elden-ring/index.html");
    assert!(elden.contains("<title>Elden Ring · News</title>") && elden.contains("<p class=\"count\">3 posts</p>"), "{elden}");
    assert!(elden.contains("second expansion") && elden.contains("patch 1.20") && !elden.contains("speedrun"), "{elden}");
    assert!(elden.contains("<a href=\"/tags/elden-ring/page/2/\" rel=\"next\">Older</a>"), "{elden}");
    assert!(text(&r, "tags/elden-ring/page/2/index.html").contains("speedrun"));
    assert!(text(&r, "tags/acao/index.html").contains("<h1>Ação</h1>"));
    assert!(!r.outputs.contains_key("tags/fromsoftware/page/2/index.html"));
    assert!(!r.outputs.keys().any(|k| k.starts_with("tags/index")), "no page for the archive itself");

    // an item links to its own terms
    let post = text(&r, "posts/elden-ring-dlc/index.html");
    assert!(post.contains("<a href=\"/tags/elden-ring/\">Elden Ring</a> <a href=\"/tags/fromsoftware/\">FromSoftware</a> <a href=\"/tags/acao/\">Ação</a>"), "{post}");
    assert!(text(&r, "posts/untagged/index.html").contains("<p class=\"tags\"></p>"));

    let sitemap = text(&r, "sitemap.xml");
    assert!(sitemap.contains("<loc>https://news.example/posts/page/2/</loc>") && sitemap.contains("<loc>https://news.example/tags/acao/</loc>"), "{sitemap}");

    let info = inspect_site(&site).unwrap();
    let pages = info["pages"].as_array().unwrap();
    let list = pages.iter().find(|p| p["id"] == "posts/index").unwrap();
    assert_eq!(list["kind"], "list");
    assert_eq!(list["urls"]["en"], serde_json::json!(["/posts/", "/posts/page/2/", "/posts/page/3/"]));
    let archive = pages.iter().find(|p| p["id"] == "tags/[tag]").unwrap();
    assert_eq!((archive["kind"].as_str(), archive["by"].as_str(), archive["term_var"].as_str()), (Some("archive"), Some("tags"), Some("tag")));
    assert_eq!(info["syntax"]["globals"].as_array().unwrap().iter().filter(|g| *g == "terms").count(), 1);
}

#[test]
fn list_declarations_are_checked() {
    let site = news("news-errors");
    let bad = |file: &str, content: &str| {
        std::fs::write(site.join("src/pages").join(file), content).unwrap();
        let e = build_site(&site).err().expect("expected an error");
        std::fs::remove_file(site.join("src/pages").join(file)).unwrap();
        e
    };
    let e = bad("by-alone.html", "<title>T</title>\n<meta name=\"description\" content=\"d\">\n<meta name=\"by\" content=\"tags\">\n<x-base></x-base>\n");
    assert!(e.message.contains("needs <meta name=\"list\">"), "{e}");
    let e = bad("no-bracket.html", "<title>T</title>\n<meta name=\"description\" content=\"d\">\n<meta name=\"list\" content=\"posts\">\n<meta name=\"by\" content=\"tags\">\n<x-base></x-base>\n");
    assert!(e.message.contains("bracket name") && e.fix.as_deref().unwrap().contains("[term].html"), "{e}");
    let e = bad("[x].html", "<title>T</title>\n<meta name=\"description\" content=\"d\">\n<meta name=\"list\" content=\"posts\">\n<x-base></x-base>\n");
    assert!(e.message.contains("needs <meta name=\"by\">"), "{e}");
    let e = bad("size.html", "<title>T</title>\n<meta name=\"description\" content=\"d\">\n<meta name=\"list\" content=\"posts\">\n<meta name=\"per-page\" content=\"many\">\n<x-base></x-base>\n");
    assert!(e.message.contains("per-page must be a whole number"), "{e}");

    std::fs::write(site.join("src/pages/nolist.html"), "<title>T</title>\n<meta name=\"description\" content=\"d\">\n<meta name=\"list\" content=\"nope\">\n<x-base></x-base>\n").unwrap();
    let r = build_site(&site).unwrap();
    assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
    assert!(r.errors[0].message.contains("no collection named \"nope\""), "{}", r.errors[0]);
    std::fs::remove_file(site.join("src/pages/nolist.html")).unwrap();

    page(&site, "limit.html", "<p limit=\"2\">x</p>");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("limit needs each"), "{}", r.errors[0]);
    page(&site, "limit.html", "<p each=\"p in posts\" limit=\"site.name\">x</p>");
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("limit must be a whole number"), "{}", r.errors[0]);
    std::fs::remove_file(site.join("src/pages/limit.html")).unwrap();

    // two spellings of one term would share a URL
    std::fs::write(site.join("src/content/posts/dup.md"), "---\ntitle: Dup\ndate: 2026-01-01\ntags: [elden ring]\n---\nx\n").unwrap();
    let r = build_site(&site).unwrap();
    assert!(r.errors[0].message.contains("would share the URL /elden-ring/"), "{}", r.errors[0]);
}

#[test]
fn indexnow_writes_a_key_file_derived_from_the_url() {
    let dir = scaffold("indexnow");
    let off = build_site(&dir).unwrap();
    assert!(!off.outputs.keys().any(|k| k.len() == 36 && k.ends_with(".txt")), "no key file unless asked for");

    let toml = std::fs::read_to_string(dir.join("site.toml")).unwrap();
    std::fs::write(dir.join("site.toml"), format!("indexnow = true
{toml}")).unwrap();
    let r = build_site(&dir).unwrap();
    assert!(r.ok());
    let key = magehat::indexnow::key_for(&r.cfg.url);
    assert_eq!(text(&r, &format!("{key}.txt")), key);
}

/// A news collection: a feed readers and Flipboard accept, a Google News
/// sitemap of the newest two days, and robots.txt naming it.
#[test]
fn news_collections_get_a_news_sitemap_and_a_full_feed() {
    let site = news("news-sitemap");
    let toml = std::fs::read_to_string(site.join("site.toml")).unwrap();
    std::fs::write(site.join("site.toml"), format!("{toml}
[collections.posts]
feed = true
news = true
feed_items = 3
")).unwrap();
    let post = site.join("src/content/posts/elden-ring-dlc.md");
    let body = std::fs::read_to_string(&post).unwrap();
    std::fs::write(&post, body.replacen("date: 2026-03-05", "date: 2026-03-05T14:30:00-03:00
author: Ana Souza
image: /covers/dlc.jpg", 1)).unwrap();
    let r = build_site(&site).unwrap();
    assert!(r.ok(), "{:?}", r.errors);

    // the newest is March 5; March 3 is within two days of it, March 2 is not
    let sitemap = text(&r, "news-sitemap.xml");
    assert!(sitemap.contains("<loc>https://news.example/posts/elden-ring-dlc/</loc>") && sitemap.contains("elden-ring-speedrun"), "{sitemap}");
    assert!(!sitemap.contains("hollow-knight") && !sitemap.contains("untagged"), "{sitemap}");
    assert!(sitemap.contains("<news:name>News</news:name><news:language>en</news:language>"), "{sitemap}");
    assert!(sitemap.contains("<news:publication_date>2026-03-05T14:30:00-03:00</news:publication_date>"), "{sitemap}");
    assert!(text(&r, "robots.txt").contains("Sitemap: https://news.example/news-sitemap.xml"));

    let feed = text(&r, "posts/feed.xml");
    assert_eq!(feed.matches("<item>").count(), 3, "feed_items caps the feed");
    assert!(feed.contains("<pubDate>Thu, 05 Mar 2026 14:30:00 -0300</pubDate>"), "{feed}");
    assert!(feed.contains("<dc:creator>Ana Souza</dc:creator>"), "{feed}");
    assert!(feed.contains("<enclosure url=\"https://news.example/covers/dlc.jpg\" length=\"0\" type=\"image/jpeg\"/>"), "{feed}");
    assert!(feed.contains("<media:content url=\"https://news.example/covers/dlc.jpg\" medium=\"image\" type=\"image/jpeg\"/>"), "{feed}");
    assert!(text(&r, "llms.txt").contains("- [posts feed](https://news.example/posts/feed.xml)"));

    // without news = true there is no news sitemap and robots.txt does not name one
    let plain = news("news-plain");
    let r = build_site(&plain).unwrap();
    assert!(!r.outputs.contains_key("news-sitemap.xml") && !text(&r, "robots.txt").contains("news-sitemap"));
}

/// The error a site fails to load with, before any page is built.
fn build_error(site: &Path) -> magehat::errors::MageError {
    match build_site(site) {
        Err(e) => e,
        Ok(_) => panic!("the site built, an error was expected"),
    }
}

/// A scaffold site with a media bucket and two records, an image and a video.
fn media_site(name: &str) -> PathBuf {
    let site = scaffold(name);
    let toml = std::fs::read_to_string(site.join("site.toml")).unwrap();
    std::fs::write(
        site.join("site.toml"),
        format!("{toml}\n[media]\nurl = \"https://media.example.com/\"\nbucket = \"example-bucket-media\"\naccount = \"0123456789abcdef0123456789abcdef\"\n"),
    )
    .unwrap();
    std::fs::create_dir_all(site.join("src/media")).unwrap();
    std::fs::write(
        site.join("src/media/cover.json"),
        r#"{"type":"image/jpeg","ext":"jpg","hash":"abcdef0123","size":10,"width":1600,"height":900,"widths":[400,800,1200,1600]}"#,
    )
    .unwrap();
    std::fs::write(site.join("src/media/trailer.json"), r#"{"type":"video/mp4","ext":"mp4","hash":"ff00ff00ff","size":10}"#).unwrap();
    site
}

#[test]
fn media_files_are_addressed_in_the_bucket_from_their_records() {
    let site = media_site("media");
    page(
        &site,
        "m.html",
        "<img src=\"media:cover\" alt=\"Cover\" width=\"800\">\n<a href=\"media:trailer\">trailer</a>\n<video src=\"media:trailer\" controls></video>\n<img src=\"media:cover\" alt=\"Small\" width=\"300\">",
    );
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let html = text(&r, "m/index.html");
    let m = "https://media.example.com/cover.abcdef0123";
    assert!(
        html.contains(&format!("<picture><source type=\"image/webp\" srcset=\"{m}.400.webp 400w, {m}.800.webp 800w, {m}.1200.webp 1200w, {m}.1600.webp 1600w\" sizes=\"(max-width: 800px) 100vw, 800px\">")),
        "{html}"
    );
    assert!(html.contains(&format!("<img src=\"{m}.800.jpg\" width=\"800\" height=\"450\" srcset=\"{m}.400.jpg 400w, {m}.800.jpg 800w, {m}.1200.jpg 1200w, {m}.1600.jpg 1600w\"")), "{html}");
    // At 300px the ladder's 400 covers both 1x and the only size that is not upscaled.
    assert!(html.contains(&format!("<img src=\"{m}.400.jpg\" width=\"300\" height=\"169\" loading=\"lazy\"")), "{html}");
    let t = "https://media.example.com/trailer.ff00ff00ff.mp4";
    assert!(html.contains(&format!("href=\"{t}\"")) && html.contains(&format!("<video src=\"{t}\"")), "{html}");
    assert!(!html.contains("media:"), "{html}");
    assert!(!r.outputs.keys().any(|k| k.starts_with("_mh/img/cover")), "nothing is encoded locally");
    assert_eq!(build_site(&site).unwrap().outputs, r.outputs, "deterministic");

    // The scaffold layout writes og:image as {{ site.url }}{{ page.image }}; a media cover survives that.
    std::fs::write(site.join("src/pages/og.html"), "<title>T</title>\n<meta name=\"description\" content=\"d\">\n<meta name=\"image\" content=\"media:cover\">\n<x-base><p>x</p></x-base>\n").unwrap();
    let r = run_check(&site).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let og = text(&r, "og/index.html");
    assert!(og.contains(&format!("property=\"og:image\" content=\"{m}.1600.jpg\"")), "{og}");
    assert!(!og.contains("media:"), "{og}");
}

#[test]
fn media_mistakes_fail_the_build_with_the_fix() {
    let site = media_site("media-missing");
    page(&site, "m.html", "<img src=\"media:nope\" alt=\"x\"><a href=\"media:also\">x</a><img src=\"media:trailer\" alt=\"v\">");
    let r = build_site(&site).unwrap();
    let msgs: Vec<String> = r.errors.iter().map(|e| e.to_string()).collect();
    assert_eq!(msgs.iter().filter(|m| m.contains("media:nope has no record") && m.contains("magehat media add <file> nope")).count(), 1, "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("media:also has no record")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("media:trailer is a video/mp4 file")), "{msgs:?}");
    assert!(msgs.iter().all(|m| m.contains("m.html")), "{msgs:?}");

    let bare = scaffold("media-no-table");
    page(&bare, "m.html", "<img src=\"media:cover\" alt=\"x\">");
    let r = build_site(&bare).unwrap();
    assert!(r.errors.iter().any(|e| e.message.contains("[media]") && e.fix.as_deref().unwrap_or_default().contains("site.toml")), "{:?}", r.errors);

    std::fs::write(site.join("src/media/bad.json"), "{nope").unwrap();
    let err = build_error(&site);
    assert!(err.to_string().contains("src/media/bad.json") && err.to_string().contains("magehat media add"), "{err}");
    std::fs::remove_file(site.join("src/media/bad.json")).unwrap();

    let toml = std::fs::read_to_string(site.join("site.toml")).unwrap();
    std::fs::write(site.join("site.toml"), toml.replace("bucket = \"example-bucket-media\"\n", "")).unwrap();
    let err = build_error(&site);
    assert!(err.to_string().contains("[media] needs bucket"), "{err}");
}
