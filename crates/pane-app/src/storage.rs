//! The Storage Overview's state: per-drive scan results (cached, then
//! refreshed), the scan in progress, the drill-down, and cleanup cards,
//! published as `shared("storage")`. Scans run on a worker thread: through
//! the helper service (instant, MFT) when it is installed, else the
//! standard folder walk.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_channel::mpsc::unbounded;
use futures_util::StreamExt;
use pane_scan::{Category, Event, Mode, Progress, ScanResult};
use zgx_desktop::TaskSpawner;
use zgx_script::{ScriptHost, Shared, Value};

pub type StorageRef = Rc<RefCell<Storage>>;

/// A cached result older than this is refreshed automatically on launch
/// (when scans are instant; standard scans wait for Rescan).
const STALE_SECS: u64 = 10 * 60;

pub struct Storage {
    out: Shared,
    tasks: TaskSpawner,
    /// "C:" etc.
    selected: String,
    results: HashMap<String, ScanResult>,
    scanning: Option<String>,
    progress: Option<Progress>,
    cancel: Arc<AtomicBool>,
    drill: Option<Category>,
    error: Option<String>,
    /// The drives, refreshed on each publish request.
    drives: Vec<pane_shell::Drive>,
    /// A message for a toast, and when (so the same one shows once).
    error_toast: Option<(String, u64)>,
}

impl Storage {
    pub fn start(host: &ScriptHost, tasks: &TaskSpawner) -> StorageRef {
        let drives: Vec<_> = pane_shell::drives().into_iter().filter(|d| d.total > 0).collect();
        let mut results = HashMap::new();
        for d in &drives {
            let v = volume_of(&d.root);
            if let Some(r) = pane_scan::cache::load(&v) {
                results.insert(v, r);
            }
        }
        let selected = drives
            .iter()
            .map(|d| volume_of(&d.root))
            .find(|v| v == "C:")
            .or_else(|| drives.first().map(|d| volume_of(&d.root)))
            .unwrap_or_else(|| "C:".into());
        let storage = Rc::new(RefCell::new(Storage {
            out: host.shared("storage", Value::Nil),
            tasks: tasks.clone(),
            selected: selected.clone(),
            results,
            scanning: None,
            progress: None,
            cancel: Arc::new(AtomicBool::new(false)),
            drill: None,
            error: None,
            drives,
            error_toast: None,
        }));
        publish(&storage);
        // First run, or a stale result with instant scans: scan now.
        let needs = {
            let s = storage.borrow();
            match s.results.get(&selected) {
                None => true,
                Some(r) => crate::service::available() && pane_scan::model::now_secs() - r.scanned_at > STALE_SECS,
            }
        };
        if needs {
            scan(&storage, &selected);
        }
        storage
    }
}

/// Scans `volume` ("C:"), replacing any scan running.
pub fn scan(storage: &StorageRef, volume: &str) {
    let (tx, mut rx) = unbounded::<Event>();
    let cancel = {
        let mut s = storage.borrow_mut();
        s.cancel.store(true, Ordering::Relaxed);
        s.cancel = Arc::new(AtomicBool::new(false));
        s.scanning = Some(volume.to_string());
        s.progress = None;
        s.error = None;
        s.cancel.clone()
    };
    publish(storage);

    let volume_owned = volume.to_string();
    std::thread::Builder::new()
        .name("pane-scan".into())
        .spawn(move || {
            let report = |e: Event| {
                if let Event::Done(r) = &e {
                    let _ = pane_scan::cache::save(r);
                }
                let _ = tx.unbounded_send(e);
            };
            // Instant through the service, else the standard walk.
            if !crate::service::scan(&volume_owned, &report, &cancel) {
                let root = PathBuf::from(format!("{volume_owned}\\"));
                pane_scan::walk::scan(&root, &report, &cancel);
            }
        })
        .expect("scan thread");

    let storage2 = storage.clone();
    let tasks = storage.borrow().tasks.clone();
    let volume = volume.to_string();
    tasks.spawn(async move {
        while let Some(event) = rx.next().await {
            {
                let mut s = storage2.borrow_mut();
                if s.scanning.as_deref() != Some(volume.as_str()) {
                    return; // replaced by another scan
                }
                match event {
                    Event::Progress(p) => s.progress = Some(p),
                    Event::Done(r) => {
                        s.results.insert(volume.clone(), *r);
                        s.scanning = None;
                        s.progress = None;
                    }
                    Event::Error(e) => {
                        s.scanning = None;
                        s.error = Some(e);
                    }
                }
            }
            publish(&storage2);
        }
        // The worker ended without a result (cancelled): stop showing a scan.
        let mut s = storage2.borrow_mut();
        if s.scanning.as_deref() == Some(volume.as_str()) {
            s.scanning = None;
            drop(s);
            publish(&storage2);
        }
    });
}

pub fn select(storage: &StorageRef, volume: &str) {
    let needs_scan = {
        let mut s = storage.borrow_mut();
        s.selected = volume.to_string();
        s.drill = None;
        !s.results.contains_key(volume) && s.scanning.is_none()
    };
    publish(storage);
    if needs_scan {
        scan(storage, volume);
    }
}

pub fn rescan(storage: &StorageRef) {
    let v = storage.borrow().selected.clone();
    scan(storage, &v);
}

pub fn drill(storage: &StorageRef, category: Option<&str>) {
    storage.borrow_mut().drill = category.and_then(Category::from_id);
    publish(storage);
}

/// After the Recycle Bin is emptied: show it at once, confirm with a rescan later.
pub fn recycle_bin_emptied(storage: &StorageRef) {
    {
        let mut s = storage.borrow_mut();
        for r in s.results.values_mut() {
            let i = Category::Recycle.index();
            let freed = r.totals.bytes[i];
            r.totals.bytes[i] = 0;
            r.totals.files[i] = 0;
            r.used = r.used.saturating_sub(freed);
            r.free += freed;
            r.top[i].clear();
            if let Some(c) = r.cleanup.iter_mut().find(|c| c.kind == pane_scan::CleanupKind::RecycleBin) {
                c.bytes = 0;
                c.files = 0;
                c.items.clear();
            }
        }
    }
    publish(storage);
}

/// Installs (`install = true`) or removes the helper service: Windows asks
/// for permission once; afterwards the Overview rescans in the new mode.
pub fn set_instant(storage: &StorageRef, install: bool) {
    let Some(exe) = crate::service::service_exe() else {
        toast(storage, "The helper service isn't next to Pane (pane-scan-service.exe).");
        return;
    };
    let hwnd = pane_shell::active_window().0 as isize;
    let (tx, rx) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        let r = pane_shell::run_elevated_and_wait(hwnd, &exe, if install { "install" } else { "uninstall" });
        let _ = tx.send(r);
    });
    let storage2 = storage.clone();
    let tasks = storage.borrow().tasks.clone();
    tasks.spawn(async move {
        match rx.await {
            Ok(Ok(0)) => {
                toast(
                    &storage2,
                    if install { "Instant scan is on." } else { "Instant scan is off; Pane will walk folders instead." },
                );
                rescan(&storage2);
            }
            Ok(Ok(code)) => toast(&storage2, &format!("Setting up instant scan failed (code {code}).")),
            Ok(Err(e)) => toast(&storage2, &e),
            Err(_) => {}
        }
        publish(&storage2);
    });
}

fn toast(storage: &StorageRef, message: &str) {
    storage.borrow_mut().error_toast = Some((message.to_string(), pane_scan::model::now_secs()));
    publish(storage);
}

/// The biggest item of a cleanup card (what "Review" opens).
pub fn cleanup_target(storage: &StorageRef, id: &str) -> Option<String> {
    let s = storage.borrow();
    let r = s.results.get(&s.selected)?;
    let c = r.cleanup.iter().find(|c| c.kind.id() == id)?;
    let item = c.items.first()?;
    Some(item.path.clone())
}

// ---------------------------------------------------------------- publishing

pub fn publish(storage: &StorageRef) {
    let s = storage.borrow();
    let selected = s.selected.clone();
    let result = s.results.get(&selected);
    let scanning = s.scanning.as_deref() == Some(selected.as_str());
    let drive = s.drives.iter().find(|d| volume_of(&d.root) == selected);
    let (total, free) = match (result, drive) {
        (_, Some(d)) => (d.total, d.free),
        (Some(r), None) => (r.total, r.free),
        _ => (0, 0),
    };
    let used = total.saturating_sub(free);

    // While scanning: the live totals; otherwise the last result. Totals
    // that overshoot `used` (hard links counted twice by a walk) are scaled
    // to fit, so the bar ends where the drive's usage does.
    let live_bytes: Option<[u64; 11]> = if scanning { s.progress.as_ref().map(|p| p.totals.bytes) } else { None };
    let bytes = live_bytes.or_else(|| result.map(|r| r.totals.bytes)).unwrap_or([0; 11]);
    let sum: u64 = bytes.iter().sum();
    let scale = if sum > used && sum > 0 { used as f64 / sum as f64 } else { 1.0 };
    let files = result.map(|r| r.totals.files).unwrap_or([0; 11]);

    let mut categories = Vec::new();
    let mut by_id = Vec::new();
    for c in Category::ALL {
        let b = (bytes[c.index()] as f64 * scale) as u64;
        by_id.push((c.id(), Value::from(b as f64)));
        categories.push(Value::record([
            ("id", Value::from(c.id())),
            ("bytes", Value::from(b as f64)),
            ("text", Value::from(size_text(b).as_str())),
            ("files", Value::from(files[c.index()] as f64)),
            ("share", Value::from(if used > 0 { b as f64 / used as f64 } else { 0.0 })),
        ]));
    }

    let phase = if scanning {
        "scanning"
    } else if s.error.is_some() {
        "error"
    } else if result.is_some() {
        "done"
    } else {
        "empty"
    };
    let status = match (scanning, result, &s.progress) {
        (true, _, Some(p)) => format!("Scanning {}", p.current),
        (true, _, None) => format!("Starting a scan of {selected}"),
        (false, Some(r), _) => format!(
            "Scanned {} · {} files in {}",
            ago(r.scanned_at),
            count_text(r.files),
            duration_text(r.took_ms)
        ),
        (false, None, _) => s.error.clone().unwrap_or_else(|| "Not scanned yet".into()),
    };

    let cleanup: Vec<Value> = result
        .map(|r| {
            r.cleanup
                .iter()
                .map(|c| {
                    Value::record([
                        ("id", Value::from(c.kind.id())),
                        ("bytes", Value::from(c.bytes as f64)),
                        ("text", Value::from(size_text(c.bytes).as_str())),
                        ("files", Value::from(c.files as f64)),
                        ("count_text", Value::from(count_text(c.files).as_str())),
                        ("items", items_value(&c.items, c.bytes, 5)),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let drill = match (s.drill, result) {
        (Some(c), Some(r)) => {
            let b = r.totals.bytes[c.index()];
            Value::record([("id", Value::from(c.id())), ("items", items_value(&r.top[c.index()], b, 30))])
        }
        _ => Value::Nil,
    };

    let drives: Vec<Value> = s
        .drives
        .iter()
        .map(|d| {
            let v = volume_of(&d.root);
            let cats = s.results.get(&v).map(|r| {
                let sum: u64 = r.totals.bytes.iter().sum();
                let used = d.total.saturating_sub(d.free);
                let scale = if sum > used && sum > 0 { used as f64 / sum as f64 } else { 1.0 };
                Value::record(
                    Category::ALL
                        .iter()
                        .map(|c| (c.id(), Value::from(r.totals.bytes[c.index()] as f64 * scale)))
                        .collect::<Vec<_>>(),
                )
            });
            Value::record([
                ("letter", Value::from(v.as_str())),
                ("label", Value::from(d.label.as_str())),
                ("total", Value::from(d.total as f64)),
                ("used", Value::from(d.total.saturating_sub(d.free) as f64)),
                ("used_text", Value::from(size_text(d.total.saturating_sub(d.free)).as_str())),
                ("total_text", Value::from(size_text(d.total).as_str())),
                ("categories", cats.unwrap_or(Value::Nil)),
                ("selected", Value::from(v == selected)),
            ])
        })
        .collect();

    let mode = result.map(|r| r.mode);
    s.out.set(Value::record([
        ("selected", Value::from(selected.as_str())),
        ("phase", Value::from(phase)),
        ("status", Value::from(status.as_str())),
        ("permille", Value::from(s.progress.as_ref().map_or(0.0, |p| p.permille as f64))),
        ("files", Value::from(s.progress.as_ref().map_or(0.0, |p| p.files as f64))),
        ("total", Value::from(total as f64)),
        ("used", Value::from(used as f64)),
        ("free", Value::from(free as f64)),
        ("used_text", Value::from(size_text(used).as_str())),
        ("total_text", Value::from(size_text(total).as_str())),
        ("free_text", Value::from(size_text(free).as_str())),
        ("bytes", Value::record(by_id)),
        ("categories", Value::List(Rc::from(categories))),
        ("cleanup", Value::List(Rc::from(cleanup))),
        ("drill", drill),
        ("drives", Value::List(Rc::from(drives))),
        ("instant", Value::from(crate::service::available())),
        ("can_install", Value::from(crate::service::service_exe().is_some())),
        ("toast", match &s.error_toast {
            Some((m, at)) => Value::record([("message", Value::from(m.as_str())), ("at", Value::from(*at as f64))]),
            None => Value::Nil,
        }),
        ("mode", Value::from(match mode {
            Some(Mode::Instant) => "instant",
            Some(Mode::Standard) => "standard",
            None => "",
        })),
    ]));
}

fn items_value(items: &[pane_scan::Item], of: u64, max: usize) -> Value {
    let biggest = items.first().map_or(1, |i| i.bytes.max(1));
    Value::List(Rc::from(
        items
            .iter()
            .take(max)
            .map(|i| {
                let name = i.path.rsplit('\\').next().unwrap_or(&i.path);
                Value::record([
                    ("path", Value::from(i.path.as_str())),
                    ("name", Value::from(name)),
                    ("bytes", Value::from(i.bytes as f64)),
                    ("text", Value::from(size_text(i.bytes).as_str())),
                    ("files", Value::from(i.files as f64)),
                    ("share", Value::from(if of > 0 { i.bytes as f64 / of as f64 } else { 0.0 })),
                    // For size bars: relative to the biggest in the list.
                    ("bar", Value::from(i.bytes as f64 / biggest as f64)),
                ])
            })
            .collect::<Vec<_>>(),
    ))
}

fn volume_of(root: &std::path::Path) -> String {
    root.to_string_lossy().trim_end_matches('\\').to_ascii_uppercase()
}

/// "334.3 GB", "812 MB", "4.2 KB": one decimal below 100.
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else if v < 100.0 {
        format!("{v:.1} {}", UNITS[u])
    } else {
        format!("{v:.0} {}", UNITS[u])
    }
}

/// "1.8M", "210K", "899".
pub fn count_text(n: u64) -> String {
    match n {
        0..=9_999 => n.to_string(),
        10_000..=999_999 => format!("{}K", n / 1000),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

fn duration_text(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} min {} s", ms / 60_000, (ms / 1000) % 60)
    }
}

/// "just now", "4 min ago", "3 h ago", "yesterday", "6 days ago".
pub fn ago(then: u64) -> String {
    let secs = pane_scan::model::now_secs().saturating_sub(then);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts() {
        assert_eq!(size_text(512), "512 B");
        assert_eq!(size_text(1536), "1.5 KB");
        assert_eq!(size_text(334 * (1 << 30) + (1 << 29)), "334 GB");
        assert_eq!(size_text(5 * (1 << 30)), "5.0 GB");
        assert_eq!(count_text(899), "899");
        assert_eq!(count_text(210_165), "210K");
        assert_eq!(count_text(1_826_374), "1.8M");
        assert_eq!(duration_text(9200), "9.2 s");
        assert_eq!(duration_text(450), "450 ms");
        assert_eq!(ago(pane_scan::model::now_secs()), "just now");
        assert_eq!(ago(pane_scan::model::now_secs() - 3 * 3600), "3 h ago");
    }
}
