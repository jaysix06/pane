//! Searching a folder and its subfolders by name, on a background thread.
//! Matches stream back in batches, so the first results show while the
//! search goes on, and a search stops at once when cancelled (a new search,
//! leaving the results, closing the tab).
//!
//! A query matches names containing it, ignoring case; with `*` or `?` it
//! is a wildcard pattern for the whole name (`*.rs`, `report-??.pdf`).
//! The Search settings choose case-sensitive and whole-name matching, hidden
//! files, and how deep to go.
//!
//! A search from a drive's root (C:\) asks the helper service first, which
//! finds names in the drive's master file table at once; without it (not
//! installed, not NTFS) the folders are walked as anywhere else.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use pane_fs::Entry;
use pane_scan::find::FindRequest;

#[derive(Clone, Debug)]
pub struct Options {
    pub case_sensitive: bool,
    pub whole_name: bool,
    pub hidden: bool,
    /// Folder levels below the one searched (0: that folder only).
    pub max_depth: u32,
    /// Ask the helper service when searching a whole drive.
    pub instant_drives: bool,
}

/// A query, ready to test names against (shared with the helper service,
/// so a drive searched either way finds the same).
pub struct Matcher(pane_scan::find::Matcher);

impl Matcher {
    pub fn new(query: &str, opts: &Options) -> Matcher {
        Matcher(pane_scan::find::Matcher::new(query, opts.case_sensitive, opts.whole_name))
    }

    pub fn matches(&self, name: &str) -> bool {
        self.0.matches(name)
    }
}

/// Most results a search of a whole drive through the service shows.
const DRIVE_LIMIT: u32 = 5000;

/// "C:" when `root` is a drive's root (C:\), else None.
pub fn drive_root(root: &Path) -> Option<String> {
    let s = root.to_str()?;
    let b = s.as_bytes();
    let is_root = (b.len() == 2 || (b.len() == 3 && (b[2] == b'\\' || b[2] == b'/'))) && b[0].is_ascii_alphabetic() && b[1] == b':';
    is_root.then(|| s[..2].to_ascii_uppercase())
}

/// The drive search through the service: true if it ran (results sent),
/// false to search the usual way instead.
fn run_instant(volume: &str, query: &str, opts: &Options, cancel: &AtomicBool, send: &mut impl FnMut(Event) -> bool) -> bool {
    let request = FindRequest {
        volume: volume.to_string(),
        query: query.trim().to_string(),
        case_sensitive: opts.case_sensitive,
        whole_name: opts.whole_name,
        hidden: opts.hidden,
        max_depth: opts.max_depth,
        limit: DRIVE_LIMIT,
    };
    if request.valid().is_err() {
        return false;
    }
    let mut stopped = false;
    let mut on_batch = |batch: Vec<pane_scan::find::Found>| {
        let entries: Vec<Entry> = batch
            .into_iter()
            .map(|f| Entry {
                name: f.path,
                is_dir: f.dir,
                size: f.size,
                modified: f.modified,
                hidden: f.hidden,
                system: f.system,
            })
            .collect();
        if !send(Event::Found(entries)) {
            stopped = true;
            return false;
        }
        true
    };
    match pane_scan::client::find(&request, &mut on_batch, cancel) {
        Some(end) => {
            if !stopped {
                eprintln!(
                    "[pane] instant search of {volume} for {query:?}: {} matches, {} ms{}",
                    end.matched,
                    end.took_ms,
                    if end.fresh { " (table read)" } else { "" }
                );
                send(Event::Done { folders: 0 });
            }
            true
        }
        None => false,
    }
}

/// A search's progress, as it goes.
pub enum Event {
    /// More matches: each entry's `name` is its path below the root
    /// (`sub\file.txt`), so `root.join(name)` is where it is.
    Found(Vec<Entry>),
    /// Finished (or cancelled), after looking in this many folders.
    Done { folders: usize },
}

/// Searches `root` for `query`, sending matches to `send` about every
/// 100 ms. Blocks: run it on a thread. Stops when `cancel` is set or
/// `send` returns false.
pub fn run(root: &Path, query: &str, opts: &Options, cancel: &AtomicBool, mut send: impl FnMut(Event) -> bool) {
    if opts.instant_drives
        && let Some(volume) = drive_root(root)
        && run_instant(&volume, query, opts, cancel, &mut send)
    {
        return;
    }
    let matcher = Matcher::new(query, opts);
    // Breadth first: nearby matches come first.
    let mut queue = std::collections::VecDeque::from([(String::new(), 0u32)]);
    let mut batch = Vec::new();
    let mut last_sent = Instant::now();
    let mut folders = 0;
    while let Some((rel, depth)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        folders += 1;
        let Ok(entries) = pane_fs::list_dir(&root.join(&rel)) else { continue };
        for mut e in entries {
            if !opts.hidden && (e.hidden || e.system) {
                continue;
            }
            let path = if rel.is_empty() { e.name.clone() } else { format!("{rel}\\{}", e.name) };
            if e.is_dir && depth < opts.max_depth {
                queue.push_back((path.clone(), depth + 1));
            }
            if matcher.matches(&e.name) {
                e.name = path;
                batch.push(e);
            }
        }
        if !batch.is_empty() && last_sent.elapsed() >= Duration::from_millis(100) {
            if !send(Event::Found(std::mem::take(&mut batch))) {
                return;
            }
            last_sent = Instant::now();
        }
    }
    if !batch.is_empty() && !send(Event::Found(batch)) {
        return;
    }
    send(Event::Done { folders });
}

/// A cancel flag shared with the search thread.
pub type Cancel = Arc<AtomicBool>;

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        Options { case_sensitive: false, whole_name: false, hidden: false, max_depth: 32, instant_drives: false }
    }

    #[test]
    fn matching() {
        let m = Matcher::new("Report", &opts());
        assert!(m.matches("2026 report.pdf"));
        assert!(!m.matches("notes.txt"));
        let m = Matcher::new("*.RS", &opts());
        assert!(m.matches("main.rs") && !m.matches("main.rs.bak"));
        let m = Matcher::new("a?c", &opts());
        assert!(m.matches("abc") && !m.matches("abbc"));
        let m = Matcher::new("Main", &Options { case_sensitive: true, ..opts() });
        assert!(m.matches("Main.rs") && !m.matches("main.rs"));
        let m = Matcher::new("main.rs", &Options { whole_name: true, ..opts() });
        assert!(m.matches("MAIN.RS") && !m.matches("main.rs.bak"));
        assert!(!Matcher::new("  ", &opts()).matches("anything"));
    }

    #[test]
    fn drive_roots() {
        assert_eq!(drive_root(Path::new(r"C:\")).as_deref(), Some("C:"));
        assert_eq!(drive_root(Path::new("d:")).as_deref(), Some("D:"));
        assert_eq!(drive_root(Path::new(r"C:\Users")), None);
        assert_eq!(drive_root(Path::new(r"\\server\share")), None);
    }

    #[test]
    fn finds_in_subfolders_up_to_the_depth() {
        let root = std::env::temp_dir().join(format!("pane-search-test-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a\\b")).unwrap();
        for f in ["top.txt", "a\\mid.txt", "a\\b\\deep.txt", "a\\b\\other.md"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        let find = |max_depth| {
            let mut names = Vec::new();
            let mut done = false;
            run(&root, ".txt", &Options { max_depth, ..opts() }, &AtomicBool::new(false), |e| {
                match e {
                    Event::Found(batch) => names.extend(batch.into_iter().map(|e| e.name)),
                    Event::Done { .. } => done = true,
                }
                true
            });
            assert!(done);
            names.sort();
            names
        };
        assert_eq!(find(32), ["a\\b\\deep.txt", "a\\mid.txt", "top.txt"]);
        assert_eq!(find(0), ["top.txt"]);
        let mut events = 0;
        run(&root, ".txt", &opts(), &AtomicBool::new(true), |_| {
            events += 1;
            true
        });
        assert_eq!(events, 1, "cancelled: only Done");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
