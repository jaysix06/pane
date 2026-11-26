//! Searching a folder and its subfolders by name, on a background thread.
//! Matches stream back in batches, so the first results show while the
//! search goes on, and a search stops at once when cancelled (a new search,
//! leaving the results, closing the tab).
//!
//! A query matches names containing it, ignoring case; with `*` or `?` it
//! is a wildcard pattern for the whole name (`*.rs`, `report-??.pdf`).
//! The Search settings choose case-sensitive and whole-name matching, hidden
//! files, and how deep to go.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use pane_fs::Entry;

#[derive(Clone, Debug)]
pub struct Options {
    pub case_sensitive: bool,
    pub whole_name: bool,
    pub hidden: bool,
    /// Folder levels below the one searched (0: that folder only).
    pub max_depth: u32,
}

/// A query, ready to test names against.
pub struct Matcher {
    text: String,
    wildcard: bool,
    case_sensitive: bool,
    whole_name: bool,
}

impl Matcher {
    pub fn new(query: &str, opts: &Options) -> Matcher {
        let query = query.trim();
        let text = if opts.case_sensitive { query.to_string() } else { query.to_lowercase() };
        Matcher {
            wildcard: text.contains(['*', '?']),
            text,
            case_sensitive: opts.case_sensitive,
            whole_name: opts.whole_name,
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        if self.text.is_empty() {
            return false;
        }
        let lowered;
        let name = if self.case_sensitive {
            name
        } else {
            lowered = name.to_lowercase();
            &lowered
        };
        if self.wildcard {
            wildcard(&self.text, name)
        } else if self.whole_name {
            name == self.text
        } else {
            name.contains(&self.text)
        }
    }
}

/// `*` (any run, also none) and `?` (one character), over the whole name.
fn wildcard(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ni;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
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
        Options { case_sensitive: false, whole_name: false, hidden: false, max_depth: 32 }
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
    fn wildcards() {
        assert!(wildcard("*", ""));
        assert!(wildcard("a*b*c", "a-x-b-y-c"));
        assert!(!wildcard("a*b", "a-x-c"));
        assert!(wildcard("*.tar.gz", "x.tar.gz"));
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
