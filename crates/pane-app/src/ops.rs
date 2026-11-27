//! File operations from the UI: copy, cut, paste, delete, rename, new
//! folder and drops, run one per thread through Windows' engine
//! (`pane_shell::fileops`), with their progress published as `shared("ops")`
//! for Pane's operations panel.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_channel::mpsc::unbounded;
use futures_util::StreamExt;
use pane_shell::fileops::{Control, Op, OpOutcome, OpProgress};
use zgx_desktop::TaskSpawner;
use zgx_script::{ScriptHost, Shared, Value};

use crate::app::{self, AppRef};
use crate::nav::Location;

pub type OpsRef = Rc<RefCell<Ops>>;

struct Running {
    id: u64,
    label: String,
    /// The footer's shorter line: "Copying to D:\Backup".
    short: String,
    kind: &'static str,
    progress: OpProgress,
    finished: bool,
    failed: Option<String>,
    control: Arc<Control>,
    paused: bool,
    /// Stopped by the person (Pane's Stop, or Cancel in Windows' dialog).
    cancelled: bool,
    started: Instant,
    /// Time spent paused, so time left counts working time only.
    paused_for: Duration,
    paused_at: Option<Instant>,
    ended: Option<Instant>,
    /// When it started ("9:26 AM"), and the local day it started on.
    at: String,
    day: (u16, u16, u16),
}

impl Running {
    fn working_time(&self) -> Duration {
        let end = self.ended.or(self.paused_at).unwrap_or_else(Instant::now);
        end.saturating_duration_since(self.started).saturating_sub(self.paused_for)
    }
}

/// Finished operations kept for the footer's popover (today's).
const KEEP_FINISHED: usize = 20;

pub struct Ops {
    out: Shared,
    /// Names cut in the active tab's folder (shown dimmed), and that folder.
    cut: Shared,
    /// Asks the UI to start renaming a row: `{ id, at }`.
    rename: Shared,
    tasks: TaskSpawner,
    running: Vec<Running>,
    next: u64,
    cut_set: Option<(PathBuf, Vec<String>)>,
}

impl Ops {
    pub fn start(host: &ScriptHost, tasks: &TaskSpawner) -> OpsRef {
        Rc::new(RefCell::new(Ops {
            out: host.shared("ops", Value::List(Rc::from(Vec::new()))),
            cut: host.shared("cut", Value::record::<&str>([])),
            rename: host.shared("rename", Value::Nil),
            tasks: tasks.clone(),
            running: Vec::new(),
            next: 1,
            cut_set: None,
        }))
    }
}

fn hwnd() -> isize {
    pane_shell::active_window().0 as isize
}

fn count(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// Runs `op` on a thread, showing it in the panel; `then` runs on the UI
/// thread when it ends.
pub fn run(ops: &OpsRef, app: &AppRef, op: Op, label: String, kind: &'static str, then: Option<Box<dyn FnOnce(&OpOutcome)>>) {
    let control = Arc::new(Control::default());
    let id = {
        let mut o = ops.borrow_mut();
        let id = o.next;
        o.next += 1;
        let now = pane_fs::date::local_time(pane_fs::date::now_filetime());
        o.running.push(Running {
            id,
            short: short_label(&op, &label),
            label,
            kind,
            progress: OpProgress::default(),
            finished: false,
            failed: None,
            control: control.clone(),
            paused: false,
            cancelled: false,
            started: Instant::now(),
            paused_for: Duration::ZERO,
            paused_at: None,
            ended: None,
            at: now.as_ref().map(pane_fs::date::clock).unwrap_or_default(),
            day: now.map_or((0, 0, 0), |t| (t.year, t.month, t.day)),
        });
        // Only today's finished operations stay, and not too many.
        let today = o.running.last().map(|r| r.day);
        o.running.retain(|r| !r.finished || Some(r.day) == today);
        let finished = o.running.iter().filter(|r| r.finished).count();
        let mut extra = finished.saturating_sub(KEEP_FINISHED);
        o.running.retain(|r| {
            if r.finished && extra > 0 {
                extra -= 1;
                false
            } else {
                true
            }
        });
        id
    };
    publish(ops);

    enum Msg {
        Progress(OpProgress),
        Done(Result<OpOutcome, String>),
    }
    let (tx, mut rx) = unbounded::<Msg>();
    let owner = hwnd();
    std::thread::Builder::new()
        .name("pane-fileop".into())
        .spawn(move || {
            let ptx = tx.clone();
            let r = pane_shell::fileops::run_with(owner, &op, control, move |p| {
                let _ = ptx.unbounded_send(Msg::Progress(p));
            });
            let _ = tx.unbounded_send(Msg::Done(r.map_err(|e| e.message())));
        })
        .expect("file operation thread");

    let (ops2, app2) = (ops.clone(), app.clone());
    let tasks = ops.borrow().tasks.clone();
    tasks.spawn(async move {
        let mut then = then;
        while let Some(msg) = rx.next().await {
            match msg {
                Msg::Progress(p) => {
                    if let Some(r) = ops2.borrow_mut().running.iter_mut().find(|r| r.id == id) {
                        r.progress = p;
                    }
                    publish(&ops2);
                }
                Msg::Done(result) => {
                    {
                        let mut o = ops2.borrow_mut();
                        if let Some(r) = o.running.iter_mut().find(|r| r.id == id) {
                            r.finished = true;
                            r.ended = Some(Instant::now());
                            if let Some(at) = r.paused_at.take() {
                                r.paused_for += at.elapsed();
                            }
                            r.paused = false;
                            r.cancelled = r.control.is_cancelled()
                                || matches!(&result, Ok(out) if out.aborted && out.failed.is_empty());
                            if !r.cancelled {
                                r.progress = OpProgress { done: 1, total: 1 };
                            }
                            match &result {
                                Err(e) => r.failed = Some(e.clone()),
                                Ok(out) if !out.failed.is_empty() => {
                                    let (path, why) = &out.failed[0];
                                    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                                    r.failed = Some(if out.failed.len() == 1 {
                                        format!("{name}: {why}")
                                    } else {
                                        format!("{} items failed, {name}: {why}", out.failed.len())
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    publish(&ops2);
                    if let Err(e) = &result {
                        app2.borrow().toast(e);
                    }
                    if let (Ok(out), Some(f)) = (&result, then.take()) {
                        f(out);
                    }
                    app::refresh(&app2);
                    app::refresh_places(&app2);
                    break;
                }
            }
        }
        // The footer shows a finished operation a moment (longer when it
        // failed); after that only the popover lists it.
        let failed = ops2.borrow().running.iter().any(|r| r.id == id && r.failed.is_some());
        zgx::timer::sleep(Duration::from_millis(if failed { 8000 } else { 2500 })).await;
        publish(&ops2);
    });
}

/// Pauses (`true`) or resumes operation `id`.
pub fn set_paused(ops: &OpsRef, id: u64, paused: bool) {
    {
        let mut o = ops.borrow_mut();
        let Some(r) = o.running.iter_mut().find(|r| r.id == id && !r.finished) else { return };
        if r.paused == paused {
            return;
        }
        r.paused = paused;
        if paused {
            r.paused_at = Some(Instant::now());
        } else if let Some(at) = r.paused_at.take() {
            r.paused_for += at.elapsed();
        }
        r.control.set_paused(paused);
    }
    publish(ops);
}

/// Stops operation `id` before its next item (what was done stays done).
pub fn cancel(ops: &OpsRef, id: u64) {
    if let Some(r) = ops.borrow().running.iter().find(|r| r.id == id && !r.finished) {
        r.control.cancel();
    }
}

/// Forgets finished operations (the popover's "Clear").
pub fn clear_finished(ops: &OpsRef) {
    ops.borrow_mut().running.retain(|r| !r.finished);
    publish(ops);
}

/// The footer's line: "Copying to D:\Backup"; other kinds keep their label.
fn short_label(op: &Op, label: &str) -> String {
    match op {
        Op::Copy { to, .. } => format!("Copying to {}", short_path(to)),
        Op::Move { to, .. } => format!("Moving to {}", short_path(to)),
        _ => label.to_string(),
    }
}

/// "D:\Backup" as it is; a deep path as its drive and last two folders:
/// "C:\…\optest\dest".
fn short_path(p: &Path) -> String {
    let parts: Vec<String> =
        p.components().map(|c| c.as_os_str().to_string_lossy().trim_end_matches('\\').to_string()).collect();
    if parts.len() <= 4 {
        return p.display().to_string();
    }
    format!(r"{}\…\{}\{}", parts[0], parts[parts.len() - 2], parts[parts.len() - 1])
}

/// How long the rest should take at the rate so far; "" until there is
/// enough to go on.
pub fn time_left(working: Duration, fraction: f64) -> String {
    let secs = working.as_secs_f64();
    if !(0.02..1.0).contains(&fraction) || secs < 2.0 {
        return String::new();
    }
    let left = (secs * (1.0 - fraction) / fraction).round() as u64;
    match left {
        0..=9 => "A few seconds left".into(),
        10..=59 => format!("{} s left", left.div_ceil(5) * 5),
        60..=3599 => format!("{} min left", left.div_ceil(60)),
        _ => format!("{} h {} min left", left / 3600, (left % 3600) / 60),
    }
}

fn publish(ops: &OpsRef) {
    let o = ops.borrow();
    let list: Vec<Value> = o
        .running
        .iter()
        .map(|r| {
            let fraction = if r.progress.total > 0 { r.progress.done as f64 / r.progress.total as f64 } else { 0.0 };
            let fraction = fraction.clamp(0.0, 1.0);
            let cancelled = r.finished && r.cancelled;
            // Just finished: the footer still shows it.
            let recent = r
                .ended
                .is_some_and(|e| e.elapsed() < Duration::from_millis(if r.failed.is_some() { 7900 } else { 2400 }));
            let eta = if r.finished || r.paused { String::new() } else { time_left(r.working_time(), fraction) };
            Value::record([
                ("id", Value::from(r.id as f64)),
                ("label", Value::from(r.label.as_str())),
                ("short", Value::from(r.short.as_str())),
                ("kind", Value::from(r.kind)),
                ("progress", Value::from(fraction)),
                ("finished", Value::from(r.finished)),
                ("recent", Value::from(recent)),
                ("cancelled", Value::from(cancelled)),
                ("paused", Value::from(r.paused)),
                // Renames and new folders are over before a button could help.
                ("can_pause", Value::from(matches!(r.kind, "copy" | "move" | "delete"))),
                ("eta", Value::from(eta.as_str())),
                ("at", Value::from(r.at.as_str())),
                ("error", r.failed.as_deref().map_or(Value::Nil, Value::from)),
            ])
        })
        .collect();
    o.out.set(Value::List(Rc::from(list)));
}

/// Cut items as `{ folder, ids }`: the UI dims them while it shows that folder.
fn publish_cut(ops: &OpsRef, _app: &AppRef) {
    let o = ops.borrow();
    let value = match &o.cut_set {
        Some((dir, names)) => Value::record([
            ("folder", Value::from(dir.display().to_string().as_str())),
            ("ids", Value::record(names.iter().map(|n| (n.as_str(), Value::from(true))).collect::<Vec<_>>())),
        ]),
        None => Value::record::<&str>([]),
    };
    o.cut.set(value);
}

/// The folder on screen, to make or paste things in: not one inside a zip
/// file, which Pane opens read-only.
fn current_folder(app: &AppRef) -> Option<PathBuf> {
    if app::showing_zip(app) {
        app.borrow().toast(READ_ONLY_ZIP);
        return None;
    }
    match &app.borrow().tab().history.current {
        Location::Folder(p) => Some(p.clone()),
        _ => None,
    }
}

const READ_ONLY_ZIP: &str = "Pane opens zip files read-only: copy items out of it to change them.";
const IN_RECYCLE_BIN: &str = "Restore it first: items in the Recycle Bin can only be restored or deleted.";

fn in_recycle_bin(app: &AppRef) -> bool {
    app.borrow().tab().history.current == Location::RecycleBin
}

// ---------------------------------------------------------------- recycle bin

/// Puts the selected items back where they were deleted from ("name (2)"
/// when that name is taken now), then shows how it went.
pub fn restore(app: &AppRef) {
    let items = app::selected_recycled(app);
    if items.is_empty() {
        return;
    }
    let mut restored: Vec<PathBuf> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for item in &items {
        match pane_shell::recycle::restore(item) {
            Ok(to) => restored.push(to),
            Err(e) => failed.push(format!("{}: {e}", item.name())),
        }
    }
    app::select_none(app);
    app::refresh(app);
    app::refresh_places(app);
    let a = app.borrow();
    if let Some(first) = failed.first() {
        a.toast(&if failed.len() == 1 { format!("Couldn't restore {first}") } else { format!("{} items couldn't be restored. {first}", failed.len()) });
    } else if restored.len() == 1 {
        a.toast(&format!("Restored to {}.", restored[0].display()));
    } else {
        a.toast(&format!("Restored {} items.", restored.len()));
    }
}

/// Deletes the selected items for good (the UI has asked): what each holds
/// and its record, as one operation with a progress card (a deleted folder
/// can be big).
pub fn purge(ops: &OpsRef, app: &AppRef) {
    let items = app::selected_recycled(app);
    if items.is_empty() {
        return;
    }
    let label = format!(
        "Deleting {} for good",
        if items.len() == 1 { items[0].name() } else { count(items.len(), "item") }
    );
    let mut paths: Vec<PathBuf> = Vec::new();
    for item in &items {
        paths.push(item.data.clone());
        paths.push(item.info.clone());
    }
    app::select_none(app);
    run(ops, app, Op::Delete { paths, permanent: true }, label, "delete", None);
}

// ---------------------------------------------------------------- actions

pub fn copy(ops: &OpsRef, app: &AppRef, cut: bool) {
    if in_recycle_bin(app) {
        app.borrow().toast(IN_RECYCLE_BIN);
        return;
    }
    let mut paths = app.borrow().tab().selected_paths();
    if paths.is_empty() {
        return;
    }
    // From a zip: taken out to a temporary folder first, so a paste
    // anywhere (Pane, Explorer, other apps) gets real files.
    if app::showing_zip(app) {
        if cut {
            app.borrow().toast(READ_ONLY_ZIP);
            return;
        }
        let mut out = Vec::new();
        for p in &paths {
            match app::extract_from_zip(p) {
                Ok(o) => out.push(o),
                Err(e) => {
                    app.borrow().toast(&e);
                    return;
                }
            }
        }
        paths = out;
    }
    if !pane_shell::clipboard::set_files(hwnd(), &paths, cut) {
        app.borrow().toast("Couldn't use the clipboard; another app may be holding it.");
        return;
    }
    ops.borrow_mut().cut_set = if cut {
        current_folder(app).map(|dir| {
            (dir, paths.iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect())
        })
    } else {
        None
    };
    publish_cut(ops, app);
    crate::app::refresh_clipboard(app);
}

pub fn paste(ops: &OpsRef, app: &AppRef) {
    let Some(dest) = current_folder(app) else { return };
    let Some((files, cut)) = pane_shell::clipboard::get_files(hwnd()) else {
        return;
    };
    let names = count(files.len(), "item");
    let op = if cut { Op::Move { from: files, to: dest.clone() } } else { Op::Copy { from: files, to: dest.clone() } };
    let label = format!("{} {names} to {}", if cut { "Moving" } else { "Copying" }, folder_name(&dest));
    let ops2 = ops.clone();
    let app2 = app.clone();
    run(
        ops,
        app,
        op,
        label,
        if cut { "move" } else { "copy" },
        Some(Box::new(move |_| {
            if cut {
                pane_shell::clipboard::clear(hwnd());
                ops2.borrow_mut().cut_set = None;
                publish_cut(&ops2, &app2);
                crate::app::refresh_clipboard(&app2);
            }
        })),
    );
}

/// Deletes the selection: to the Recycle Bin, or for good with `permanent`
/// (the UI has asked; `confirmed` says so).
pub fn delete(ops: &OpsRef, app: &AppRef, permanent: bool, confirmed: bool) {
    // In the bin, Delete is for good (the UI asks first).
    if in_recycle_bin(app) {
        if confirmed {
            purge(ops, app);
        }
        return;
    }
    if app::showing_zip(app) {
        app.borrow().toast(READ_ONLY_ZIP);
        return;
    }
    let paths = app.borrow().tab().selected_paths();
    if paths.is_empty() || (permanent && !confirmed) {
        return;
    }
    let label = format!(
        "{} {}",
        if permanent { "Deleting" } else { "Moving to Recycle Bin:" },
        if paths.len() == 1 { folder_name(&paths[0]) } else { count(paths.len(), "item") }
    );
    app::select_none(app);
    run(ops, app, Op::Delete { paths, permanent }, label, "delete", None);
}

/// Files dropped on the sidebar's Recycle Bin: to the bin, as Delete does
/// (Windows asks first, as its own confirmation setting says). Not from
/// inside a zip, which is read-only.
pub fn recycle(ops: &OpsRef, app: &AppRef, paths: Vec<PathBuf>) {
    let paths: Vec<PathBuf> = paths.into_iter().filter(|p| !app::in_zip(p) && p.exists()).collect();
    if paths.is_empty() {
        return;
    }
    let label = format!(
        "Moving to Recycle Bin: {}",
        if paths.len() == 1 { folder_name(&paths[0]) } else { count(paths.len(), "item") }
    );
    run(ops, app, Op::Delete { paths, permanent: false }, label, "delete", None);
}

pub fn rename(ops: &OpsRef, app: &AppRef, index: usize, name: &str) {
    if in_recycle_bin(app) {
        app.borrow().toast(IN_RECYCLE_BIN);
        return;
    }
    if app::showing_zip(app) {
        app.borrow().toast(READ_ONLY_ZIP);
        return;
    }
    let Some(path) = app.borrow().tab().path_of(index) else { return };
    let current = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name == current {
        return;
    }
    if let Err(why) = pane_shell::fileops::check_name(name) {
        app.borrow().toast(&why);
        return;
    }
    let label = format!("Renaming {current}");
    let new_name = name.to_string();
    let app2 = app.clone();
    run(
        ops,
        app,
        Op::Rename { path, name: new_name.clone() },
        label,
        "rename",
        Some(Box::new(move |out| {
            if out.failed.is_empty() && !out.aborted {
                app::select_name_after_load(&app2, &new_name);
            }
        })),
    );
}

pub fn new_folder(ops: &OpsRef, app: &AppRef) {
    let Some(dir) = current_folder(app) else { return };
    let name = pane_shell::fileops::unique_name(&dir, "New folder");
    let ops2 = ops.clone();
    let app2 = app.clone();
    let created = name.clone();
    run(
        ops,
        app,
        Op::NewFolder { parent: dir, name },
        "Creating a folder".into(),
        "new",
        Some(Box::new(move |out| {
            if out.failed.is_empty() {
                // Select it and start renaming, as Explorer does.
                app::select_name_after_load(&app2, &created);
                request_rename(&ops2, &created);
            }
        })),
    );
}

/// The New menu's files: an empty file of `kind` ("text", "markdown"),
/// selected and renaming as a new folder is.
pub fn new_file(ops: &OpsRef, app: &AppRef, kind: &str) {
    let Some(dir) = current_folder(app) else { return };
    let (stem, ext) = match kind {
        "text" => ("New Text Document", "txt"),
        "markdown" => ("New Markdown Document", "md"),
        _ => return,
    };
    let name = pane_shell::fileops::unique_file_name(&dir, stem, ext);
    match std::fs::File::create_new(dir.join(&name)) {
        Ok(_) => {
            app::select_name_after_load(app, &name);
            request_rename(ops, &name);
            app::refresh(app);
        }
        Err(e) => app.borrow().toast(&format!("Couldn't create {name}: {e}")),
    }
}

/// Asks the UI to put row `name` into rename mode.
pub fn request_rename(ops: &OpsRef, name: &str) {
    ops.borrow().rename.set(Value::record([
        ("id", Value::from(name)),
        ("at", Value::from(pane_scan::model::now_secs() as f64 * 1000.0 + rand_ms())),
    ]));
}

fn rand_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.subsec_millis() as f64)
}

/// Files dropped (from Pane or another app) on folder `target`: moved
/// within a drive, copied across drives, as Explorer does; `copy` (Ctrl)
/// always copies.
pub fn drop_files(ops: &OpsRef, app: &AppRef, paths: Vec<PathBuf>, target: PathBuf, copy: bool) {
    let paths: Vec<PathBuf> = paths
        .into_iter()
        // Not onto itself, into its own subfolder, or where it already is.
        .filter(|p| !target.starts_with(p) && p.parent() != Some(target.as_path()))
        .collect();
    if paths.is_empty() || !target.is_dir() {
        return;
    }
    let same_drive = paths.iter().all(|p| drive(p) == drive(&target));
    let moving = same_drive && !copy;
    let label = format!(
        "{} {} to {}",
        if moving { "Moving" } else { "Copying" },
        count(paths.len(), "item"),
        folder_name(&target)
    );
    let op = if moving { Op::Move { from: paths, to: target } } else { Op::Copy { from: paths, to: target } };
    run(ops, app, op, label, if moving { "move" } else { "copy" }, None);
}

/// Deletes the contents of the given folders for good (the Overview's
/// temp cleanup, after the UI confirmed): what is in use is skipped.
pub fn purge_contents(ops: &OpsRef, app: &AppRef, folders: Vec<PathBuf>, then: Box<dyn FnOnce(&OpOutcome)>) {
    let mut children = Vec::new();
    for f in &folders {
        if let Ok(rd) = std::fs::read_dir(f) {
            children.extend(rd.flatten().map(|e| e.path()));
        }
    }
    if children.is_empty() {
        return;
    }
    run(ops, app, Op::Purge { paths: children }, "Clearing temporary files".into(), "delete", Some(then));
}

fn drive(p: &Path) -> String {
    p.components().next().map(|c| c.as_os_str().to_string_lossy().to_ascii_uppercase()).unwrap_or_default()
}

fn folder_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}


#[cfg(test)]
mod tests {
    use super::{short_path, time_left};
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn short_paths() {
        assert_eq!(short_path(Path::new(r"D:\Backup")), r"D:\Backup");
        assert_eq!(short_path(Path::new(r"C:\Users\me\Temp\optest\dest")), r"C:\…\optest\dest");
    }

    #[test]
    fn time_left_reads_well() {
        let s = Duration::from_secs;
        assert_eq!(time_left(s(1), 0.5), "");
        assert_eq!(time_left(s(10), 0.01), "");
        assert_eq!(time_left(s(10), 1.0), "");
        assert_eq!(time_left(s(4), 0.5), "A few seconds left");
        assert_eq!(time_left(s(12), 0.5), "15 s left");
        assert_eq!(time_left(s(60), 0.25), "3 min left");
        assert_eq!(time_left(s(600), 0.1), "1 h 30 min left");
    }
}
