//! Where a tab is, and how it got there: locations, back/forward history,
//! breadcrumbs and typed addresses. Pure logic, tested without a window.

use std::path::{Component, Path, PathBuf};

pub const THIS_PC: &str = "::thispc";
pub const OVERVIEW: &str = "::overview";
/// "::search|<root>|<query>" (a path never holds "|").
const SEARCH: &str = "::search|";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// The Storage Overview: Pane's home.
    Overview,
    /// The drives, as Explorer's "This PC".
    ThisPc,
    Folder(PathBuf),
    /// Search results: what's under `root` whose name matches `query`.
    Search { root: PathBuf, query: String },
}

impl Location {
    /// From the UI's string form: a path, or `THIS_PC`.
    pub fn from_ui(s: &str) -> Location {
        match s {
            THIS_PC => Location::ThisPc,
            OVERVIEW => Location::Overview,
            _ => match s.strip_prefix(SEARCH).and_then(|rest| rest.split_once('|')) {
                Some((root, query)) => Location::Search { root: PathBuf::from(root), query: query.into() },
                None => Location::Folder(PathBuf::from(s)),
            },
        }
    }

    /// The string the UI passes back to `from_ui`.
    pub fn to_ui(&self) -> String {
        match self {
            Location::ThisPc => THIS_PC.into(),
            Location::Overview => OVERVIEW.into(),
            Location::Folder(p) => p.display().to_string(),
            Location::Search { root, query } => format!("{SEARCH}{}|{query}", root.display()),
        }
    }

    /// The folder this location shows or searches.
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Location::Folder(p) | Location::Search { root: p, .. } => Some(p),
            Location::ThisPc | Location::Overview => None,
        }
    }

    /// What a tab says: the folder's name, or the drive ("Local Disk (C:)" is
    /// the sidebar's job; a tab says "C:").
    pub fn title(&self) -> String {
        match self {
            Location::ThisPc => "This PC".into(),
            Location::Overview => "Overview".into(),
            Location::Folder(p) => match p.file_name() {
                Some(name) => name.to_string_lossy().into_owned(),
                None => p.display().to_string().trim_end_matches('\\').to_string(),
            },
            Location::Search { query, .. } => format!("Search: {query}"),
        }
    }

    /// What the address bar shows while editing.
    pub fn address(&self) -> String {
        match self {
            Location::ThisPc => "This PC".into(),
            Location::Overview => "Overview".into(),
            Location::Folder(p) | Location::Search { root: p, .. } => p.display().to_string(),
        }
    }

    /// One level up: a drive's root goes to This PC.
    pub fn parent(&self) -> Option<Location> {
        match self {
            Location::ThisPc | Location::Overview => None,
            // Up from search results: the folder searched.
            Location::Search { root, .. } => Some(Location::Folder(root.clone())),
            Location::Folder(p) => Some(match p.parent() {
                Some(parent) => Location::Folder(parent.to_path_buf()),
                None => Location::ThisPc,
            }),
        }
    }

    /// Breadcrumbs: ("This PC", ThisPc), ("C:", C:\), ("Users", C:\Users)...
    pub fn crumbs(&self) -> Vec<(String, Location)> {
        if *self == Location::Overview {
            return vec![("Overview".to_string(), Location::Overview)];
        }
        let mut out = vec![("This PC".to_string(), Location::ThisPc)];
        let path = match self {
            Location::Folder(p) => p,
            Location::Search { root, query } => {
                let mut crumbs = Location::Folder(root.clone()).crumbs();
                crumbs.push((format!("Search “{query}”"), self.clone()));
                return crumbs;
            }
            _ => return out,
        };
        let mut acc = PathBuf::new();
        for comp in path.components() {
            acc.push(comp.as_os_str());
            match comp {
                Component::Prefix(p) => {
                    out.push((p.as_os_str().to_string_lossy().into_owned(), Location::Folder(acc.clone())))
                }
                // Fold the root into the drive: "C:\" shows as "C:".
                Component::RootDir => {
                    if let Some((_, Location::Folder(last))) = out.last_mut() {
                        *last = acc.clone();
                    }
                }
                other => out.push((other.as_os_str().to_string_lossy().into_owned(), Location::Folder(acc.clone()))),
            }
        }
        out
    }
}

/// A tab's back/forward history.
#[derive(Clone, Debug)]
pub struct History {
    pub current: Location,
    back: Vec<Location>,
    forward: Vec<Location>,
}

impl History {
    pub fn new(start: Location) -> Self {
        History { current: start, back: Vec::new(), forward: Vec::new() }
    }

    /// Goes somewhere new; returns false if it is where we are.
    pub fn go(&mut self, to: Location) -> bool {
        if to == self.current {
            return false;
        }
        let from = std::mem::replace(&mut self.current, to);
        self.back.push(from);
        self.forward.clear();
        true
    }

    pub fn back(&mut self) -> bool {
        let Some(prev) = self.back.pop() else { return false };
        let from = std::mem::replace(&mut self.current, prev);
        self.forward.push(from);
        true
    }

    pub fn forward(&mut self) -> bool {
        let Some(next) = self.forward.pop() else { return false };
        let from = std::mem::replace(&mut self.current, next);
        self.back.push(from);
        true
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }
    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}

/// Reads a typed address: a path (quotes and `%VARS%` allowed), "~" for
/// home, or "This PC". Relative paths are taken from `current`.
pub fn parse_address(text: &str, current: &Location, home: &Path) -> Option<Location> {
    let text = text.trim().trim_matches('"').trim();
    if text.is_empty() {
        return None;
    }
    if text.eq_ignore_ascii_case("this pc") || text.eq_ignore_ascii_case("computer") {
        return Some(Location::ThisPc);
    }
    if text.eq_ignore_ascii_case("overview") || text.eq_ignore_ascii_case("storage") {
        return Some(Location::Overview);
    }
    let expanded = expand_env(text);
    let mut path = if expanded == "~" {
        home.to_path_buf()
    } else if let Some(rest) = expanded.strip_prefix("~\\").or_else(|| expanded.strip_prefix("~/")) {
        home.join(rest)
    } else {
        PathBuf::from(&expanded)
    };
    // "C:" alone means the drive's root, not its current directory.
    if expanded.len() == 2 && expanded.ends_with(':') {
        path = PathBuf::from(format!("{expanded}\\"));
    }
    if path.is_relative() {
        let Location::Folder(base) = current else { return None };
        path = base.join(path);
    }
    Some(Location::Folder(normalize(&path)))
}

/// `%NAME%` replaced by the environment variable, left as typed if unset.
fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Resolves "." and ".." without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.file_name().is_some() {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(s: &str) -> Location {
        Location::Folder(PathBuf::from(s))
    }

    #[test]
    fn history_goes_back_and_forward() {
        let mut h = History::new(folder("C:\\a"));
        assert!(h.go(folder("C:\\b")));
        assert!(!h.go(folder("C:\\b")), "same place is not a step");
        assert!(h.go(folder("C:\\c")));
        assert!(h.back());
        assert_eq!(h.current, folder("C:\\b"));
        assert!(h.can_forward());
        assert!(h.go(folder("C:\\d")), "going somewhere new");
        assert!(!h.can_forward(), "clears forward");
        assert!(h.back() && h.back());
        assert_eq!(h.current, folder("C:\\a"));
        assert!(!h.back());
    }

    #[test]
    fn parent_of_a_drive_is_this_pc() {
        assert_eq!(folder("C:\\Users").parent(), Some(folder("C:\\")));
        assert_eq!(folder("C:\\").parent(), Some(Location::ThisPc));
        assert_eq!(Location::ThisPc.parent(), None);
        assert_eq!(Location::Overview.parent(), None);
    }

    #[test]
    fn crumbs_fold_the_root_into_the_drive() {
        let c: Vec<_> = folder("C:\\Users\\me").crumbs().into_iter().map(|(l, p)| (l, p.to_ui())).collect();
        assert_eq!(
            c,
            [
                ("This PC".to_string(), THIS_PC.to_string()),
                ("C:".to_string(), "C:\\".to_string()),
                ("Users".to_string(), "C:\\Users".to_string()),
                ("me".to_string(), "C:\\Users\\me".to_string()),
            ]
        );
        assert_eq!(Location::ThisPc.crumbs().len(), 1);
    }

    #[test]
    fn titles() {
        assert_eq!(folder("C:\\Users\\me").title(), "me");
        assert_eq!(folder("C:\\").title(), "C:");
        assert_eq!(Location::ThisPc.title(), "This PC");
        assert_eq!(Location::Overview.title(), "Overview");
        assert_eq!(Location::from_ui(OVERVIEW), Location::Overview);
        assert_eq!(Location::Overview.crumbs().len(), 1);
    }

    #[test]
    fn searches_round_trip_and_go_up_to_their_folder() {
        let s = Location::Search { root: PathBuf::from(r"C:\Work"), query: "a|b *.rs".into() };
        assert_eq!(Location::from_ui(&s.to_ui()), s);
        assert_eq!(s.parent(), Some(folder(r"C:\Work")));
        assert_eq!(s.folder(), Some(Path::new(r"C:\Work")));
        assert_eq!(s.title(), "Search: a|b *.rs");
        let crumbs = s.crumbs();
        assert_eq!(crumbs.last().unwrap().1, s);
        assert_eq!(crumbs[crumbs.len() - 2].1, folder(r"C:\Work"));
    }

    #[test]
    fn addresses_parse() {
        let home = Path::new("C:\\Users\\me");
        let here = folder("C:\\Work");
        assert_eq!(parse_address("  \"C:\\Temp\"  ", &here, home), Some(folder("C:\\Temp")));
        assert_eq!(parse_address("this pc", &here, home), Some(Location::ThisPc));
        assert_eq!(parse_address("Overview", &here, home), Some(Location::Overview));
        assert_eq!(parse_address("~", &here, home), Some(folder("C:\\Users\\me")));
        assert_eq!(parse_address("~\\Desktop", &here, home), Some(folder("C:\\Users\\me\\Desktop")));
        assert_eq!(parse_address("sub\\..\\other", &here, home), Some(folder("C:\\Work\\other")));
        assert_eq!(parse_address("D:", &here, home), Some(folder("D:\\")));
        assert_eq!(parse_address("", &here, home), None);
        assert_eq!(parse_address("rel", &Location::ThisPc, home), None);
    }

    #[test]
    fn env_vars_expand() {
        // SAFETY: tests in this module don't read this variable concurrently.
        unsafe { std::env::set_var("PANE_TEST_DIR", "C:\\Data") };
        assert_eq!(expand_env("%PANE_TEST_DIR%\\x"), "C:\\Data\\x");
        assert_eq!(expand_env("%PANE_NOT_SET_X%\\x"), "%PANE_NOT_SET_X%\\x");
        assert_eq!(expand_env("100%"), "100%");
    }
}
