//! `magehat dev`: build, serve dist/, rebuild when src changes, reload the
//! browser. A polling watcher and a small HTTP server; nothing to configure.

use crate::build::{build_site, write_outputs};
use crate::check::format_report;
use crate::components::walk_files;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tiny_http::{Header, Response, Server, StatusCode};

const RELOAD_SCRIPT: &str = r#"<script>
(async () => {
  let seen = null;
  for (;;) {
    try {
      const v = await (await fetch('/_mh/version', {cache: 'no-store'})).text();
      if (seen !== null && v !== seen) location.reload();
      seen = v;
    } catch (e) {}
    await new Promise(r => setTimeout(r, 700));
  }
})();
</script>
"#;

struct State {
    root: PathBuf,
    version: AtomicU64,
    build_lock: Mutex<()>,
}

impl State {
    fn rebuild(&self) {
        let _guard = self.build_lock.lock().unwrap();
        let started = Instant::now();
        let report = match build_site(&self.root) {
            Ok(result) => match write_outputs(&result, &result.cfg.dist()) {
                Ok(()) => format_report(&result),
                Err(e) => format!("error: {e}"),
            },
            Err(e) => format!("error: {e}"),
        };
        self.version.fetch_add(1, Ordering::SeqCst);
        let mut lines: Vec<&str> = report.lines().collect();
        let summary = lines.pop().unwrap_or("");
        println!("built in {:.0?}: {summary}", started.elapsed());
        for line in lines {
            println!("  {line}");
        }
    }
}

fn snapshot(root: &Path) -> BTreeMap<String, SystemTime> {
    let mut snap = BTreeMap::new();
    let site = root.join("site.toml");
    if let Ok(m) = std::fs::metadata(&site) {
        snap.insert("site.toml".into(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH));
    }
    for (path, rel) in walk_files(root, &root.join("src")) {
        if let Ok(m) = std::fs::metadata(&path) {
            snap.insert(rel, m.modified().unwrap_or(SystemTime::UNIX_EPOCH));
        }
    }
    snap
}

fn watch(state: Arc<State>) {
    let mut last = snapshot(&state.root);
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let now = snapshot(&state.root);
        if now != last {
            last = now;
            state.rebuild();
        }
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn respond(request: tiny_http::Request, status: u16, ctype: &str, body: Vec<u8>) {
    let response = Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", ctype))
        .with_header(header("Cache-Control", "no-store"));
    let _ = request.respond(response);
}

fn handle(request: tiny_http::Request, dist: &Path, state: &State) {
    let url = request.url().to_string();
    let path = url.split(['?', '#']).next().unwrap_or("/").to_string();
    if path == "/_mh/version" {
        respond(request, 200, "text/plain", state.version.load(Ordering::SeqCst).to_string().into_bytes());
        return;
    }
    // Keep the request inside dist/.
    let clean: Vec<&str> = path.split('/').filter(|s| !s.is_empty() && *s != ".." && *s != ".").collect();
    let mut file = dist.to_path_buf();
    for seg in &clean {
        file.push(seg);
    }
    if file.is_dir() {
        if !path.ends_with('/') {
            let response = Response::empty(StatusCode(302)).with_header(header("Location", &format!("{path}/")));
            let _ = request.respond(response);
            return;
        }
        file.push("index.html");
    }
    if file.is_file() {
        let mut body = std::fs::read(&file).unwrap_or_default();
        let ctype = content_type(&file);
        if ctype.starts_with("text/html") {
            let text = String::from_utf8_lossy(&body).to_string();
            let injected = match text.to_lowercase().rfind("</body>") {
                Some(i) => format!("{}{RELOAD_SCRIPT}{}", &text[..i], &text[i..]),
                None => format!("{text}{RELOAD_SCRIPT}"),
            };
            body = injected.into_bytes();
        }
        respond(request, 200, ctype, body);
        return;
    }
    let not_found = dist.join("404.html");
    match std::fs::read(&not_found) {
        Ok(body) => respond(request, 404, "text/html; charset=utf-8", body),
        Err(_) => respond(request, 404, "text/plain; charset=utf-8", b"404 Not Found".to_vec()),
    }
}

pub fn serve(root: &Path, port: u16) -> crate::errors::Result<()> {
    let state = Arc::new(State { root: root.to_path_buf(), version: AtomicU64::new(0), build_lock: Mutex::new(()) });
    state.rebuild();
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist)?;
    let server = Server::http(("127.0.0.1", port)).map_err(|e| crate::errors::MageError::new(format!("cannot listen on port {port}: {e}")).fix("pick another port with --port"))?;
    let watcher = Arc::clone(&state);
    std::thread::spawn(move || watch(watcher));
    println!("Serving http://localhost:{port}/  (watching src/, Ctrl+C to stop)");
    for request in server.incoming_requests() {
        handle(request, &dist, &state);
    }
    Ok(())
}
