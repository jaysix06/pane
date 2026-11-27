//! Finding files by name across a whole drive, from the master file table
//! the helper service reads: the query, how names match it, and what comes
//! back over the pipe. Pane's folder search (pane-app's search.rs) matches
//! with the same [`Matcher`], so a drive searched either way agrees.

use serde::{Deserialize, Serialize};

/// A query, ready to test names against. Names containing it match,
/// ignoring case; with `*` or `?` it is a wildcard for the whole name.
pub struct Matcher {
    text: String,
    wildcard: bool,
    case_sensitive: bool,
    whole_name: bool,
}

impl Matcher {
    pub fn new(query: &str, case_sensitive: bool, whole_name: bool) -> Matcher {
        let query = query.trim();
        let text = if case_sensitive { query.to_string() } else { query.to_lowercase() };
        Matcher { wildcard: text.contains(['*', '?']), text, case_sensitive, whole_name }
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
pub fn wildcard(pattern: &str, name: &str) -> bool {
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

/// `{"find": {...}}`: names on `volume` ("C:") matching `query`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindRequest {
    pub volume: String,
    pub query: String,
    pub case_sensitive: bool,
    pub whole_name: bool,
    /// Hidden and system items, and what is inside hidden folders.
    pub hidden: bool,
    /// Folder levels below the drive's root (0: the root only).
    pub max_depth: u32,
    /// At most this many results.
    pub limit: u32,
}

/// The longest query accepted, in characters.
pub const MAX_QUERY: usize = 200;
/// The most results one request may ask for.
pub const MAX_LIMIT: u32 = 20_000;

impl FindRequest {
    pub fn valid(&self) -> Result<(), String> {
        let b = self.volume.as_bytes();
        if !(b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':') {
            return Err("bad volume".into());
        }
        if self.query.trim().is_empty() || self.query.chars().count() > MAX_QUERY {
            return Err("bad query".into());
        }
        if self.limit == 0 || self.limit > MAX_LIMIT {
            return Err("bad limit".into());
        }
        Ok(())
    }
}

/// One match: its path below the drive's root ("Users\me\a.txt").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub path: String,
    pub dir: bool,
    pub size: u64,
    /// FILETIME (0 if unknown).
    pub modified: u64,
    pub hidden: bool,
    pub system: bool,
}

/// What the service sends for a find, one JSON line each: batches of
/// matches, then `done` (or `error`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindReply {
    Found(Vec<Found>),
    Done {
        /// Matches sent.
        matched: u64,
        took_ms: u64,
        /// The table was read for this request (else kept from one before:
        /// changes since then aren't in it).
        fresh: bool,
    },
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching() {
        let m = Matcher::new("Report", false, false);
        assert!(m.matches("2026 report.pdf"));
        assert!(!m.matches("notes.txt"));
        let m = Matcher::new("*.RS", false, false);
        assert!(m.matches("main.rs") && !m.matches("main.rs.bak"));
        let m = Matcher::new("a?c", false, false);
        assert!(m.matches("abc") && !m.matches("abbc"));
        let m = Matcher::new("Main", true, false);
        assert!(m.matches("Main.rs") && !m.matches("main.rs"));
        let m = Matcher::new("main.rs", false, true);
        assert!(m.matches("MAIN.RS") && !m.matches("main.rs.bak"));
        assert!(!Matcher::new("  ", false, false).matches("anything"));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("*", ""));
        assert!(wildcard("a*b*c", "a-x-b-y-c"));
        assert!(!wildcard("a*b", "a-x-c"));
        assert!(wildcard("*.tar.gz", "x.tar.gz"));
    }

    #[test]
    fn requests_are_checked() {
        let ok = FindRequest {
            volume: "C:".into(),
            query: "report".into(),
            case_sensitive: false,
            whole_name: false,
            hidden: false,
            max_depth: 32,
            limit: 1000,
        };
        assert!(ok.valid().is_ok());
        assert!(FindRequest { volume: "C:\\Windows".into(), ..ok.clone() }.valid().is_err());
        assert!(FindRequest { query: " ".into(), ..ok.clone() }.valid().is_err());
        assert!(FindRequest { query: "x".repeat(MAX_QUERY + 1), ..ok.clone() }.valid().is_err());
        assert!(FindRequest { limit: MAX_LIMIT + 1, ..ok.clone() }.valid().is_err());
    }
}
