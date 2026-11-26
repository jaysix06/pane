//! The open tabs, saved as `%APPDATA%\Pane\session.json` and reopened on
//! launch when "Restore last session" is on. The window's size and place are
//! zgx's (`frame_autosave`); this is what's inside it.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::nav::Location;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    /// `Location::to_ui`: a folder path, "::thispc" or "::overview".
    pub location: String,
    pub view: String,
    /// "name", "date", "type" or "size".
    pub sort: String,
    pub descending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub tabs: Vec<SavedTab>,
    /// Index into `tabs`.
    pub active: usize,
}

impl Session {
    pub fn path(settings_path: &Path) -> PathBuf {
        settings_path.with_file_name("session.json")
    }

    /// The saved session, or `None` when there is none to restore (no file,
    /// a broken one, or no tabs).
    pub fn load(path: &Path) -> Option<Session> {
        let text = std::fs::read_to_string(path).ok()?;
        let s: Session = serde_json::from_str(&text).ok()?;
        (!s.tabs.is_empty()).then_some(s)
    }

    /// Writes atomically: a temporary file beside it, renamed over it.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(io::Error::other)?)?;
        std::fs::rename(&tmp, path)
    }

    /// The tabs that can still open: folders gone since (an unplugged
    /// drive, a deleted folder) are left out, keeping the active tab where
    /// it was among the rest. `None` if none can.
    pub fn usable(self, exists: impl Fn(&Path) -> bool) -> Option<Session> {
        let mut active = 0;
        let mut tabs = Vec::new();
        for (i, tab) in self.tabs.into_iter().enumerate() {
            let ok = match Location::from_ui(&tab.location) {
                Location::Folder(p) | Location::Search { root: p, .. } => exists(&p),
                Location::ThisPc | Location::Overview => true,
            };
            if ok {
                if i <= self.active {
                    active = tabs.len();
                }
                tabs.push(tab);
            }
        }
        (!tabs.is_empty()).then_some(Session { tabs, active })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(location: &str) -> SavedTab {
        SavedTab { location: location.into(), view: "details".into(), sort: "name".into(), descending: false }
    }

    #[test]
    fn saves_and_loads() {
        let path = std::env::temp_dir().join(format!("pane-session-test-{}.json", std::process::id()));
        let s = Session { tabs: vec![tab("C:\\"), tab("::thispc")], active: 1 };
        s.save(&path).unwrap();
        assert_eq!(Session::load(&path), Some(s));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(Session::load(&path), None);
    }

    #[test]
    fn leaves_out_folders_gone_since() {
        let s = Session { tabs: vec![tab("C:\\a"), tab("C:\\gone"), tab("C:\\b"), tab("C:\\gone2")], active: 3 };
        let kept = s.usable(|p| !p.to_string_lossy().contains("gone")).unwrap();
        assert_eq!(kept.tabs, vec![tab("C:\\a"), tab("C:\\b")]);
        assert_eq!(kept.active, 1, "the active tab was gone: the one before it");
        let none = Session { tabs: vec![tab("C:\\gone")], active: 0 }.usable(|_| false);
        assert_eq!(none, None);
    }
}
