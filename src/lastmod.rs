//! The sitemap's `<lastmod>`: the day a page's own content last changed.
//!
//! Google and Bing use lastmod to decide what to recrawl, but only while it
//! stays accurate: a date that moves on every build is learnt as noise and
//! ignored. So the date is never the build's. It comes from git, the one
//! record of when a file really changed that every machine and the deploy
//! workflow share: the last commit that touched the file, or the build's day
//! for a file with uncommitted changes (it is changing now). A page counts the
//! files its main content comes from (its page file, its item, the items a
//! list page shows, an archive term's record), not layouts and components: a new footer is not new
//! content, and Google says lastmod should mark significant changes only.
//!
//! Without git history (no repository, git not installed, or a shallow clone,
//! where every file would carry the one commit's date) there is no lastmod
//! beyond an item's own `date`: no date beats a wrong one. The deploy workflow
//! checks out the full history for this.

use crate::clock::{civil_from_days, Clock};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

pub struct Revisions {
    /// Path relative to the site root, with forward slashes -> YYYY-MM-DD.
    committed: HashMap<String, String>,
    /// Files changed, added or untracked in the working tree.
    dirty: HashSet<String>,
    today: String,
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
}

impl Revisions {
    /// The site's history, or None when it has none worth reading.
    pub fn load(root: &Path, clock: &Clock) -> Option<Revisions> {
        if git(root, &["rev-parse", "--is-shallow-repository"])?.trim() != "false" {
            return None;
        }
        let prefix = git(root, &["rev-parse", "--show-prefix"])?.trim().to_string();
        // Newest first, so the first date seen for a file is its last change.
        // %cs is the committer date: a rebased or amended commit counts from
        // when it landed, like a deploy would.
        let log = git(root, &["log", "--relative", "--no-renames", "--name-only", "--format=%x00%cs", "--", "."])?;
        Some(Revisions::parse(&log, &git(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."])?, &prefix, clock))
    }

    fn parse(log: &str, status: &str, prefix: &str, clock: &Clock) -> Revisions {
        let mut committed = HashMap::new();
        let mut date = String::new();
        for line in log.lines() {
            if let Some(d) = line.strip_prefix('\0') {
                date = d.trim().to_string();
            } else if !line.trim().is_empty() && !date.is_empty() {
                committed.entry(norm(line.trim())).or_insert_with(|| date.clone());
            }
        }
        // Porcelain paths are relative to the repository root; a rename's
        // second entry (its old path) needs no date.
        let mut dirty = HashSet::new();
        let mut entries = status.split('\0');
        while let Some(entry) = entries.next() {
            if entry.len() < 4 {
                continue;
            }
            let path = norm(&entry[3..]);
            dirty.insert(path.strip_prefix(prefix).unwrap_or(&path).to_string());
            if entry.starts_with('R') || entry.starts_with('C') {
                entries.next();
            }
        }
        let (y, m, d) = civil_from_days(clock.today());
        Revisions { committed, dirty, today: format!("{y:04}-{m:02}-{d:02}") }
    }

    /// The day `file` (relative to the site root) last changed.
    pub fn date(&self, file: &str) -> Option<String> {
        let file = norm(file);
        if self.dirty.contains(&file) {
            return Some(self.today.clone());
        }
        self.committed.get(&file).cloned()
    }
}

/// The later of two dates, compared by day; the one kept is written as it
/// came, so an item's own `date` with a time keeps it.
pub fn latest(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.get(..10).unwrap_or(&b) > a.get(..10).unwrap_or(&a) { b } else { a }),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> Clock {
        Clock::at("2026-10-17").unwrap()
    }

    #[test]
    fn a_file_takes_its_newest_commit_and_a_dirty_file_today() {
        let log = "\0 2026-10-12\nsrc/pages/a.html\nsrc/content/blog/x.md\n\n\0 2026-09-01\nsrc/pages/a.html\nsrc/pages/b.html\n";
        let status = " M sites/s/src/pages/b.html\0?? sites/s/src/content/blog/new.md\0R  sites/s/src/pages/c.html\0sites/s/src/pages/old.html\0";
        let r = Revisions::parse(log, status, "sites/s/", &clock());
        assert_eq!(r.date("src/pages/a.html").as_deref(), Some("2026-10-12"), "newest commit wins");
        assert_eq!(r.date("src\\content\\blog\\x.md").as_deref(), Some("2026-10-12"), "Windows separators");
        assert_eq!(r.date("src/pages/b.html").as_deref(), Some("2026-10-17"), "modified now");
        assert_eq!(r.date("src/content/blog/new.md").as_deref(), Some("2026-10-17"), "untracked");
        assert_eq!(r.date("src/pages/c.html").as_deref(), Some("2026-10-17"), "renamed");
        assert_eq!(r.date("src/pages/old.html"), None, "a rename's old path is not a file");
        assert_eq!(r.date("src/pages/none.html"), None);
    }

    #[test]
    fn latest_compares_by_day_and_keeps_what_was_written() {
        assert_eq!(latest(Some("2026-01-15T09:00".into()), Some("2026-01-15".into())).as_deref(), Some("2026-01-15T09:00"));
        assert_eq!(latest(Some("2026-01-15".into()), Some("2026-02-01".into())).as_deref(), Some("2026-02-01"));
        assert_eq!(latest(None, Some("2026-02-01".into())).as_deref(), Some("2026-02-01"));
        assert_eq!(latest(None, None), None);
    }
}
