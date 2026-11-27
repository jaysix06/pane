//! Folder sizes for the folder on screen, worked out on a background thread
//! and published as `shared("sizes")`:
//!
//! `{ folder, done, total, items = { [name] = { count, size, share } } }`
//!
//! First every subfolder's item count ("9 items", quick), then each one's
//! size, all the way down (slow on big trees). `share` is the folder's part
//! of everything here, 0 to 1, once every size is known (-1 before). A new
//! folder cancels the one being measured. Sizes are kept for a couple of
//! minutes, so going back and forth doesn't walk the same trees again.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::StreamExt;
use zgx_script::{ScriptHost, Shared, Value};

/// How long a measured size is trusted.
const KEEP: Duration = Duration::from_secs(120);
/// How often the worker sends what it has found.
const BATCH: Duration = Duration::from_millis(150);

struct Job {
    generation: u64,
    folder: PathBuf,
    names: Vec<String>,
    cancel: Arc<AtomicBool>,
}

enum Found {
    Count(String, usize),
    Size(String, u64),
}

struct Batch {
    generation: u64,
    found: Vec<Found>,
    done: bool,
}

#[derive(Default)]
struct Item {
    count: Option<usize>,
    bytes: Option<u64>,
}

/// The UI side: what's known about the folder being measured.
#[derive(Default)]
struct Local {
    generation: u64,
    folder: Option<PathBuf>,
    /// Names in listing order, and what's known of each.
    order: Vec<String>,
    items: HashMap<String, Item>,
    /// The files directly in the folder, added to the total.
    file_bytes: u64,
    done: bool,
}

pub struct Sizes {
    queue: Arc<(Mutex<Option<Job>>, Condvar)>,
    cancel: RefCell<Arc<AtomicBool>>,
    local: Rc<RefCell<Local>>,
    out: Shared,
    /// Says which folder has every size known, for app.rs to take
    /// (a Size sort puts the folders in order then).
    done_rx: RefCell<Option<UnboundedReceiver<PathBuf>>>,
}

impl Sizes {
    pub fn start(host: &ScriptHost) -> (Sizes, impl Future<Output = ()> + use<>) {
        let queue = Arc::new((Mutex::new(None::<Job>), Condvar::new()));
        let (tx, rx) = unbounded::<Batch>();
        {
            let queue = queue.clone();
            std::thread::Builder::new()
                .name("pane-sizes".into())
                .spawn(move || work(&queue, &tx))
                .expect("folder size thread");
        }
        let (done_tx, done_rx) = unbounded::<PathBuf>();
        let sizes = Sizes {
            queue,
            cancel: RefCell::new(Arc::new(AtomicBool::new(false))),
            local: Rc::default(),
            out: host.shared("sizes", Value::Nil),
            done_rx: RefCell::new(Some(done_rx)),
        };
        let publish = publish(sizes.out.clone(), sizes.local.clone(), rx, done_tx);
        (sizes, publish)
    }

    /// Measures `names` (subfolders of `folder`), replacing whatever was
    /// being measured. `file_bytes` is the files beside them. The same
    /// folder with the same subfolders carries on where it is: files that
    /// change all the time (a home folder's) must not restart a long walk.
    pub fn measure(&self, folder: &Path, names: Vec<String>, file_bytes: u64) {
        {
            let mut local = self.local.borrow_mut();
            if local.folder.as_deref() == Some(folder) && local.order == names {
                if local.file_bytes != file_bytes {
                    local.file_bytes = file_bytes;
                    publish_now(&self.out, &local);
                }
                return;
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel.replace(cancel.clone()).store(true, Ordering::Relaxed);
        let generation = {
            let mut local = self.local.borrow_mut();
            local.generation += 1;
            local.folder = Some(folder.to_path_buf());
            local.items = names.iter().map(|n| (n.clone(), Item::default())).collect();
            local.order = names.clone();
            local.file_bytes = file_bytes;
            local.done = names.is_empty();
            local.generation
        };
        publish_now(&self.out, &self.local.borrow());
        if names.is_empty() {
            return;
        }
        let (lock, wake) = &*self.queue;
        *lock.lock().expect("size queue") = Some(Job { generation, folder: folder.to_path_buf(), names, cancel });
        wake.notify_one();
    }

    /// Takes the stream of folders whose sizes are all known (once).
    pub fn take_done(&self) -> Option<UnboundedReceiver<PathBuf>> {
        self.done_rx.borrow_mut().take()
    }

    /// Every subfolder's size in bytes, once all of `folder`'s are known.
    pub fn known(&self, folder: &Path) -> Option<HashMap<String, u64>> {
        let l = self.local.borrow();
        if !l.done || l.folder.as_deref() != Some(folder) {
            return None;
        }
        Some(l.items.iter().filter_map(|(n, i)| Some((n.clone(), i.bytes?))).collect())
    }

    /// Stops measuring and shows no sizes (a place that isn't a folder, or
    /// sizes turned off).
    pub fn clear(&self) {
        self.cancel.borrow().store(true, Ordering::Relaxed);
        let mut local = self.local.borrow_mut();
        local.generation += 1;
        local.folder = None;
        local.order.clear();
        local.items.clear();
        self.out.set(Value::Nil);
    }
}

fn work(queue: &(Mutex<Option<Job>>, Condvar), tx: &UnboundedSender<Batch>) {
    pane_fs::background_priority();
    let mut known: HashMap<PathBuf, (u64, Instant)> = HashMap::new();
    loop {
        let job = {
            let (lock, wake) = queue;
            let mut slot = lock.lock().expect("size queue");
            loop {
                if let Some(job) = slot.take() {
                    break job;
                }
                slot = wake.wait(slot).expect("size queue");
            }
        };
        known.retain(|_, (_, at)| at.elapsed() < KEEP);
        let started = Instant::now();
        let mut found = Vec::new();
        let mut sent = Instant::now();
        let send = |found: &mut Vec<Found>, done: bool| {
            let batch = Batch { generation: job.generation, found: std::mem::take(found), done };
            tx.unbounded_send(batch).is_ok()
        };
        // Item counts first: one listing each.
        for name in &job.names {
            if job.cancel.load(Ordering::Relaxed) {
                break;
            }
            if let Some(n) = pane_fs::item_count(&job.folder.join(name)) {
                found.push(Found::Count(name.clone(), n));
            }
            if sent.elapsed() >= BATCH {
                send(&mut found, false);
                sent = Instant::now();
            }
        }
        // Then sizes, all the way down.
        for name in &job.names {
            if job.cancel.load(Ordering::Relaxed) {
                break;
            }
            let path = job.folder.join(name);
            let bytes = match known.get(&path) {
                Some(&(bytes, _)) => Some(bytes),
                None => pane_fs::folder_size(&path, &job.cancel).map(|s| {
                    known.insert(path.clone(), (s.bytes, Instant::now()));
                    s.bytes
                }),
            };
            if let Some(bytes) = bytes {
                found.push(Found::Size(name.clone(), bytes));
            }
            if sent.elapsed() >= BATCH {
                send(&mut found, false);
                sent = Instant::now();
            }
        }
        if !job.cancel.load(Ordering::Relaxed) {
            send(&mut found, true);
            eprintln!(
                "[pane] sizes of {} folders in {}: {:.1} s",
                job.names.len(),
                job.folder.display(),
                started.elapsed().as_secs_f64()
            );
        }
    }
}

async fn publish(
    out: Shared,
    local: Rc<RefCell<Local>>,
    mut rx: UnboundedReceiver<Batch>,
    done_tx: UnboundedSender<PathBuf>,
) {
    while let Some(first) = rx.next().await {
        let mut batches = vec![first];
        while let Ok(more) = rx.try_recv() {
            batches.push(more);
        }
        let mut l = local.borrow_mut();
        let was_done = l.done;
        let mut changed = false;
        for batch in batches {
            if batch.generation != l.generation {
                continue;
            }
            for f in batch.found {
                match f {
                    Found::Count(name, n) => {
                        if let Some(item) = l.items.get_mut(&name) {
                            item.count = Some(n);
                        }
                    }
                    Found::Size(name, bytes) => {
                        if let Some(item) = l.items.get_mut(&name) {
                            item.bytes = Some(bytes);
                        }
                    }
                }
            }
            l.done |= batch.done;
            changed = true;
        }
        if changed {
            publish_now(&out, &l);
            if l.done && !was_done {
                if let Some(folder) = &l.folder {
                    let _ = done_tx.unbounded_send(folder.clone());
                }
            }
        }
    }
}

fn items_text(n: usize) -> String {
    match n {
        0 => "Empty".into(),
        1 => "1 item".into(),
        _ => format!("{n} items"),
    }
}

fn publish_now(out: &Shared, l: &Local) {
    let Some(folder) = &l.folder else {
        out.set(Value::Nil);
        return;
    };
    let total: u64 = l.file_bytes + l.items.values().filter_map(|i| i.bytes).sum::<u64>();
    let items: Vec<(Rc<str>, Value)> = l
        .order
        .iter()
        .filter_map(|name| {
            let item = l.items.get(name)?;
            let share = match item.bytes {
                Some(b) if l.done && total > 0 => b as f64 / total as f64,
                _ => -1.0,
            };
            Some((
                Rc::from(name.as_str()),
                Value::record([
                    ("count", Value::from(item.count.map(items_text).unwrap_or_default().as_str())),
                    ("size", Value::from(item.bytes.map(pane_fs::format_size).unwrap_or_default().as_str())),
                    ("share", Value::from(share)),
                ]),
            ))
        })
        .collect();
    let folder_bytes: u64 = l.items.values().filter_map(|i| i.bytes).sum();
    out.set(Value::record([
        ("folder", Value::from(folder.display().to_string().as_str())),
        ("done", Value::from(l.done)),
        ("total", Value::from(if l.done { pane_fs::format_size(total) } else { String::new() }.as_str())),
        ("folders_total", Value::from(if l.done { pane_fs::format_size(folder_bytes) } else { String::new() }.as_str())),
        ("items", Value::record(items)),
    ]));
}
