//! Pane: the window, the Luau UI host, and the bridge between them and the
//! file system core. Milestone 1 spike: browse folders, a virtual list fed
//! from Rust, and Explorer's own right-click menu.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use pane_fs::{Entry, SortKey};
use zgx_desktop::{Application, Glass, GlassMaterial, Titlebar, WindowOptions};
use zgx_script::{ScriptHost, Shared, Value};

/// What the window shows, kept by Rust; the UI reads it through `Shared`s.
struct Model {
    path: PathBuf,
    entries: Vec<Entry>,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    /// Bumped per navigation so a slow listing that finishes late is dropped.
    generation: u64,
}

struct Ui {
    rows: Shared,
    path: Shared,
    status: Shared,
    nav: Shared,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui_dir = ui_dir();
    let start = std::env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(home_dir);

    Application::new()
        .window(WindowOptions {
            title: "Pane".into(),
            width: 1180.,
            height: 760.,
            min_size: Some((640., 420.)),
            titlebar: Titlebar::Inset,
            glass: Some(Glass::new(GlassMaterial::WindowBackground)),
            ..Default::default()
        })
        .run(move |cx| {
            pane_shell::init_com();
            let host = ScriptHost::new(&cx.ui.runtime, ui_dir.join("app.luau")).app_id("pane");
            // `ui.window.titlebar_area` / `controls`: the toolbar stays clear
            // of Windows' caption buttons under the inset title bar.
            host.window_chrome(cx.window_info.titlebar_area.clone(), cx.window_info.controls.clone());
            host.window_state(&cx.ui);
            host.window_size(cx.viewport.clone());

            let ui = Rc::new(Ui {
                rows: host.shared("rows", Value::List(Rc::from(Vec::new()))),
                path: host.shared("path", ""),
                status: host.shared("status", ""),
                nav: host.shared("nav", Value::Nil),
            });
            let model = Rc::new(RefCell::new(Model {
                path: start.clone(),
                entries: Vec::new(),
                back: Vec::new(),
                forward: Vec::new(),
                generation: 0,
            }));
            let tasks = cx.tasks.clone();
            let load: Rc<dyn Fn(PathBuf)> = Rc::new({
                let (model, ui) = (model.clone(), ui.clone());
                move |path| load_folder(&model, &ui, &tasks, path)
            });

            let navigate = {
                let (model, load) = (model.clone(), load.clone());
                move |path: PathBuf| {
                    let mut m = model.borrow_mut();
                    if m.path != path {
                        let previous = std::mem::replace(&mut m.path, path.clone());
                        m.back.push(previous);
                        m.forward.clear();
                    }
                    drop(m);
                    load(path);
                }
            };

            host.action("open", {
                let (model, navigate) = (model.clone(), navigate.clone());
                move |arg| {
                    let Some(i) = index_arg(&arg) else { return };
                    let m = model.borrow();
                    let Some(entry) = m.entries.get(i) else { return };
                    let target = m.path.join(&entry.name);
                    let is_dir = entry.is_dir;
                    drop(m);
                    if is_dir {
                        navigate(target);
                    } else {
                        pane_shell::open_path(pane_shell::active_window(), &target);
                    }
                }
            });
            host.action("navigate", {
                let navigate = navigate.clone();
                move |arg| {
                    if let Value::Str(s) = arg {
                        navigate(PathBuf::from(&*s));
                    }
                }
            });
            host.action("up", {
                let (model, navigate) = (model.clone(), navigate.clone());
                move |_| {
                    let parent = model.borrow().path.parent().map(Path::to_path_buf);
                    if let Some(parent) = parent {
                        navigate(parent);
                    }
                }
            });
            host.action("back", {
                let (model, load) = (model.clone(), load.clone());
                move |_| {
                    let mut m = model.borrow_mut();
                    let Some(prev) = m.back.pop() else { return };
                    let current = std::mem::replace(&mut m.path, prev.clone());
                    m.forward.push(current);
                    drop(m);
                    load(prev);
                }
            });
            host.action("forward", {
                let (model, load) = (model.clone(), load.clone());
                move |_| {
                    let mut m = model.borrow_mut();
                    let Some(next) = m.forward.pop() else { return };
                    let current = std::mem::replace(&mut m.path, next.clone());
                    m.back.push(current);
                    drop(m);
                    load(next);
                }
            });
            host.action("refresh", {
                let (model, load) = (model.clone(), load.clone());
                move |_| {
                    let path = model.borrow().path.clone();
                    load(path);
                }
            });
            host.action("context_menu", {
                let model = model.clone();
                let load = load.clone();
                move |arg| {
                    let Some(i) = index_arg(&arg) else { return };
                    let m = model.borrow();
                    let Some(entry) = m.entries.get(i) else { return };
                    let target = m.path.join(&entry.name);
                    let folder = m.path.clone();
                    drop(m);
                    match pane_shell::show_context_menu(pane_shell::active_window(), &[target]) {
                        // Anything that may have changed the folder: list it again.
                        Ok(pane_shell::MenuOutcome::Invoked(_)) => load(folder),
                        Ok(pane_shell::MenuOutcome::Dismissed) => {}
                        Err(e) => eprintln!("context menu failed: {e}"),
                    }
                }
            });

            if let Err(e) = host.reload() {
                eprintln!("script error: {e}");
            }
            load(start.clone());

            let watch = host.watch(Duration::from_millis(250));
            cx.tasks.spawn(watch);
            cx.render(host.view());
            cx.on_closed(move || host.save());
        })
}

/// Lists `path` on a worker thread and publishes it when done.
fn load_folder(model: &Rc<RefCell<Model>>, ui: &Rc<Ui>, tasks: &zgx_desktop::TaskSpawner, path: PathBuf) {
    let generation = {
        let mut m = model.borrow_mut();
        m.generation += 1;
        m.generation
    };
    ui.path.set(Value::from(path.to_string_lossy().as_ref()));
    publish_nav(model, ui);

    let (tx, rx) = futures_channel::oneshot::channel();
    let worker_path = path.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let result = pane_fs::list_dir(&worker_path).map(|mut entries| {
            entries.retain(|e| !(e.hidden || e.system));
            pane_fs::sort_entries(&mut entries, SortKey::Name, false);
            // Text formatting is done here, off the UI thread; the UI thread
            // only wraps the strings into script values.
            let texts: Vec<RowText> = entries.iter().map(RowText::of).collect();
            (entries, texts)
        });
        let _ = tx.send((result, started.elapsed()));
    });

    let (model, ui) = (model.clone(), ui.clone());
    tasks.spawn(async move {
        let Ok((result, list_time)) = rx.await else { return };
        if model.borrow().generation != generation {
            return;
        }
        let publish_start = Instant::now();
        match result {
            Ok((entries, texts)) => {
                let rows: Vec<Value> = entries.iter().zip(&texts).map(|(e, t)| t.value(e)).collect();
                let count = rows.len();
                ui.rows.set(Value::List(Rc::from(rows)));
                model.borrow_mut().entries = entries;
                let message = format!(
                        "{count} items · listed in {:.1} ms · published in {:.1} ms",
                        list_time.as_secs_f64() * 1e3,
                        publish_start.elapsed().as_secs_f64() * 1e3
                    );
                // Also on stderr, so performance checks can read it from a script.
                eprintln!("[pane] {}: {message}", path.display());
                ui.status.set(Value::from(message.as_str()));
            }
            Err(e) => {
                ui.rows.set(Value::List(Rc::from(Vec::new())));
                model.borrow_mut().entries.clear();
                ui.status.set(Value::from(format!("Can't open this folder: {e}").as_str()));
            }
        }
    });
}

fn publish_nav(model: &Rc<RefCell<Model>>, ui: &Ui) {
    let m = model.borrow();
    let crumbs: Vec<Value> = breadcrumbs(&m.path)
        .into_iter()
        .map(|(label, path)| {
            Value::record([("label", Value::from(label.as_str())), ("path", Value::from(path.as_str()))])
        })
        .collect();
    ui.nav.set(Value::record([
        ("can_back", Value::from(!m.back.is_empty())),
        ("can_forward", Value::from(!m.forward.is_empty())),
        ("can_up", Value::from(m.path.parent().is_some())),
        ("crumbs", Value::List(Rc::from(crumbs))),
    ]));
}

/// The path split into clickable parts: ("C:", "C:\"), ("Users", "C:\Users")...
fn breadcrumbs(path: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut acc = PathBuf::new();
    for comp in path.components() {
        acc.push(comp.as_os_str());
        let label = match comp {
            std::path::Component::Prefix(p) => p.as_os_str().to_string_lossy().into_owned(),
            std::path::Component::RootDir => {
                // Fold the root into the drive: "C:\" shows as "C:".
                if let Some((_, last)) = out.last_mut() {
                    *last = acc.to_string_lossy().into_owned();
                }
                continue;
            }
            other => other.as_os_str().to_string_lossy().into_owned(),
        };
        out.push((label, acc.to_string_lossy().into_owned()));
    }
    out
}

/// A row's display text, made on the worker thread (script values are not Send).
struct RowText {
    kind: String,
    date: String,
    size: String,
}

impl RowText {
    fn of(e: &Entry) -> Self {
        let kind = if e.is_dir {
            "File folder".to_string()
        } else {
            match e.extension() {
                ext if ext.is_empty() => "File".to_string(),
                ext => format!("{} File", ext.to_ascii_uppercase()),
            }
        };
        RowText {
            kind,
            date: pane_fs::format_modified(e.modified),
            size: if e.is_dir { String::new() } else { pane_fs::format_size(e.size) },
        }
    }

    fn value(&self, e: &Entry) -> Value {
        Value::record([
            ("id", Value::from(e.name.as_str())),
            ("name", Value::from(e.name.as_str())),
            ("dir", Value::from(e.is_dir)),
            ("kind", Value::from(self.kind.as_str())),
            ("date", Value::from(self.date.as_str())),
            ("size", Value::from(self.size.as_str())),
        ])
    }
}

/// Lua indexes from 1; Rust from 0.
fn index_arg(arg: &Value) -> Option<usize> {
    match arg {
        Value::Num(n) if *n >= 1.0 => Some(*n as usize - 1),
        _ => None,
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"))
}

/// The Luau UI: beside the executable in a release, the source tree in development.
fn ui_dir() -> PathBuf {
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("ui")))
        && dir.join("app.luau").exists()
    {
        return dir;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui")
}
