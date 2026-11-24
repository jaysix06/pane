//! The window's state: tabs, each with a location, history, listing and
//! selection; the settings; and what the UI reads, published through
//! `ScriptHost::shared`. Folder listing runs on worker threads; results come
//! back to the UI thread through the window's task queue.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::StreamExt;
use pane_fs::{Entry, SortKey, SortOptions};
use zgx_desktop::TaskSpawner;
use zgx_script::{ScriptHost, Shared, Value};

use crate::icons::{self, Icons, Size};
use crate::nav::{History, Location, parse_address};
use crate::selection::{Move, Selection};
use crate::settings::{LoadNote, Settings};

pub type AppRef = Rc<RefCell<App>>;

pub struct Tab {
    pub id: u64,
    pub history: History,
    pub entries: Vec<Entry>,
    /// For This PC: each row's drive root. Empty for folders.
    pub roots: Vec<PathBuf>,
    /// The published rows, kept to show the tab again at once.
    pub rows: Value,
    pub sort: SortKey,
    pub descending: bool,
    pub view: String,
    pub selection: Selection,
    /// Bumped per load so a slow listing that finishes late is dropped.
    pub generation: u64,
    pub error: Option<String>,
    pub loading: bool,
}

impl Tab {
    fn new(id: u64, start: Location, view: &str) -> Tab {
        Tab {
            id,
            history: History::new(start),
            entries: Vec::new(),
            roots: Vec::new(),
            rows: empty_list(),
            sort: SortKey::Name,
            descending: false,
            view: view.into(),
            selection: Selection::default(),
            generation: 0,
            error: None,
            loading: false,
        }
    }

    /// The full path of row `i`.
    pub fn path_of(&self, i: usize) -> Option<PathBuf> {
        match &self.history.current {
            Location::ThisPc => self.roots.get(i).cloned(),
            Location::Overview => None,
            Location::Folder(dir) => self.entries.get(i).map(|e| dir.join(&e.name)),
        }
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.selection.indices().filter_map(|i| self.path_of(i)).collect()
    }
}

/// What the UI reads.
struct Out {
    tabs: Shared,
    active: Shared,
    rows: Shared,
    nav: Shared,
    sel: Shared,
    status: Shared,
    places: Shared,
    settings: Shared,
    toast: Shared,
}

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub settings: Settings,
    settings_path: PathBuf,
    next_id: u64,
    out: Out,
    icons: Icons,
    tasks: TaskSpawner,
    watcher: Option<(PathBuf, pane_fs::Watcher)>,
    watch_tx: UnboundedSender<PathBuf>,
    save_pending: bool,
    pub home: PathBuf,
}

impl App {
    /// Builds the app, publishes its first state, and starts its background
    /// loops on `tasks`.
    pub fn start(host: &ScriptHost, tasks: &TaskSpawner, start: Option<PathBuf>) -> AppRef {
        let settings_path = Settings::default_path();
        let (settings, note) = Settings::load(&settings_path);
        let out = Out {
            tabs: host.shared("tabs", empty_list()),
            active: host.shared("active", 1.0),
            rows: host.shared("rows", empty_list()),
            nav: host.shared("nav", Value::Nil),
            sel: host.shared(
                "sel",
                Value::record([
                    ("ids", Value::record::<&str>([])),
                    ("count", Value::from(0.0)),
                    ("focus", Value::from(0.0)),
                ]),
            ),
            status: host.shared("status", ""),
            places: host.shared("places", Value::Nil),
            settings: host.shared("settings", json_to_value(&settings.to_json())),
            toast: host.shared("toast", Value::Nil),
        };
        let (icons, icon_loop) = Icons::start(host.clone());
        tasks.spawn(icon_loop);
        let (watch_tx, watch_rx) = unbounded();
        let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"));

        let first = start.map(Location::Folder).unwrap_or_else(|| start_location(&settings, &home));
        let view = settings.default_view.clone();
        let app = Rc::new(RefCell::new(App {
            tabs: vec![Tab::new(1, first, &view)],
            active: 0,
            settings,
            settings_path,
            next_id: 2,
            out,
            icons,
            tasks: tasks.clone(),
            watcher: None,
            watch_tx,
            save_pending: false,
            home,
        }));
        if let LoadNote::Recovered(backup) = note {
            app.borrow().toast(&format!(
                "Your settings file couldn't be read, so defaults are in use. The old file was kept as {}.",
                backup.display()
            ));
        }
        tasks.spawn(watch_loop(app.clone(), watch_rx));
        tasks.spawn(places_loop(app.clone()));
        publish_tabs(&app);
        load(&app, 0, false);
        app
    }

    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }
    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn toast(&self, message: &str) {
        self.out.toast.set(Value::record([
            ("message", Value::from(message)),
            ("at", Value::from(now_ms())),
        ]));
    }

    fn sort_options(&self, tab: &Tab) -> SortOptions {
        SortOptions {
            key: tab.sort,
            descending: tab.descending,
            folders_first: self.settings.folders_first,
            natural: self.settings.natural_sort,
        }
    }
}

// ---------------------------------------------------------------- loading

/// What a worker hands back: plain data (script values are not Send).
struct Listing {
    entries: Vec<Entry>,
    roots: Vec<PathBuf>,
    texts: Vec<RowText>,
    took: Duration,
}

struct RowText {
    id: String,
    name: String,
    dir: bool,
    kind: String,
    date: String,
    size: String,
    icon: String,
}

impl RowText {
    fn value(&self) -> Value {
        Value::record([
            ("id", Value::from(self.id.as_str())),
            ("name", Value::from(self.name.as_str())),
            ("dir", Value::from(self.dir)),
            ("kind", Value::from(self.kind.as_str())),
            ("date", Value::from(self.date.as_str())),
            ("size", Value::from(self.size.as_str())),
            ("icon", Value::from(self.icon.as_str())),
        ])
    }
}

/// Options the worker needs, copied from the settings.
#[derive(Clone, Copy)]
struct ListOptions {
    sort: SortOptions,
    show_hidden: bool,
    show_system: bool,
    show_extensions: bool,
}

/// (Re)lists tab `index`'s location in the background. With
/// `keep_selection`, rows that still exist stay selected (a refresh or a
/// change on disk); otherwise the selection is cleared (a navigation).
pub fn load(app: &AppRef, index: usize, keep_selection: bool) {
    let (generation, tab_id, location, opts) = {
        let mut a = app.borrow_mut();
        let opts = ListOptions {
            sort: a.sort_options(&a.tabs[index]),
            show_hidden: a.settings.show_hidden,
            show_system: a.settings.show_system,
            show_extensions: a.settings.show_extensions,
        };
        let tab = &mut a.tabs[index];
        tab.generation += 1;
        tab.loading = true;
        if !keep_selection {
            tab.selection.clear();
        }
        (tab.generation, tab.id, tab.history.current.clone(), opts)
    };
    if index == app.borrow().active {
        publish_nav(app);
        ensure_watch(app);
    }
    publish_tabs(app);

    let (tx, rx) = futures_channel::oneshot::channel();
    let worker_location = location.clone();
    std::thread::spawn(move || {
        let _ = tx.send(list_location(&worker_location, opts));
    });

    let app = app.clone();
    let tasks = app.borrow().tasks.clone();
    tasks.spawn(async move {
        let Ok(result) = rx.await else { return };
        let Some(index) = app.borrow().tabs.iter().position(|t| t.id == tab_id) else { return };
        if app.borrow().tabs[index].generation != generation {
            return;
        }
        let publish_start = Instant::now();
        let (count, took) = {
            let mut a = app.borrow_mut();
            let tab = &mut a.tabs[index];
            tab.loading = false;
            match result {
                Ok(listing) => {
                    let rows: Vec<Value> = listing.texts.iter().map(RowText::value).collect();
                    let old_names: Vec<String> = tab.entries.iter().map(|e| e.name.clone()).collect();
                    let new_names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
                    tab.selection = if keep_selection {
                        tab.selection.remap(&old_names, &new_names)
                    } else {
                        Selection::default()
                    };
                    tab.rows = Value::List(Rc::from(rows));
                    tab.error = None;
                    let count = listing.entries.len();
                    tab.entries = listing.entries;
                    tab.roots = listing.roots;
                    // Icons for what is shown, at the size the view needs.
                    let keys: Vec<String> = listing.texts.into_iter().map(|t| t.icon).collect();
                    let size = icon_size(&tab.view);
                    let folder = match &location {
                        Location::Folder(p) => Some(p.clone()),
                        Location::ThisPc | Location::Overview => None,
                    };
                    match folder {
                        Some(f) => a.icons.request_entries(&f, &a.tabs[index].entries, &keys, size),
                        None => {
                            for (root, key) in a.tabs[index].roots.iter().zip(&keys) {
                                a.icons.request(key, root, size);
                            }
                        }
                    }
                    (count, Some(listing.took))
                }
                Err(e) => {
                    tab.entries.clear();
                    tab.roots.clear();
                    tab.rows = empty_list();
                    tab.selection.clear();
                    tab.error = Some(e);
                    (0, None)
                }
            }
        };
        if index == app.borrow().active {
            publish_rows(&app);
            publish_nav(&app);
            if let Some(took) = took {
                eprintln!(
                    "[pane] {}: {count} items · listed in {:.1} ms · published in {:.1} ms",
                    location.address(),
                    took.as_secs_f64() * 1e3,
                    publish_start.elapsed().as_secs_f64() * 1e3
                );
            }
        }
        publish_tabs(&app);
    });
}

fn list_location(location: &Location, opts: ListOptions) -> Result<Listing, String> {
    let started = Instant::now();
    match location {
        // The Overview has no listing: storage.rs publishes its contents.
        Location::Overview => Ok(Listing { entries: Vec::new(), roots: Vec::new(), texts: Vec::new(), took: started.elapsed() }),
        Location::ThisPc => {
            let drives = pane_shell::drives();
            let mut entries = Vec::new();
            let mut roots = Vec::new();
            let mut texts = Vec::new();
            for d in drives {
                let used = d.total.saturating_sub(d.free);
                entries.push(Entry {
                    name: d.label.clone(),
                    is_dir: true,
                    size: used,
                    modified: 0,
                    hidden: false,
                    system: false,
                });
                texts.push(RowText {
                    id: d.root.display().to_string(),
                    name: d.label.clone(),
                    dir: true,
                    // "Local Disk · 143 GB free": the size column keeps the total.
                    kind: if d.total > 0 {
                        format!("{} · {} free", drive_kind(d.kind), pane_fs::format_size(d.free))
                    } else {
                        drive_kind(d.kind).into()
                    },
                    date: String::new(),
                    size: if d.total > 0 { pane_fs::format_size(d.total) } else { String::new() },
                    icon: format!("p:{}", d.root.display()),
                });
                roots.push(d.root);
            }
            Ok(Listing { entries, roots, texts, took: started.elapsed() })
        }
        Location::Folder(dir) => {
            let mut entries = pane_fs::list_dir(dir).map_err(|e| friendly_error(dir, &e))?;
            entries.retain(|e| (opts.show_hidden || !e.hidden) && (opts.show_system || !(e.system && e.hidden)));
            pane_fs::sort_entries(&mut entries, opts.sort);
            let texts = entries.iter().map(|e| row_text(e, dir, opts.show_extensions)).collect();
            Ok(Listing { entries, roots: Vec::new(), texts, took: started.elapsed() })
        }
    }
}

fn row_text(e: &Entry, dir: &Path, show_extensions: bool) -> RowText {
    let ext = e.extension();
    let kind = if e.is_dir {
        "File folder".to_string()
    } else if ext.is_empty() {
        "File".to_string()
    } else {
        format!("{} File", ext.to_ascii_uppercase())
    };
    let name = if !show_extensions && !e.is_dir && !ext.is_empty() {
        e.name[..e.name.len() - ext.len() - 1].to_string()
    } else {
        e.name.clone()
    };
    RowText {
        id: e.name.clone(),
        name,
        dir: e.is_dir,
        kind,
        date: pane_fs::format_modified(e.modified),
        size: if e.is_dir { String::new() } else { pane_fs::format_size(e.size) },
        icon: icons::key_for(e, dir),
    }
}

fn friendly_error(dir: &Path, e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(5) => format!("You don't have permission to open {}.", dir.display()),
        Some(2) | Some(3) => format!("{} doesn't exist anymore.", dir.display()),
        Some(21) => format!("{} isn't ready. Insert a disk and try again.", dir.display()),
        Some(53) | Some(67) => format!("Can't reach {}. Check your network connection.", dir.display()),
        _ => format!("Can't open {}: {e}", dir.display()),
    }
}

fn drive_kind(kind: pane_shell::DriveKind) -> &'static str {
    match kind {
        pane_shell::DriveKind::Fixed => "Local Disk",
        pane_shell::DriveKind::Removable => "Removable Disk",
        pane_shell::DriveKind::Network => "Network Drive",
        pane_shell::DriveKind::Optical => "CD Drive",
        pane_shell::DriveKind::Other => "Drive",
    }
}

fn icon_size(view: &str) -> Size {
    if view == "grid" { Size::Large } else { Size::Small }
}

// ---------------------------------------------------------------- navigation

/// Moves the active tab to `to` (a new history step).
pub fn navigate(app: &AppRef, to: Location) {
    let active = {
        let mut a = app.borrow_mut();
        let i = a.active;
        a.tabs[i].history.go(to);
        i
    };
    load(app, active, false);
}

pub fn back(app: &AppRef) {
    let moved = app.borrow_mut().tab_mut().history.back();
    if moved {
        let i = app.borrow().active;
        load(app, i, false);
    }
}

pub fn forward(app: &AppRef) {
    let moved = app.borrow_mut().tab_mut().history.forward();
    if moved {
        let i = app.borrow().active;
        load(app, i, false);
    }
}

pub fn up(app: &AppRef) {
    let parent = app.borrow().tab().history.current.parent();
    if let Some(parent) = parent {
        navigate(app, parent);
    }
}

pub fn refresh(app: &AppRef) {
    let i = app.borrow().active;
    load(app, i, true);
}

/// A typed address. Unknown places get a message instead of a step.
pub fn go_to_address(app: &AppRef, text: &str) {
    let parsed = {
        let a = app.borrow();
        parse_address(text, &a.tab().history.current, &a.home)
    };
    match parsed {
        Some(Location::Folder(p)) if !p.is_dir() => {
            if p.is_file() {
                pane_shell::open_path(pane_shell::active_window(), &p);
            } else {
                app.borrow().toast(&format!("Windows can't find '{}'. Check the spelling and try again.", text.trim()));
            }
        }
        Some(loc) => navigate(app, loc),
        None => {}
    }
}

/// Opens row `i`: a folder or drive in this tab (or a new one), a file
/// with its app.
pub fn open_row(app: &AppRef, i: usize, new_tab: bool) {
    let (path, is_dir) = {
        let a = app.borrow();
        let tab = a.tab();
        let Some(path) = tab.path_of(i) else { return };
        let is_dir = matches!(tab.history.current, Location::ThisPc) || tab.entries.get(i).is_some_and(|e| e.is_dir);
        (path, is_dir)
    };
    if is_dir {
        if new_tab || app.borrow().settings.open_folders_in_new_tab {
            new_tab_at(app, Location::Folder(path), !new_tab);
        } else {
            navigate(app, Location::Folder(path));
        }
    } else if !pane_shell::open_path(pane_shell::active_window(), &path) {
        app.borrow().toast(&format!("Couldn't open {}.", path.display()));
    }
}

/// Opens every selected row (Enter with several selected).
pub fn open_selection(app: &AppRef) {
    let rows: Vec<usize> = app.borrow().tab().selection.indices().collect();
    if rows.len() == 1 {
        open_row(app, rows[0], false);
        return;
    }
    // Several: files open with their apps, folders in new background tabs.
    for i in rows {
        let is_dir = app.borrow().tab().entries.get(i).is_some_and(|e| e.is_dir);
        if is_dir {
            let path = app.borrow().tab().path_of(i);
            if let Some(path) = path {
                new_tab_at(app, Location::Folder(path), false);
            }
        } else {
            open_row(app, i, false);
        }
    }
}

// ---------------------------------------------------------------- tabs

pub fn new_tab_at(app: &AppRef, at: Location, switch: bool) {
    let index = {
        let mut a = app.borrow_mut();
        let id = a.next_id;
        a.next_id += 1;
        let view = a.settings.default_view.clone();
        let insert_at = a.active + 1;
        a.tabs.insert(insert_at, Tab::new(id, at, &view));
        if switch {
            a.active = insert_at;
        }
        insert_at
    };
    load(app, index, false);
    if switch {
        show_active(app);
    }
}

pub fn new_tab(app: &AppRef) {
    let start = {
        let a = app.borrow();
        start_location(&a.settings, &a.home)
    };
    new_tab_at(app, start, true);
}

/// Closes tab `i`; false if it was the last one (the window should close).
pub fn close_tab(app: &AppRef, i: usize) -> bool {
    {
        let mut a = app.borrow_mut();
        if a.tabs.len() <= 1 {
            return false;
        }
        if i >= a.tabs.len() {
            return true;
        }
        a.tabs.remove(i);
        if a.active > i || a.active >= a.tabs.len() {
            a.active = a.active.saturating_sub(1);
        }
    }
    show_active(app);
    true
}

pub fn switch_tab(app: &AppRef, i: usize) {
    {
        let mut a = app.borrow_mut();
        if i >= a.tabs.len() || i == a.active {
            return;
        }
        a.active = i;
    }
    show_active(app);
    // Fresh contents, keeping what was selected.
    load(app, i, true);
}

/// Moves to the next (`+1`) or previous (`-1`) tab, wrapping.
pub fn cycle_tab(app: &AppRef, by: isize) {
    let i = {
        let a = app.borrow();
        let n = a.tabs.len() as isize;
        ((a.active as isize + by).rem_euclid(n)) as usize
    };
    switch_tab(app, i);
}

/// Shows the active tab's cached state at once.
fn show_active(app: &AppRef) {
    publish_tabs(app);
    publish_rows(app);
    publish_nav(app);
    ensure_watch(app);
}

// ---------------------------------------------------------------- selection

pub fn click(app: &AppRef, i: usize, ctrl: bool, shift: bool) {
    {
        let mut a = app.borrow_mut();
        let len = a.tab().entries.len();
        if i >= len {
            return;
        }
        a.tab_mut().selection.click(i, ctrl, shift);
    }
    publish_selection(app);
}

pub fn ensure_selected(app: &AppRef, i: usize) {
    app.borrow_mut().tab_mut().selection.ensure(i);
    publish_selection(app);
}

pub fn select_all(app: &AppRef) {
    {
        let mut a = app.borrow_mut();
        let len = a.tab().entries.len();
        a.tab_mut().selection.select_all(len);
    }
    publish_selection(app);
}

pub fn select_none(app: &AppRef) {
    app.borrow_mut().tab_mut().selection.clear();
    publish_selection(app);
}

pub fn move_focus(app: &AppRef, by: Move, shift: bool, page: usize) {
    {
        let mut a = app.borrow_mut();
        let len = a.tab().entries.len();
        a.tab_mut().selection.move_focus(by, shift, len, page);
    }
    publish_selection(app);
}

pub fn jump(app: &AppRef, prefix: &str) {
    {
        let mut a = app.borrow_mut();
        let tab = a.tab();
        let names: Vec<&str> = tab.entries.iter().map(|e| e.name.as_str()).collect();
        let Some(to) = crate::selection::jump(&names, prefix, tab.selection.focus()) else { return };
        a.tab_mut().selection.click(to, false, false);
    }
    publish_selection(app);
}

// ---------------------------------------------------------------- sort, view, settings

/// A column header click: the same key flips the direction.
pub fn sort_by(app: &AppRef, key: SortKey) {
    {
        let mut a = app.borrow_mut();
        let tab = a.tab_mut();
        if tab.sort == key {
            tab.descending = !tab.descending;
        } else {
            tab.sort = key;
            tab.descending = matches!(key, SortKey::Modified | SortKey::Size);
        }
    }
    refresh(app);
}

pub fn set_view(app: &AppRef, view: &str) {
    if !matches!(view, "details" | "list" | "grid") {
        return;
    }
    {
        let mut a = app.borrow_mut();
        a.tab_mut().view = view.into();
    }
    publish_nav(app);
    // The grid wants bigger icons: ask for those of what is shown.
    if view == "grid" {
        let a = app.borrow();
        let tab = a.tab();
        match &tab.history.current {
            Location::Folder(dir) => {
                let keys: Vec<String> = tab.entries.iter().map(|e| icons::key_for(e, dir)).collect();
                a.icons.request_entries(dir, &tab.entries, &keys, Size::Large);
            }
            Location::ThisPc | Location::Overview => {
                for root in &tab.roots {
                    a.icons.request(&format!("p:{}", root.display()), root, Size::Large);
                }
            }
        }
    }
}

/// One settings field from the UI. Applies it live and saves soon after.
pub fn set_setting(app: &AppRef, key: &str, value: serde_json::Value) {
    let result = app.borrow_mut().settings.set_field(key, value);
    match result {
        Ok(()) => settings_changed(app, &[key]),
        Err(e) => app.borrow().toast(&e),
    }
}

pub fn reset_settings(app: &AppRef, keys: &[&str]) {
    if keys.is_empty() {
        app.borrow_mut().settings = Settings::default();
    } else {
        app.borrow_mut().settings.reset_fields(keys);
    }
    settings_changed(app, keys);
}

fn settings_changed(app: &AppRef, keys: &[&str]) {
    {
        let a = app.borrow();
        a.out.settings.set(json_to_value(&a.settings.to_json()));
    }
    // Settings that change what a listing shows: list again.
    const RELIST: &[&str] =
        &["show_hidden", "show_system", "show_extensions", "folders_first", "natural_sort"];
    if keys.is_empty() || keys.iter().any(|k| RELIST.contains(k)) {
        let n = app.borrow().tabs.len();
        for i in 0..n {
            load(app, i, true);
        }
    }
    if keys.is_empty() || keys.iter().any(|k| k.starts_with("sidebar_") || *k == "pinned") {
        refresh_places(app);
    }
    schedule_save(app);
}

/// Saves the settings 300 ms after the last change (a slider dragged saves once).
fn schedule_save(app: &AppRef) {
    if std::mem::replace(&mut app.borrow_mut().save_pending, true) {
        return;
    }
    let app2 = app.clone();
    let tasks = app.borrow().tasks.clone();
    tasks.spawn(async move {
        zgx::timer::sleep(Duration::from_millis(300)).await;
        let mut a = app2.borrow_mut();
        a.save_pending = false;
        if let Err(e) = a.settings.save(&a.settings_path) {
            a.toast(&format!("Couldn't save settings: {e}"));
        }
    });
}

/// Saves now (the window is closing).
pub fn save_now(app: &AppRef) {
    let a = app.borrow();
    let _ = a.settings.save(&a.settings_path);
}

pub fn pin(app: &AppRef, path: &str) {
    let mut pinned = app.borrow().settings.pinned.clone();
    if !pinned.iter().any(|p| p.eq_ignore_ascii_case(path)) {
        pinned.push(path.to_string());
        set_setting(app, "pinned", serde_json::json!(pinned));
    }
}

pub fn unpin(app: &AppRef, path: &str) {
    let mut pinned = app.borrow().settings.pinned.clone();
    pinned.retain(|p| !p.eq_ignore_ascii_case(path));
    set_setting(app, "pinned", serde_json::json!(pinned));
}

// ---------------------------------------------------------------- watching

/// Watches the active tab's folder (and only it).
fn ensure_watch(app: &AppRef) {
    let mut a = app.borrow_mut();
    let wanted = match &a.tab().history.current {
        Location::Folder(p) => Some(p.clone()),
        Location::ThisPc | Location::Overview => None,
    };
    if a.watcher.as_ref().map(|(p, _)| p) == wanted.as_ref() {
        return;
    }
    a.watcher = None;
    if let Some(path) = wanted {
        let tx = a.watch_tx.clone();
        let sent = path.clone();
        if let Ok(w) = pane_fs::watch(&path, move || {
            let _ = tx.unbounded_send(sent.clone());
        }) {
            a.watcher = Some((path, w));
        }
    }
}

/// Re-lists the active folder when it changes on disk, at most every 250 ms.
async fn watch_loop(app: AppRef, mut rx: UnboundedReceiver<PathBuf>) {
    while let Some(path) = rx.next().await {
        zgx::timer::sleep(Duration::from_millis(250)).await;
        // Drop the burst that came meanwhile (a copy of many files).
        while rx.try_recv().is_ok() {}
        let (is_active_folder, i) = {
            let a = app.borrow();
            (a.tab().history.current == Location::Folder(path), a.active)
        };
        if is_active_folder {
            load(&app, i, true);
        }
    }
}

// ---------------------------------------------------------------- sidebar

/// Refreshes the sidebar's drives every 15 s (a USB stick plugged in).
async fn places_loop(app: AppRef) {
    loop {
        refresh_places(&app);
        zgx::timer::sleep(Duration::from_secs(15)).await;
    }
}

fn refresh_places(app: &AppRef) {
    let (tx, rx) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send((pane_shell::known_folders(), pane_shell::drives()));
    });
    let app = app.clone();
    let tasks = app.borrow().tasks.clone();
    tasks.spawn(async move {
        let Ok((known, drives)) = rx.await else { return };
        let a = app.borrow();
        let s = &a.settings;
        let place = |label: &str, path: &Path, extra: Vec<(&str, Value)>| {
            let key = format!("p:{}", path.display());
            a.icons.request(&key, path, Size::Small);
            let mut fields = vec![
                ("label", Value::from(label)),
                ("path", Value::from(path.display().to_string().as_str())),
                ("icon", Value::from(key.as_str())),
            ];
            fields.extend(extra);
            Value::record(fields)
        };
        let pinned: Vec<Value> = if s.sidebar_quick_access {
            s.pinned
                .iter()
                .map(PathBuf::from)
                .map(|p| {
                    let label = Location::Folder(p.clone()).title();
                    place(&label, &p, vec![("pinned", Value::from(true))])
                })
                .collect()
        } else {
            Vec::new()
        };
        let known: Vec<Value> = if s.sidebar_known_folders {
            known.iter().map(|k| place(k.label, &k.path, vec![("id", Value::from(k.id))])).collect()
        } else {
            Vec::new()
        };
        let drives: Vec<Value> = if s.sidebar_drives {
            drives
                .iter()
                .map(|d| {
                    let used = if d.total > 0 { (d.total - d.free) as f64 / d.total as f64 } else { 0.0 };
                    let detail = if d.total > 0 {
                        format!("{} free of {}", pane_fs::format_size(d.free), pane_fs::format_size(d.total))
                    } else {
                        String::new()
                    };
                    place(&d.label, &d.root, vec![("used", Value::from(used)), ("detail", Value::from(detail.as_str()))])
                })
                .collect()
        } else {
            Vec::new()
        };
        a.out.places.set(Value::record([
            ("pinned", Value::List(Rc::from(pinned))),
            ("known", Value::List(Rc::from(known))),
            ("drives", Value::List(Rc::from(drives))),
        ]));
    });
}

// ---------------------------------------------------------------- publishing

fn publish_tabs(app: &AppRef) {
    let a = app.borrow();
    let tabs: Vec<Value> = a
        .tabs
        .iter()
        .map(|t| {
            Value::record([
                ("id", Value::from(t.id as f64)),
                ("title", Value::from(t.history.current.title().as_str())),
                ("path", Value::from(t.history.current.to_ui().as_str())),
                ("loading", Value::from(t.loading)),
            ])
        })
        .collect();
    a.out.tabs.set(Value::List(Rc::from(tabs)));
    a.out.active.set(Value::from((a.active + 1) as f64));
}

fn publish_rows(app: &AppRef) {
    let a = app.borrow();
    a.out.rows.set(a.tab().rows.clone());
    drop(a);
    publish_selection(app);
}

fn publish_nav(app: &AppRef) {
    let a = app.borrow();
    let tab = a.tab();
    let crumbs: Vec<Value> = tab
        .history
        .current
        .crumbs()
        .into_iter()
        .map(|(label, loc)| {
            Value::record([("label", Value::from(label.as_str())), ("path", Value::from(loc.to_ui().as_str()))])
        })
        .collect();
    let sort = match tab.sort {
        SortKey::Name => "name",
        SortKey::Modified => "date",
        SortKey::Type => "type",
        SortKey::Size => "size",
    };
    let location = &tab.history.current;
    a.out.nav.set(Value::record([
        ("can_back", Value::from(tab.history.can_back())),
        ("can_forward", Value::from(tab.history.can_forward())),
        ("can_up", Value::from(location.parent().is_some())),
        ("crumbs", Value::List(Rc::from(crumbs))),
        ("address", Value::from(location.address().as_str())),
        ("path", Value::from(location.to_ui().as_str())),
        ("this_pc", Value::from(matches!(location, Location::ThisPc))),
        ("overview", Value::from(matches!(location, Location::Overview))),
        ("sort", Value::from(sort)),
        ("descending", Value::from(tab.descending)),
        ("view", Value::from(tab.view.as_str())),
        ("loading", Value::from(tab.loading)),
        ("error", tab.error.as_deref().map_or(Value::Nil, Value::from)),
    ]));
}

fn publish_selection(app: &AppRef) {
    let a = app.borrow();
    let tab = a.tab();
    let ids: Vec<(Rc<str>, Value)> = tab
        .selection
        .indices()
        .filter_map(|i| tab.entries.get(i))
        .map(|e| (Rc::from(e.name.as_str()), Value::from(true)))
        .collect();
    let focus = tab.selection.focus().map_or(0.0, |f| (f + 1) as f64);
    a.out.sel.set(Value::record([
        ("ids", Value::record(ids)),
        ("count", Value::from(tab.selection.len() as f64)),
        ("focus", Value::from(focus)),
    ]));
    a.out.status.set(Value::from(status_text(tab).as_str()));
}

fn status_text(tab: &Tab) -> String {
    let n = tab.entries.len();
    let items = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
    let selected = tab.selection.len();
    if selected == 0 {
        return items;
    }
    let mut text = format!("{items}   {selected} selected");
    let files: Vec<&Entry> = tab.selection.indices().filter_map(|i| tab.entries.get(i)).filter(|e| !e.is_dir).collect();
    if !files.is_empty() && matches!(tab.history.current, Location::Folder(_)) {
        let total: u64 = files.iter().map(|e| e.size).sum();
        text.push_str(&format!("   {}", pane_fs::format_size(total)));
    }
    text
}

// ---------------------------------------------------------------- helpers

pub fn start_location(settings: &Settings, home: &Path) -> Location {
    match settings.start_folder.as_str() {
        "overview" => Location::Overview,
        "home" => Location::Folder(home.to_path_buf()),
        "this_pc" => Location::ThisPc,
        "downloads" => {
            let d = home.join("Downloads");
            Location::Folder(if d.is_dir() { d } else { home.to_path_buf() })
        }
        path if Path::new(path).is_dir() => Location::Folder(PathBuf::from(path)),
        _ => Location::Folder(home.to_path_buf()),
    }
}

fn empty_list() -> Value {
    Value::List(Rc::from(Vec::new()))
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// serde_json to a script value (settings go to the UI this way).
pub fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::from(*b),
        serde_json::Value::Number(n) => Value::from(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::from(s.as_str()),
        serde_json::Value::Array(a) => Value::List(a.iter().map(json_to_value).collect::<Vec<_>>().into()),
        serde_json::Value::Object(m) => {
            Value::record(m.iter().map(|(k, v)| (k.as_str(), json_to_value(v))).collect::<Vec<_>>())
        }
    }
}

/// A script value to serde_json; whole numbers become integers so they fit
/// integer settings.
pub fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => serde_json::json!(*n as i64),
        Value::Num(n) => serde_json::json!(*n),
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Map(fields) => serde_json::Value::Object(
            fields.iter().map(|(k, v)| (k.to_string(), value_to_json(v))).collect(),
        ),
        _ => serde_json::Value::Null,
    }
}
