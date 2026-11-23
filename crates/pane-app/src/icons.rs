//! File icons: extracted on a background COM thread, published to the UI as
//! host images ("host:<name>"). Each name is extracted once per run.
//!
//! Names are "<size>/<key>": size "s" (32 px, lists) or "l" (96 px, grid);
//! the key says what the icon depends on: "d" (a folder), "e:<ext>" (any file
//! of that type), or "f:<path>" for types whose icon is per file (programs,
//! shortcuts, icon files).

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;

use futures_channel::mpsc::{UnboundedReceiver, unbounded};
use futures_util::StreamExt;
use zgx_script::ScriptHost;

use pane_fs::Entry;

/// Types whose icon differs per file.
const PER_FILE: &[&str] = &["exe", "lnk", "ico", "cur", "ani", "url", "scr", "msc", "cpl", "appref-ms"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Small,
    Large,
}

impl Size {
    fn prefix(self) -> &'static str {
        match self {
            Size::Small => "s",
            Size::Large => "l",
        }
    }
    fn pixels(self) -> u32 {
        match self {
            Size::Small => 32,
            Size::Large => 96,
        }
    }
}

/// The icon key for an entry (see the module docs).
pub fn key_for(entry: &Entry, folder: &Path) -> String {
    if entry.is_dir {
        return "d".into();
    }
    let ext = entry.extension();
    if PER_FILE.contains(&ext.as_str()) {
        format!("f:{}", folder.join(&entry.name).display())
    } else {
        format!("e:{ext}")
    }
}

struct Job {
    name: String,
    path: PathBuf,
    size: Size,
}

struct Done {
    name: String,
    icon: Option<pane_shell::Icon>,
}

/// Owns the worker; cloned handles share it.
#[derive(Clone)]
pub struct Icons {
    jobs: mpsc::Sender<Job>,
    requested: Rc<RefCell<HashSet<String>>>,
}

impl Icons {
    /// Starts the worker. Spawn the returned future on the window's tasks:
    /// it publishes icons to `host` as they arrive.
    pub fn start(host: ScriptHost) -> (Icons, impl Future<Output = ()>) {
        let (jobs, rx) = mpsc::channel::<Job>();
        let (done_tx, done_rx) = unbounded::<Done>();
        std::thread::Builder::new()
            .name("pane-icons".into())
            .spawn(move || {
                pane_shell::init_com();
                while let Ok(job) = rx.recv() {
                    let icon = pane_shell::icon_rgba(&job.path, job.size.pixels());
                    if done_tx.unbounded_send(Done { name: job.name, icon }).is_err() {
                        break;
                    }
                }
            })
            .expect("icon thread");
        let icons = Icons { jobs, requested: Rc::new(RefCell::new(HashSet::new())) };
        (icons, publish(host, done_rx))
    }

    /// Asks for `key`'s icon at `size`, using `path` (a file or folder it
    /// stands for) to get it. Does nothing if already asked.
    pub fn request(&self, key: &str, path: &Path, size: Size) {
        let name = format!("{}/{key}", size.prefix());
        if self.requested.borrow_mut().insert(name.clone()) {
            let _ = self.jobs.send(Job { name, path: path.to_path_buf(), size });
        }
    }

    /// Asks for the icons a folder's entries need, one file per key.
    pub fn request_entries(&self, folder: &Path, entries: &[Entry], keys: &[String], size: Size) {
        let mut seen = HashSet::new();
        for (entry, key) in entries.iter().zip(keys) {
            if seen.insert(key.as_str()) {
                self.request(key, &folder.join(&entry.name), size);
            }
        }
    }
}

async fn publish(host: ScriptHost, mut done: UnboundedReceiver<Done>) {
    while let Some(Done { name, icon }) = done.next().await {
        let Some(icon) = icon else { continue };
        if let Ok(image) = zgx::image::ImageData::new(icon.width, icon.height, icon.rgba) {
            host.image(&name, Some(Arc::new(image)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> Entry {
        Entry { name: name.into(), is_dir, size: 0, modified: 0, hidden: false, system: false }
    }

    #[test]
    fn keys_group_by_type_except_per_file_types() {
        let folder = Path::new("C:\\x");
        assert_eq!(key_for(&entry("a.TXT", false), folder), "e:txt");
        assert_eq!(key_for(&entry("sub", true), folder), "d");
        assert_eq!(key_for(&entry("app.exe", false), folder), "f:C:\\x\\app.exe");
        assert_eq!(key_for(&entry("README", false), folder), "e:");
    }
}
