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
use crate::session::{SavedTab, Session};
use crate::settings::{LoadNote, Settings};
use crate::preview::{self, Preview};
use crate::search;
use crate::sizes::Sizes;
use crate::thumbs::{self, Thumbs};

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
    /// A name to select once the next listing has it (a new or renamed item).
    pub pending_select: Option<String>,
    /// The search running for this tab, cancelled when it's replaced or the
    /// tab closes.
    pub search: Option<search::Cancel>,
}

impl Drop for Tab {
    fn drop(&mut self) {
        self.cancel_search();
    }
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
            pending_select: None,
            search: None,
        }
    }

    fn cancel_search(&mut self) {
        if let Some(cancel) = self.search.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// The full path of row `i`.
    pub fn path_of(&self, i: usize) -> Option<PathBuf> {
        match &self.history.current {
            Location::ThisPc => self.roots.get(i).cloned(),
            Location::Overview => None,
            // A search result's name is its path below the folder searched.
            Location::Folder(dir) | Location::Search { root: dir, .. } => {
                self.entries.get(i).map(|e| dir.join(&e.name))
            }
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
    /// Files on the clipboard: { count, cut } ("Paste 2 items").
    clipboard: Shared,
    /// The details view's group headers: { folders, files, files_size }
    /// when folders come first in a folder, else nil.
    groups: Shared,
}

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub settings: Settings,
    settings_path: PathBuf,
    next_id: u64,
    out: Out,
    icons: Icons,
    pub thumbs: Thumbs,
    preview: Preview,
    sizes: Sizes,
    pub tasks: TaskSpawner,
    watcher: Option<(PathBuf, pane_fs::Watcher)>,
    watch_tx: UnboundedSender<PathBuf>,
    save_pending: bool,
    session_path: PathBuf,
    session_pending: bool,
    /// The clipboard's sequence number when it was last read.
    clip_seq: u32,
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
            status: host.shared("status", Value::record::<&str>([])),
            places: host.shared("places", Value::Nil),
            settings: host.shared("settings", json_to_value(&settings.to_json())),
            toast: host.shared("toast", Value::Nil),
            clipboard: host.shared(
                "clipboard",
                Value::record([("count", Value::from(0.0)), ("cut", Value::from(false))]),
            ),
            groups: host.shared("groups", Value::Nil),
        };
        let (icons, icon_loop) = Icons::start(host.clone());
        tasks.spawn(icon_loop);
        let (thumbs, thumb_loop) = Thumbs::start(host.clone(), settings.thumbnail_threads as usize);
        tasks.spawn(thumb_loop);
        let (preview, preview_loop) = Preview::start(host.clone());
        tasks.spawn(preview_loop);
        let (sizes, sizes_loop) = Sizes::start(host);
        tasks.spawn(sizes_loop);
        let (watch_tx, watch_rx) = unbounded();
        let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"));

        let session_path = Session::path(&settings_path);
        // The tabs left open last time, unless Pane was asked to open a
        // folder (a folder opened with it, Win+E with a start folder).
        let restored = match &start {
            None if settings.restore_session => {
                Session::load(&session_path).and_then(|s| s.usable(|p| p.is_dir()))
            }
            _ => None,
        };
        let first = start.map(Location::Folder).unwrap_or_else(|| start_location(&settings, &home));
        let view = settings.default_view.clone();
        let (tabs, active) = match restored {
            Some(s) => {
                let tabs: Vec<Tab> = s
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let mut tab = Tab::new(i as u64 + 1, Location::from_ui(&t.location), &t.view);
                        tab.sort = sort_key(&t.sort);
                        tab.descending = t.descending;
                        tab
                    })
                    .collect();
                let active = s.active.min(tabs.len() - 1);
                (tabs, active)
            }
            None => (vec![Tab::new(1, first, &view)], 0),
        };
        let next_id = tabs.len() as u64 + 1;
        let app = Rc::new(RefCell::new(App {
            tabs,
            active,
            settings,
            settings_path,
            next_id,
            out,
            icons,
            thumbs,
            preview,
            sizes,
            tasks: tasks.clone(),
            watcher: None,
            watch_tx,
            save_pending: false,
            session_path,
            session_pending: false,
            clip_seq: 0,
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
        load(&app, active, false);
        app
    }

    /// The open tabs, to reopen next time. Search results reopen as the
    /// folder searched.
    fn session(&self) -> Session {
        let tabs = self
            .tabs
            .iter()
            .map(|t| {
                let location = match &t.history.current {
                    Location::Search { root, .. } => Location::Folder(root.clone()),
                    other => other.clone(),
                };
                SavedTab {
                    location: location.to_ui(),
                    view: t.view.clone(),
                    sort: sort_name(t.sort).into(),
                    descending: t.descending,
                }
            })
            .collect();
        Session { tabs, active: self.active }
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
    /// The full date and time, for the date's tooltip.
    date_full: String,
    /// Changed in the last hour (the date gets an amber dot).
    recent: bool,
    /// The extension, shown as a tag when names hide it ("" for none).
    ext: String,
    size: String,
    icon: String,
    /// The thumbnail's host image name, "" for none (see thumbs.rs).
    thumb: String,
}

impl RowText {
    fn value(&self) -> Value {
        Value::record([
            ("id", Value::from(self.id.as_str())),
            ("name", Value::from(self.name.as_str())),
            ("dir", Value::from(self.dir)),
            ("kind", Value::from(self.kind.as_str())),
            ("date", Value::from(self.date.as_str())),
            ("date_full", Value::from(self.date_full.as_str())),
            ("recent", Value::from(self.recent)),
            ("ext", Value::from(self.ext.as_str())),
            ("size", Value::from(self.size.as_str())),
            ("icon", Value::from(self.icon.as_str())),
            ("thumb", Value::from(self.thumb.as_str())),
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
    /// Thumbnails for pictures and videos here (Settings: on, and on
    /// network drives too or not).
    thumbnails: bool,
    /// Dates as "Today 9:26 AM", seen from `now` (local) / `now_ft`.
    relative_dates: bool,
    now: Option<pane_fs::date::LocalTime>,
    now_ft: u64,
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
            thumbnails: a.settings.thumbnails
                && (a.settings.thumbnails_network
                    || !a.tabs[index].history.current.folder().is_some_and(pane_shell::is_network_path)),
            relative_dates: a.settings.relative_dates,
            now: pane_fs::date::local_time(pane_fs::date::now_filetime()),
            now_ft: pane_fs::date::now_filetime(),
        };
        let tab = &mut a.tabs[index];
        tab.cancel_search();
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

    if let Location::Search { root, query } = &location {
        start_search(app, index, generation, tab_id, root.clone(), query.clone(), opts);
        return;
    }

    let (tx, mut rx) = futures_channel::mpsc::unbounded::<Listed>();
    let worker_location = location.clone();
    std::thread::spawn(move || {
        // A navigation into a huge folder shows its first rows while the
        // rest is read; a refresh keeps the rows on screen until it's done.
        let early_tx = tx.clone();
        let early = move |listing: Listing| {
            let _ = early_tx.unbounded_send(Listed::Early(listing));
        };
        let early: Option<&dyn Fn(Listing)> = if keep_selection { None } else { Some(&early) };
        let _ = tx.unbounded_send(Listed::Done(list_location(&worker_location, opts, early)));
    });

    let app = app.clone();
    let tasks = app.borrow().tasks.clone();
    tasks.spawn(async move {
        // Whether the first rows were shown (and could have been selected).
        let mut early_shown = false;
        let result = loop {
            let Some(listed) = rx.next().await else { return };
            let Some(index) = app.borrow().tabs.iter().position(|t| t.id == tab_id) else { return };
            if app.borrow().tabs[index].generation != generation {
                return;
            }
            match listed {
                Listed::Done(result) => break result,
                Listed::Early(listing) => {
                    show_early(&app, index, &location, listing);
                    early_shown = true;
                }
            }
        };
        let Some(index) = app.borrow().tabs.iter().position(|t| t.id == tab_id) else { return };
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
                    tab.selection = if keep_selection || early_shown {
                        tab.selection.remap(&old_names, &new_names)
                    } else {
                        Selection::default()
                    };
                    tab.rows = Value::List(Rc::from(rows));
                    tab.error = None;
                    if let Some(name) = tab.pending_select.take() {
                        match listing.entries.iter().position(|e| e.name.eq_ignore_ascii_case(&name)) {
                            Some(i) => tab.selection.click(i, false, false),
                            None => tab.pending_select = Some(name), // not listed yet: next time
                        }
                    }
                    let count = listing.entries.len();
                    tab.entries = listing.entries;
                    tab.roots = listing.roots;
                    // Icons for what is shown, at the size the view needs.
                    let keys: Vec<String> = listing.texts.into_iter().map(|t| t.icon).collect();
                    let size = icon_size(&tab.view);
                    let folder = location.folder().map(Path::to_path_buf);
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

/// What the listing worker sends: a huge folder's first rows, then all.
enum Listed {
    Early(Listing),
    Done(Result<Listing, String>),
}

/// How many entries a folder needs before its first rows are shown early.
const EARLY_ROWS: usize = 1000;

/// Shows a huge folder's first rows while the rest is still being read.
/// The tab stays loading: no folder sizes or group headers yet.
fn show_early(app: &AppRef, index: usize, location: &Location, listing: Listing) {
    let took = listing.took;
    let publish_start = Instant::now();
    {
        let mut a = app.borrow_mut();
        let tab = &mut a.tabs[index];
        tab.rows = Value::List(listing.texts.iter().map(RowText::value).collect());
        tab.error = None;
        tab.entries = listing.entries;
        tab.roots.clear();
        if let Some(folder) = location.folder() {
            let keys: Vec<String> = listing.texts.into_iter().map(|t| t.icon).collect();
            let size = icon_size(&a.tabs[index].view);
            a.icons.request_entries(folder, &a.tabs[index].entries, &keys, size);
        }
    }
    if index == app.borrow().active {
        publish_rows(app);
        eprintln!(
            "[pane] {}: first {} rows · listed in {:.1} ms · published in {:.1} ms",
            location.address(),
            app.borrow().tabs[index].entries.len(),
            took.as_secs_f64() * 1e3,
            publish_start.elapsed().as_secs_f64() * 1e3
        );
    }
}

/// A folder's entries filtered, sorted and turned into row texts.
fn folder_listing(mut entries: Vec<Entry>, dir: &Path, opts: &ListOptions, started: Instant) -> Listing {
    entries.retain(|e| (opts.show_hidden || !e.hidden) && (opts.show_system || !(e.system && e.hidden)));
    pane_fs::sort_entries(&mut entries, opts.sort);
    let texts = entries.iter().map(|e| row_text(e, dir, opts, false)).collect();
    Listing { entries, roots: Vec::new(), texts, took: started.elapsed() }
}

/// Lists `location`. With `early`, a folder of more than [`EARLY_ROWS`]
/// entries also hands it its first ones, sorted, while the rest is read.
fn list_location(location: &Location, opts: ListOptions, early: Option<&dyn Fn(Listing)>) -> Result<Listing, String> {
    let started = Instant::now();
    match location {
        // The Overview has no listing: storage.rs publishes its contents.
        // The Overview has no listing (storage.rs publishes it); a search
        // streams its own (start_search).
        Location::Overview | Location::Search { .. } => {
            Ok(Listing { entries: Vec::new(), roots: Vec::new(), texts: Vec::new(), took: started.elapsed() })
        }
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
                    date_full: String::new(),
                    recent: false,
                    ext: String::new(),
                    size: if d.total > 0 { pane_fs::format_size(d.total) } else { String::new() },
                    icon: format!("p:{}", d.root.display()),
                    thumb: String::new(),
                });
                roots.push(d.root);
            }
            Ok(Listing { entries, roots, texts, took: started.elapsed() })
        }
        Location::Folder(dir) => {
            let first = if early.is_some() { EARLY_ROWS } else { usize::MAX };
            let entries = pane_fs::list_dir_early(dir, first, |first| {
                if let Some(early) = early {
                    early(folder_listing(first.to_vec(), dir, &opts, started));
                }
            })
            .map_err(|e| friendly_error(dir, &e))?;
            Ok(folder_listing(entries, dir, &opts, started))
        }
    }
}

/// A row's texts. `search`: the entry's name is a path below `dir`; the row
/// shows the file's own name, and where it is in place of its type.
fn row_text(e: &Entry, dir: &Path, opts: &ListOptions, search: bool) -> RowText {
    let ext = e.extension();
    let (shown, parent) = match e.name.rsplit_once('\\') {
        Some((parent, name)) if search => (name, parent),
        _ => (e.name.as_str(), ""),
    };
    let kind = if e.is_dir { "Folder".to_string() } else { kind_name(&ext, shown) };
    let hide_ext = !opts.show_extensions && !e.is_dir && !ext.is_empty();
    let name = if hide_ext { shown[..shown.len() - ext.len() - 1].to_string() } else { shown.to_string() };
    let local = pane_fs::date::local_time(e.modified);
    let date_full = local.map(|t| pane_fs::date::absolute(&t)).unwrap_or_default();
    let date = match (local, opts.now) {
        (Some(then), Some(now)) if opts.relative_dates => pane_fs::date::relative(&then, &now),
        _ => date_full.clone(),
    };
    let recent = e.modified != 0 && opts.now_ft >= e.modified && opts.now_ft - e.modified < pane_fs::date::HOUR;
    let kind = if search {
        let top = Location::Folder(dir.to_path_buf()).title();
        if parent.is_empty() { top } else { format!("{top}\\{parent}") }
    } else {
        kind
    };
    RowText {
        id: e.name.clone(),
        name,
        dir: e.is_dir,
        kind,
        date,
        date_full,
        recent,
        ext: if hide_ext { ext } else { String::new() },
        size: if e.is_dir { String::new() } else { pane_fs::format_size(e.size) },
        icon: icons::key_for(e, dir),
        thumb: if opts.thumbnails { thumbs::name_for(e, dir) } else { String::new() },
    }
}

/// A file's kind in words: "Markdown", "Rust source", or "PDF file" for
/// an extension without a name here.
fn kind_name(ext: &str, name: &str) -> String {
    let known = match ext {
        "" => match name.to_ascii_lowercase().as_str() {
            "license" | "licence" | "copying" => "License",
            ".gitignore" | ".gitattributes" | ".gitmodules" => "Git file",
            ".editorconfig" | ".env" => "Settings file",
            "makefile" => "Makefile",
            "dockerfile" => "Dockerfile",
            _ => "File",
        },
        "md" | "markdown" => "Markdown",
        "txt" | "text" => "Text",
        "log" => "Log",
        "rs" => "Rust source",
        "py" => "Python script",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" | "mts" => "TypeScript",
        "tsx" | "jsx" => "React component",
        "lua" | "luau" => "Luau script",
        "c" | "h" => "C source",
        "cpp" | "cc" | "hpp" => "C++ source",
        "cs" => "C# source",
        "go" => "Go source",
        "java" => "Java source",
        "ps1" => "PowerShell script",
        "bat" | "cmd" => "Batch script",
        "sh" => "Shell script",
        "html" | "htm" => "Web page",
        "css" => "Stylesheet",
        "json" => "JSON",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "xml" => "XML",
        "csv" => "CSV",
        "ini" | "cfg" | "conf" => "Settings file",
        "lock" => "Lockfile",
        "gitignore" | "gitattributes" => "Git file",
        "pdf" => "PDF",
        "doc" | "docx" => "Word document",
        "xls" | "xlsx" => "Excel workbook",
        "ppt" | "pptx" => "PowerPoint deck",
        "jpg" | "jpeg" | "jfif" => "JPEG image",
        "png" => "PNG image",
        "gif" => "GIF image",
        "webp" => "WebP image",
        "svg" => "SVG image",
        "bmp" => "Bitmap image",
        "ico" => "Icon",
        "heic" | "heif" => "HEIC image",
        "psd" => "Photoshop image",
        "mp4" | "m4v" => "MP4 video",
        "mov" => "QuickTime video",
        "mkv" => "MKV video",
        "webm" => "WebM video",
        "avi" => "AVI video",
        "mp3" => "MP3 audio",
        "wav" => "WAV audio",
        "flac" => "FLAC audio",
        "m4a" | "aac" => "AAC audio",
        "ogg" | "opus" => "Ogg audio",
        "zip" => "ZIP archive",
        "7z" => "7-Zip archive",
        "rar" => "RAR archive",
        "tar" | "gz" | "tgz" | "xz" => "Archive",
        "exe" => "Application",
        "msi" => "Installer",
        "dll" => "Library",
        "lnk" => "Shortcut",
        "url" => "Web shortcut",
        "iso" => "Disc image",
        "ttf" | "otf" | "woff" | "woff2" => "Font",
        "blend" => "Blender file",
        "fbx" | "obj" | "glb" | "gltf" => "3D model",
        // A long or numbered tail ("tmp.8540.29c7101d5650") isn't a type.
        _ if ext.len() > 6 || ext.bytes().any(|b| b.is_ascii_digit()) && ext.len() > 4 => "File",
        _ => return format!("{} file", ext.to_ascii_uppercase()),
    };
    known.to_string()
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
    let (parent, came_from) = {
        let a = app.borrow();
        let current = &a.tab().history.current;
        let name = match current {
            Location::Folder(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()),
            _ => None,
        };
        (current.parent(), name)
    };
    if let Some(parent) = parent {
        navigate(app, parent);
        // Select the folder we came out of, as Explorer does.
        if let Some(name) = came_from {
            select_name_after_load(app, &name);
        }
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

/// Shows the active tab's cached state at once, listing it if it never was
/// (a tab reopened from last session, shown for the first time).
fn show_active(app: &AppRef) {
    publish_tabs(app);
    publish_rows(app);
    publish_nav(app);
    ensure_watch(app);
    let (never_listed, i) = {
        let a = app.borrow();
        (a.tab().generation == 0, a.active)
    };
    if never_listed {
        load(app, i, false);
    }
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

/// Selects `name` in the active tab once it is listed (after a rename or a
/// new folder, whose listing arrives a moment later).
pub fn select_name_after_load(app: &AppRef, name: &str) {
    app.borrow_mut().tab_mut().pending_select = Some(name.to_string());
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
            Location::Folder(dir) | Location::Search { root: dir, .. } => {
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
    const RELIST: &[&str] = &[
        "show_hidden",
        "show_system",
        "show_extensions",
        "folders_first",
        "natural_sort",
        "relative_dates",
        "folder_sizes",
        "thumbnails",
        "thumbnails_network",
    ];
    if keys.is_empty() || keys.iter().any(|k| RELIST.contains(k)) {
        let n = app.borrow().tabs.len();
        for i in 0..n {
            load(app, i, true);
        }
    }
    if keys.is_empty() || keys.iter().any(|k| k.starts_with("sidebar_") || *k == "pinned") {
        refresh_places(app);
    }
    if keys.is_empty() || keys.iter().any(|k| k.starts_with("preview_")) {
        update_preview(app);
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

/// Settings → Advanced → Clear caches: thumbnails are made again.
pub fn clear_caches(app: &AppRef, host: &ScriptHost) {
    app.borrow().thumbs.clear(host);
    app.borrow().toast("Caches cleared.");
}

/// Settings → Advanced: the folder with settings.json and session.json.
pub fn open_settings_folder(app: &AppRef) {
    let a = app.borrow();
    if let Some(dir) = a.settings_path.parent() {
        let _ = std::fs::create_dir_all(dir);
        pane_shell::open_path(pane_shell::active_window(), dir);
    }
}

/// Settings → Advanced → Export: the settings, to a file the person chose.
pub fn export_settings(app: &AppRef, path: &Path) {
    let a = app.borrow();
    match a.settings.save(path) {
        Ok(()) => a.toast(&format!("Settings exported to {}.", path.display())),
        Err(e) => a.toast(&format!("Couldn't export settings: {e}")),
    }
}

/// Settings → Advanced → Import: settings from a file exported before.
pub fn import_settings(app: &AppRef, path: &Path) {
    let loaded = std::fs::read_to_string(path).ok().and_then(|t| Settings::from_text(&t));
    match loaded {
        Some(s) => {
            app.borrow_mut().settings = s;
            settings_changed(app, &[]);
            app.borrow().toast("Settings imported.");
        }
        None => app.borrow().toast(&format!("{} isn't a Pane settings file.", path.display())),
    }
}

/// Saves now (the window is closing).
pub fn save_now(app: &AppRef) {
    let a = app.borrow();
    let _ = a.settings.save(&a.settings_path);
    let _ = a.session().save(&a.session_path);
}

/// Saves the open tabs a second after they last changed, so a crash or a
/// power cut still reopens them.
fn schedule_session_save(app: &AppRef) {
    if std::mem::replace(&mut app.borrow_mut().session_pending, true) {
        return;
    }
    let app2 = app.clone();
    let tasks = app.borrow().tasks.clone();
    tasks.spawn(async move {
        zgx::timer::sleep(Duration::from_secs(1)).await;
        let mut a = app2.borrow_mut();
        a.session_pending = false;
        let _ = a.session().save(&a.session_path);
    });
}

fn sort_name(key: SortKey) -> &'static str {
    match key {
        SortKey::Name => "name",
        SortKey::Modified => "date",
        SortKey::Type => "type",
        SortKey::Size => "size",
    }
}

fn sort_key(name: &str) -> SortKey {
    match name {
        "date" => SortKey::Modified,
        "type" => SortKey::Type,
        "size" => SortKey::Size,
        _ => SortKey::Name,
    }
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
        Location::ThisPc | Location::Overview | Location::Search { .. } => None,
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

/// Reads the places and drives again (free space changes after a copy).
pub fn refresh_places(app: &AppRef) {
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
        // Every drive, even with the sidebar's hidden (the footer shows the
        // current one's free space).
        let drives: Vec<Value> = drives
            .iter()
            .map(|d| {
                let used = used_share(d.total.saturating_sub(d.free), d.total);
                let (detail, free, of) = if d.total > 0 {
                    (
                        format!("{} free of {}", pane_fs::format_size(d.free), pane_fs::format_size(d.total)),
                        format!("{} free", pane_fs::format_size(d.free)),
                        used_of(d.total - d.free, d.total),
                    )
                } else {
                    Default::default()
                };
                place(
                    &d.label,
                    &d.root,
                    vec![
                        ("used", Value::from(used)),
                        ("detail", Value::from(detail.as_str())),
                        ("free_text", Value::from(free.as_str())),
                        ("used_of", Value::from(of.as_str())),
                    ],
                )
            })
            .collect();
        a.out.places.set(Value::record([
            ("pinned", Value::List(Rc::from(pinned))),
            ("known", Value::List(Rc::from(known))),
            ("drives", Value::List(Rc::from(drives))),
            ("show_drives", Value::from(s.sidebar_drives)),
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
    drop(a);
    schedule_session_save(app);
}

fn publish_rows(app: &AppRef) {
    let a = app.borrow();
    a.out.rows.set(a.tab().rows.clone());
    publish_groups(&a);
    drop(a);
    request_sizes(app);
    publish_selection(app);
}

/// Folders and files as two groups in details view, when folders come
/// first in a folder (not search results, This PC or the Overview).
fn publish_groups(a: &App) {
    let tab = a.tab();
    let grouped = a.settings.folders_first
        && matches!(tab.history.current, Location::Folder(_))
        && !tab.entries.is_empty()
        && !tab.loading;
    if !grouped {
        a.out.groups.set(Value::Nil);
        return;
    }
    let folders = tab.entries.iter().filter(|e| e.is_dir).count();
    let files = tab.entries.len() - folders;
    let file_bytes: u64 = tab.entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
    a.out.groups.set(Value::record([
        ("folders", Value::from(folders as f64)),
        ("files", Value::from(files as f64)),
        ("files_size", Value::from(pane_fs::format_size(file_bytes).as_str())),
    ]));
}

/// Sizes of the active tab's subfolders, worked out in the background
/// (sizes.rs): in folders on local drives, when the setting is on.
fn request_sizes(app: &AppRef) {
    let a = app.borrow();
    let tab = a.tab();
    let folder = match &tab.history.current {
        Location::Folder(dir) if a.settings.folder_sizes && !tab.loading && !pane_shell::is_network_path(dir) => dir,
        _ => {
            a.sizes.clear();
            return;
        }
    };
    let names: Vec<String> = tab.entries.iter().filter(|e| e.is_dir).map(|e| e.name.clone()).collect();
    let file_bytes = tab.entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
    a.sizes.measure(folder, names, file_bytes);
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
    let sort = sort_name(tab.sort);
    let location = &tab.history.current;
    a.out.nav.set(Value::record([
        ("can_back", Value::from(tab.history.can_back())),
        ("can_forward", Value::from(tab.history.can_forward())),
        ("can_up", Value::from(location.parent().is_some())),
        ("crumbs", Value::List(Rc::from(crumbs))),
        ("address", Value::from(location.address().as_str())),
        // A search's folder: rows' ids are paths below it.
        (
            "path",
            Value::from(
                match location {
                    Location::Search { root, .. } => root.display().to_string(),
                    _ => location.to_ui(),
                }
                .as_str(),
            ),
        ),
        (
            "search",
            match location {
                Location::Search { query, .. } => Value::from(query.as_str()),
                _ => Value::Nil,
            },
        ),
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
    let summary = selection_summary(tab);
    a.out.sel.set(Value::record([
        ("ids", Value::record(ids)),
        ("count", Value::from(tab.selection.len() as f64)),
        ("focus", Value::from(focus)),
        ("files", Value::from(summary.files as f64)),
        ("folders", Value::from(summary.folders as f64)),
        ("size", summary.size.map_or(Value::from(""), |b| Value::from(pane_fs::format_size(b).as_str()))),
    ]));
    a.out.status.set(status_value(tab));
    drop(a);
    update_preview(app);
    refresh_clipboard(app);
}

/// How many files and folders are selected, and the files' total size
/// (in a folder, when any files are selected).
struct SelectionSummary {
    files: usize,
    folders: usize,
    size: Option<u64>,
}

fn selection_summary(tab: &Tab) -> SelectionSummary {
    let selected: Vec<&Entry> = tab.selection.indices().filter_map(|i| tab.entries.get(i)).collect();
    let files: Vec<&&Entry> = selected.iter().filter(|e| !e.is_dir).collect();
    let size = (!files.is_empty() && tab.history.current.folder().is_some()).then(|| files.iter().map(|e| e.size).sum());
    SelectionSummary { files: files.len(), folders: selected.len() - files.len(), size }
}

/// Publishes how many files are on the clipboard, when it has changed since
/// it was last read (any app may change it: checked as the selection
/// changes, after copy and cut, and when the window comes to the front).
pub fn refresh_clipboard(app: &AppRef) {
    let seq = pane_shell::clipboard::sequence();
    if std::mem::replace(&mut app.borrow_mut().clip_seq, seq) == seq {
        return;
    }
    let (count, cut) = pane_shell::clipboard::get_files(pane_shell::active_window().0 as isize)
        .map_or((0, false), |(files, cut)| (files.len(), cut));
    app.borrow().out.clipboard.set(Value::record([
        ("count", Value::from(count as f64)),
        ("cut", Value::from(cut)),
    ]));
}

/// Shows the selection in the preview pane, if it's open: one item, or how
/// many are selected.
fn update_preview(app: &AppRef) {
    let a = app.borrow();
    if !a.settings.preview_pane {
        a.preview.clear();
        return;
    }
    let tab = a.tab();
    let target = match tab.selection.len() {
        0 => preview::Target::Nothing,
        1 => {
            let i = tab.selection.indices().next().expect("one selected");
            match preview_item(tab, i) {
                // The pane shows the large icon while loading, and for files
                // without a picture or text: the list may only have the small.
                Some(item) => {
                    a.icons.request(&item.icon, &item.path, Size::Large);
                    preview::Target::Item(item)
                }
                None => preview::Target::Nothing,
            }
        }
        n => preview::Target::Many(n),
    };
    a.preview.show(target, a.settings.preview_max_kb);
}

fn preview_item(tab: &Tab, i: usize) -> Option<preview::Item> {
    let entry = tab.entries.get(i)?;
    let path = match &tab.history.current {
        Location::Folder(dir) | Location::Search { root: dir, .. } => dir.join(&entry.name),
        Location::ThisPc => tab.roots.get(i)?.clone(),
        Location::Overview => return None,
    };
    let Value::List(rows) = &tab.rows else { return None };
    let row = rows.get(i)?;
    let text = |name: &str| match row.field(name) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    };
    Some(preview::Item {
        name: text("name"),
        dir: entry.is_dir,
        kind: text("kind"),
        size: text("size"),
        date: text("date"),
        icon: text("icon"),
        picture: !text("thumb").is_empty(),
        modified: entry.modified,
        path,
    })
}

/// The footer's selection cell: `{ items, selected, size }` ("14 items",
/// "3 selected", "13 KB"; "" when not shown).
fn status_value(tab: &Tab) -> Value {
    let n = tab.entries.len();
    let items = match (&tab.history.current, tab.loading) {
        (Location::Search { .. }, true) => format!("Searching… {n} found"),
        (Location::Search { .. }, false) if n == 1 => "1 result".to_string(),
        (Location::Search { .. }, false) => format!("{n} results"),
        _ if n == 1 => "1 item".to_string(),
        _ => format!("{n} items"),
    };
    let selected = tab.selection.len();
    let (selected_text, size) = if selected == 0 {
        (String::new(), String::new())
    } else {
        (format!("{selected} selected"), selection_summary(tab).size.map(pane_fs::format_size).unwrap_or_default())
    };
    Value::record([
        ("items", Value::from(items.as_str())),
        ("selected", Value::from(selected_text.as_str())),
        ("size", Value::from(size.as_str())),
    ])
}

/// "352 of 476 GB used" (one unit when both share it), else
/// "820 MB of 1.5 GB used".
fn used_of(used: u64, total: u64) -> String {
    let (u, t) = (pane_fs::format_size(used), pane_fs::format_size(total));
    match (u.rsplit_once(' '), t.rsplit_once(' ')) {
        (Some((un, uu)), Some((_, tu))) if uu == tu => format!("{un} of {t} used"),
        _ => format!("{u} of {t} used"),
    }
}

/// How full a drive is, 0..1, in thousandths: free space moves by a few
/// bytes all the time, and the sidebar's drives are read again every 15 s.
/// Rounded, a reading that changes nothing visible publishes nothing, so
/// the meters don't animate (and the window doesn't draw) while idle.
fn used_share(used: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (used as f64 / total as f64 * 1000.0).round() / 1000.0
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

// ---------------------------------------------------------------- search

/// Searches the active tab's folder (or the folder its results came from)
/// for `query`, showing the results in the tab. An empty query leaves the
/// results.
pub fn search(app: &AppRef, query: &str) {
    let (root, current) = {
        let a = app.borrow();
        let current = a.tab().history.current.clone();
        (current.folder().map(Path::to_path_buf), current)
    };
    let Some(root) = root else { return };
    let query = query.trim();
    if query.is_empty() {
        if matches!(current, Location::Search { .. }) {
            navigate(app, Location::Folder(root));
        }
        return;
    }
    navigate(app, Location::Search { root, query: query.to_string() });
}

/// Runs tab `index`'s search, streaming matches into it as they're found.
fn start_search(app: &AppRef, index: usize, generation: u64, tab_id: u64, root: PathBuf, query: String, list: ListOptions) {
    let (opts, cancel) = {
        let mut a = app.borrow_mut();
        let opts = search::Options {
            case_sensitive: a.settings.search_case_sensitive,
            whole_name: a.settings.search_whole_name,
            hidden: a.settings.search_hidden,
            max_depth: a.settings.search_max_depth,
        };
        let cancel: search::Cancel = Default::default();
        let tab = &mut a.tabs[index];
        tab.search = Some(cancel.clone());
        tab.entries.clear();
        tab.roots.clear();
        tab.rows = empty_list();
        tab.error = None;
        (opts, cancel)
    };
    if index == app.borrow().active {
        publish_rows(app);
    }
    let (tx, mut rx) = unbounded::<search::Event>();
    {
        let (root, query) = (root.clone(), query.clone());
        std::thread::spawn(move || {
            search::run(&root, &query, &opts, &cancel, |event| tx.unbounded_send(event).is_ok());
        });
    }
    let app = app.clone();
    let tasks = app.borrow().tasks.clone();
    let started = Instant::now();
    tasks.spawn(async move {
        let mut rows: Vec<Value> = Vec::new();
        while let Some(event) = rx.next().await {
            let Some(index) = app.borrow().tabs.iter().position(|t| t.id == tab_id) else { return };
            if app.borrow().tabs[index].generation != generation {
                return;
            }
            let done = matches!(event, search::Event::Done { .. });
            {
                let mut a = app.borrow_mut();
                let size = icon_size(&a.tabs[index].view);
                match event {
                    search::Event::Found(batch) => {
                        let texts: Vec<RowText> =
                            batch.iter().map(|e| row_text(e, &root, &list, true)).collect();
                        let keys: Vec<String> = texts.iter().map(|t| t.icon.clone()).collect();
                        a.icons.request_entries(&root, &batch, &keys, size);
                        rows.extend(texts.iter().map(RowText::value));
                        let tab = &mut a.tabs[index];
                        tab.entries.extend(batch);
                        tab.rows = Value::List(Rc::from(rows.clone()));
                    }
                    search::Event::Done { folders } => {
                        // Finished: sort as the tab sorts, keeping the selection.
                        let tab = &mut a.tabs[index];
                        let old: Vec<String> = tab.entries.iter().map(|e| e.name.clone()).collect();
                        // By the file's own name (then where it is), not by
                        // its path: "file name\u{1}sub\file name" while sorting.
                        for e in &mut tab.entries {
                            let file = e.name.rsplit('\\').next().unwrap_or(&e.name).to_string();
                            e.name = format!("{file}\u{1}{}", e.name);
                        }
                        pane_fs::sort_entries(&mut tab.entries, list.sort);
                        for e in &mut tab.entries {
                            if let Some((_, path)) = e.name.split_once('\u{1}') {
                                e.name = path.to_string();
                            }
                        }
                        let new: Vec<&str> = tab.entries.iter().map(|e| e.name.as_str()).collect();
                        tab.selection = tab.selection.remap(&old, &new);
                        rows = tab
                            .entries
                            .iter()
                            .map(|e| row_text(e, &root, &list, true).value())
                            .collect();
                        tab.rows = Value::List(Rc::from(rows.clone()));
                        tab.loading = false;
                        tab.search = None;
                        eprintln!(
                            "[pane] search {query:?} in {}: {} found in {folders} folders, {:.0} ms",
                            root.display(),
                            tab.entries.len(),
                            started.elapsed().as_secs_f64() * 1e3
                        );
                    }
                }
            }
            if index == app.borrow().active {
                publish_rows(&app);
                if done {
                    publish_nav(&app);
                }
            }
            if done {
                publish_tabs(&app);
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_read_as_words() {
        assert_eq!(kind_name("md", "README.md"), "Markdown");
        assert_eq!(kind_name("lock", "Cargo.lock"), "Lockfile");
        assert_eq!(kind_name("", "LICENSE"), "License");
        assert_eq!(kind_name("", ".gitignore"), "Git file");
        assert_eq!(kind_name("", "notes"), "File");
        assert_eq!(kind_name("xyz", "a.xyz"), "XYZ file");
        assert_eq!(kind_name("29c7101d5650", "a.tmp.29c7101d5650"), "File");
    }

    #[test]
    fn used_share_ignores_small_changes() {
        const GB: u64 = 1 << 30;
        assert_eq!(used_share(0, 0), 0.0);
        assert_eq!(used_share(300 * GB, 476 * GB), 0.63);
        // A few megabytes written meanwhile: the same share.
        assert_eq!(used_share(300 * GB + (5 << 20), 476 * GB), 0.63);
        assert_eq!(used_share(476 * GB, 476 * GB), 1.0);
    }

    #[test]
    fn used_of_shares_a_unit() {
        const GB: u64 = 1 << 30;
        assert_eq!(used_of(352 * GB, 476 * GB), "352 of 476 GB used");
        assert_eq!(used_of(820 << 20, 3 * GB / 2), "820 MB of 1.5 GB used");
    }
}
