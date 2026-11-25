//! Thumbnails for the grid: pictures and videos, made by the shell on a few
//! background COM threads and published to the UI as host images
//! ("host:<name>"), only for what the grid shows.
//!
//! The grid tells us which names it shows (`show`) and stops showing
//! (`hide`). Workers take the newest request first and skip ones scrolled
//! away before their turn, so a fast scroll doesn't queue up a whole folder.
//! Published thumbnails are kept, least recently shown going first, within
//! [`BUDGET`] bytes; ones on screen are never dropped.
//!
//! A name is "t/<modified>/<path>": a file changed on disk gets a new name,
//! so its old thumbnail is never shown for it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::StreamExt;
use zgx_script::ScriptHost;

use pane_fs::Entry;

/// The side thumbnails are made at: sharp up to the grid's largest icons
/// (128 points) on a 200% display.
const PIXELS: u32 = 256;
/// Bytes of published thumbnails kept (a 256 px square is 256 KiB).
const BUDGET: usize = 96 << 20;
const WORKERS: usize = 3;

/// Types the shell makes thumbnails of on a stock Windows (pictures,
/// camera raw files, videos). Others keep their icon without asking.
const THUMB_TYPES: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "dib", "tif", "tiff", "webp", "heic", "heif",
    "avif", "jxr", "wdp", "dng", "cr2", "cr3", "nef", "arw", "orf", "rw2", "raf", "mp4", "m4v",
    "mov", "avi", "wmv", "mkv", "webm", "mpg", "mpeg", "m2ts", "mts", "ts", "3gp", "flv",
];

/// The thumbnail name for an entry in `folder`, or "" for one without.
pub fn name_for(entry: &Entry, folder: &Path) -> String {
    if entry.is_dir || !THUMB_TYPES.contains(&entry.extension().as_str()) {
        return String::new();
    }
    format!("t/{}/{}", entry.modified, folder.join(&entry.name).display())
}

fn path_of(name: &str) -> Option<PathBuf> {
    let (_modified, path) = name.strip_prefix("t/")?.split_once('/')?;
    Some(PathBuf::from(path))
}

/// Shared with the workers.
#[derive(Default)]
struct Queue {
    /// Requests, newest last (taken first).
    jobs: Vec<String>,
    /// Of `jobs`, those still on screen: the rest are skipped.
    wanted: HashSet<String>,
}

struct Done {
    name: String,
    /// `None` when skipped (scrolled away) or when it has no thumbnail.
    icon: Option<pane_shell::Icon>,
    skipped: bool,
}

/// What the UI thread keeps.
#[derive(Default)]
struct Local {
    /// Names on screen, with how many cells show each.
    visible: HashMap<String, u32>,
    /// Asked of the workers and not answered yet.
    inflight: HashSet<String>,
    /// Published, least recently shown first, with their bytes.
    published: VecDeque<(String, usize)>,
    bytes: usize,
    /// Files with no thumbnail: not asked again this run.
    failed: HashSet<String>,
}

#[derive(Clone)]
pub struct Thumbs {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    local: Rc<RefCell<Local>>,
}

impl Thumbs {
    /// Starts the workers. Spawn the returned future on the window's tasks:
    /// it publishes thumbnails to `host` as they arrive.
    pub fn start(host: ScriptHost) -> (Thumbs, impl Future<Output = ()>) {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (done_tx, done_rx) = unbounded::<Done>();
        for n in 0..WORKERS {
            let (queue, done_tx) = (queue.clone(), done_tx.clone());
            std::thread::Builder::new()
                .name(format!("pane-thumbs-{n}"))
                .spawn(move || work(&queue, &done_tx))
                .expect("thumbnail thread");
        }
        let thumbs = Thumbs { queue, local: Rc::new(RefCell::new(Local::default())) };
        let publish = publish(host, thumbs.local.clone(), done_rx);
        (thumbs, publish)
    }

    /// The grid shows `names`: make the ones not made yet, newest first.
    pub fn show(&self, names: &[&str]) {
        let mut local = self.local.borrow_mut();
        let mut asks = Vec::new();
        for &name in names {
            if !name.starts_with("t/") {
                continue;
            }
            let count = local.visible.entry(name.to_string()).or_insert(0);
            *count += 1;
            if *count > 1 || local.failed.contains(name) {
                continue;
            }
            if let Some(at) = local.published.iter().position(|(n, _)| n == name) {
                let item = local.published.remove(at).expect("found");
                local.published.push_back(item);
                continue;
            }
            asks.push(name.to_string());
        }
        if asks.is_empty() {
            return;
        }
        let (lock, wake) = &*self.queue;
        let mut q = lock.lock().expect("thumbnail queue");
        for name in asks {
            q.wanted.insert(name.clone());
            if local.inflight.insert(name.clone()) {
                q.jobs.push(name);
            }
        }
        drop(q);
        wake.notify_all();
    }

    /// The grid stopped showing `names`.
    pub fn hide(&self, names: &[&str]) {
        let mut local = self.local.borrow_mut();
        let mut gone = Vec::new();
        for &name in names {
            if let Some(count) = local.visible.get_mut(name) {
                *count -= 1;
                if *count == 0 {
                    local.visible.remove(name);
                    gone.push(name);
                }
            }
        }
        if !gone.is_empty() {
            let mut q = self.queue.0.lock().expect("thumbnail queue");
            for name in gone {
                q.wanted.remove(name);
            }
        }
    }
}

fn work(queue: &(Mutex<Queue>, Condvar), done: &UnboundedSender<Done>) {
    pane_shell::init_com();
    let (lock, wake) = queue;
    loop {
        let (name, wanted) = {
            let mut q = lock.lock().expect("thumbnail queue");
            while q.jobs.is_empty() {
                q = wake.wait(q).expect("thumbnail queue");
            }
            let name = q.jobs.pop().expect("a job");
            let wanted = q.wanted.remove(&name);
            (name, wanted)
        };
        let icon = if wanted { path_of(&name).and_then(|p| pane_shell::thumbnail_rgba(&p, PIXELS)) } else { None };
        if done.unbounded_send(Done { name, icon, skipped: !wanted }).is_err() {
            return;
        }
    }
}

async fn publish(host: ScriptHost, local: Rc<RefCell<Local>>, mut done: UnboundedReceiver<Done>) {
    while let Some(Done { name, icon, skipped }) = done.next().await {
        let mut l = local.borrow_mut();
        l.inflight.remove(&name);
        if skipped {
            continue;
        }
        let Some(icon) = icon else {
            l.failed.insert(name);
            continue;
        };
        let bytes = icon.rgba.len();
        let Ok(image) = zgx::image::ImageData::new(icon.width, icon.height, icon.rgba) else {
            l.failed.insert(name);
            continue;
        };
        host.image(&name, Some(Arc::new(image)));
        l.published.push_back((name, bytes));
        l.bytes += bytes;
        // Over budget: drop the least recently shown that are off screen.
        let mut turns = l.published.len();
        while l.bytes > BUDGET && turns > 0 {
            turns -= 1;
            let (old, size) = l.published.pop_front().expect("non-empty");
            if l.visible.contains_key(&old) {
                l.published.push_back((old, size));
            } else {
                l.bytes -= size;
                host.image(&old, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool, modified: u64) -> Entry {
        Entry { name: name.into(), is_dir, size: 0, modified, hidden: false, system: false }
    }

    #[test]
    fn pictures_and_videos_get_names_that_change_with_the_file() {
        let folder = Path::new("C:\\pics");
        let name = name_for(&entry("Beach.JPG", false, 42), folder);
        assert_eq!(name, "t/42/C:\\pics\\Beach.JPG");
        assert_eq!(path_of(&name), Some(PathBuf::from("C:\\pics\\Beach.JPG")));
        assert_ne!(name_for(&entry("Beach.JPG", false, 43), folder), name);
        assert!(!name_for(&entry("clip.mp4", false, 1), folder).is_empty());
        assert_eq!(name_for(&entry("notes.txt", false, 1), folder), "");
        assert_eq!(name_for(&entry("album.png", true, 1), folder), "");
    }
}
